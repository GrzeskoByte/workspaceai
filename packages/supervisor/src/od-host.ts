// Hosts one Open Design daemon (R-04), used unmodified: this starts it through its own `startDaemonRuntime` entry
// with the renderer hooks its Electron desktop normally supplies (PDF export, slide/page capture for PPTX and
// screenshot PDF, artifact export), implemented here with headless Chromium. Run as a child process:
//   node od-host.ts <open-design dir> <port>     (OD_DATA_DIR in the environment)
import { createRequire } from 'node:module';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright-core') as typeof import('playwright-core');

type Browser = import('playwright-core').Browser;
type Page = import('playwright-core').Page;

let browser: Promise<Browser> | null = null;
function getBrowser(): Promise<Browser> {
  const exe = [process.env.HARNESS_CHROMIUM, '/usr/bin/chromium', '/usr/bin/chromium-browser', '/Applications/Chromium.app/Contents/MacOS/Chromium'].find((p) => p && existsSync(p));
  browser ??= chromium.launch({ headless: true, ...(exe ? { executablePath: exe } : {}) });
  return browser;
}

function withBase(html: string, baseHref?: string): string {
  if (!baseHref || /<base\s/i.test(html)) return html;
  const tag = `<base href="${baseHref.replace(/"/g, '&quot;')}">`;
  return /<head[^>]*>/i.test(html) ? html.replace(/<head[^>]*>/i, (m) => m + tag) : tag + html;
}

async function open(html: string, baseHref: string | undefined, width: number, height: number): Promise<Page> {
  const page = await (await getBrowser()).newPage({ viewport: { width, height }, deviceScaleFactor: 1 });
  await page.setContent(withBase(html, baseHref), { waitUntil: 'load', timeout: 60000 });
  await page.evaluate(() => (document as any).fonts?.ready).catch(() => {});
  await page.waitForTimeout(150);
  return page;
}

/** Shows only slide `i` of a `section.slide` deck, filling the stage, so one screenshot is one slide. */
async function isolateSlide(page: Page, i: number): Promise<void> {
  await page.evaluate((idx) => {
    const slides = Array.from(document.querySelectorAll('section.slide')) as HTMLElement[];
    let style = document.getElementById('__od_host_style');
    if (!style) {
      style = document.createElement('style'); style.id = '__od_host_style';
      style.textContent = '[data-od-host-hidden]{display:none!important}[data-od-host-current]{visibility:visible!important;opacity:1!important;transform:none!important}html,body{overflow:hidden!important}';
      document.head.appendChild(style);
    }
    slides.forEach((s, n) => {
      if (n === idx) { s.removeAttribute('data-od-host-hidden'); s.setAttribute('data-od-host-current', ''); s.scrollIntoView({ block: 'start' }); }
      else { s.setAttribute('data-od-host-hidden', ''); s.removeAttribute('data-od-host-current'); }
    });
  }, i);
  await page.waitForTimeout(60);
}

function save(outputDir: string | undefined, name: string, buf: Buffer, mime: string): string {
  if (!outputDir) return `data:${mime};base64,${buf.toString('base64')}`;
  mkdirSync(outputDir, { recursive: true });
  const file = join(outputDir, name);
  writeFileSync(file, buf);
  return file;
}

async function renderSlides(input: any): Promise<any> {
  if (input.editable) return { ok: false, errorCode: 'RENDER_FAILED', error: 'Editable PPTX needs the Open Design desktop app; the image-per-slide PPTX works here.' };
  const width = input.width || 1920, height = input.height || 1080;
  const page = await open(input.html, input.baseHref, width, height);
  try {
    const count = await page.locator('section.slide').count();
    const deck = input.deck ?? count > 0;
    const out: string[] = [];
    if (deck) {
      if (!count) return { ok: false, errorCode: 'NO_SLIDES', error: 'No <section class="slide"> found' };
      const which = typeof input.index === 'number' ? [input.index] : Array.from({ length: count }, (_, i) => i);
      if (which.some((i) => i < 0 || i >= count)) return { ok: false, errorCode: 'SLIDE_INDEX_OUT_OF_RANGE', error: `Slide ${input.index} of ${count}` };
      for (const i of which) {
        await isolateSlide(page, i);
        out.push(save(input.outputDir, `slide-${String(i + 1).padStart(3, '0')}.png`, await page.screenshot({ type: 'png' }), 'image/png'));
      }
      const files = input.outputDir ? { slideFiles: out } : { slides: out };
      return { ok: true, mode: 'deck', width, height, ...files };
    }
    const type = input.pageImageFormat === 'jpeg' ? 'jpeg' : 'png';
    const mime = 'image/' + type;
    if (input.paginate) {
      const total = await page.evaluate(() => document.documentElement.scrollHeight);
      for (let y = 0, n = 1; y < total; y += height, n++) {
        await page.evaluate((top) => window.scrollTo(0, top), y);
        await page.waitForTimeout(40);
        out.push(save(input.outputDir, `page-${String(n).padStart(3, '0')}.${type}`, await page.screenshot({ type, ...(type === 'jpeg' ? { quality: 88 } : {}) }), mime));
      }
    } else out.push(save(input.outputDir, `page.${type}`, await page.screenshot({ type, fullPage: true }), mime));
    const h = await page.evaluate(() => document.documentElement.scrollHeight);
    return { ok: true, mode: 'page', width, height: input.paginate ? height : h, ...(input.outputDir ? { slideFiles: out } : { slides: out }) };
  } catch (e) {
    return { ok: false, errorCode: 'RENDER_FAILED', error: String((e as Error).message || e) };
  } finally { await page.close().catch(() => {}); }
}

/** Vector PDF: decks print one slide per 1920×1080 page; documents print with their own CSS. */
async function pdfOf(input: any): Promise<Buffer> {
  const deck = !!input.deck;
  const page = await open(input.html, input.baseHref, deck ? 1920 : 1280, deck ? 1080 : 900);
  try {
    if (deck) {
      await page.addStyleTag({ content: '@page{size:1920px 1080px;margin:0}section.slide{display:block!important;visibility:visible!important;opacity:1!important;width:1920px!important;height:1080px!important;break-after:page;page-break-after:always;overflow:hidden;position:relative!important;transform:none!important}html,body{margin:0!important}' });
      return await page.pdf({ printBackground: true, width: '1920px', height: '1080px', preferCSSPageSize: true });
    }
    return await page.pdf({ printBackground: true, format: 'A4', margin: { top: '16mm', bottom: '16mm', left: '14mm', right: '14mm' } });
  } finally { await page.close().catch(() => {}); }
}

function tmpFile(name: string, buf: Buffer): string {
  const dir = mkdtempSync(join(tmpdir(), 'od-host-'));
  const file = join(dir, name.replace(/[^\w.\- ]+/g, '_') || 'export');
  writeFileSync(file, buf);
  return file;
}

async function exportPdf(input: any): Promise<any> {
  try {
    const buf = await pdfOf(input);
    return { ok: true, path: tmpFile((input.defaultFilename || input.title || 'export').replace(/\.pdf$/i, '') + '.pdf', buf) };
  } catch (e) { return { ok: false, error: String((e as Error).message || e) }; }
}

async function exportArtifact(input: any): Promise<any> {
  try {
    if (input.format === 'pdf') { const buf = await pdfOf(input); return { ok: true, path: tmpFile('export.pdf', buf), mime: 'application/pdf', bytes: buf.length }; }
    const type = input.imageFormat === 'jpeg' ? 'jpeg' : 'png';
    const first = input.captureMode === 'first_viewport_thumbnail';
    const page = await open(input.html, input.baseHref, input.width || (input.deck ? 1920 : 1280), input.height || (input.deck ? 1080 : 800));
    try {
      if (input.deck && (await page.locator('section.slide').count())) await isolateSlide(page, 0);
      const buf = await page.screenshot({ type, fullPage: !first && !input.deck });
      return { ok: true, path: tmpFile('export.' + (type === 'jpeg' ? 'jpg' : 'png'), buf), mime: 'image/' + type, bytes: buf.length };
    } finally { await page.close().catch(() => {}); }
  } catch (e) { return { ok: false, code: 'render_timeout', error: String((e as Error).message || e) }; }
}

async function main(): Promise<void> {
  const [dir, portArg] = process.argv.slice(2);
  const { startDaemonRuntime } = await import(pathToFileURL(join(dir, 'apps/daemon/dist/daemon-startup.js')).href);
  const runtime = await startDaemonRuntime({
    host: '127.0.0.1',
    port: Number(portArg) || 0,
    logListening: false,
    openBrowser: false,
    desktopPdfExporter: exportPdf,
    desktopSlideRenderer: renderSlides,
    desktopArtifactExporter: exportArtifact,
  });
  console.log(`[od] listening on ${runtime.url} (harness renderer)`);
  const stop = () => { void Promise.allSettled([runtime.stop(), browser?.then((b) => b.close())]).finally(() => process.exit(0)); };
  process.on('SIGTERM', stop);
  process.on('SIGINT', stop);
}

if (import.meta.url === pathToFileURL(process.argv[1] || '').href) void main().catch((e) => { console.error(e); process.exit(1); });

export { renderSlides, exportPdf, exportArtifact };
