// Artifact files live in the project folder, so moving a project carries its artifacts (R-13):
//   <project>/.harness/artifacts/<id>/manifest.json + v<n>.<ext>
// Every save is a version; restoring makes a new version rather than rewriting history (R-14).
import { join } from 'node:path';
import { existsSync, mkdirSync, renameSync, cpSync, rmSync } from 'node:fs';
import type { Artifact, ArtifactFormat, ArtifactType, ArtifactVersion } from '../../shared/src/types.ts';
import { renderMarkdown, esc as escHtml } from '../../shared/src/markdown.ts';
import { uid } from '../../shared/src/util.ts';
import { readText, writeText, writeJson } from './jsonfile.ts';
import { expandHome } from './paths.ts';
import type { WorkspaceRuntime } from './runtime.ts';
import { openDesign } from './opendesign.ts';
import { odSettings } from '../../shared/src/defaults.ts';

const FILE_EXT: Record<ArtifactFormat, string> = { markdown: 'md', html: 'html', csv: 'csv', svg: 'svg', json: 'json' };
const EXT_FORMAT: Record<string, ArtifactFormat> = { md: 'markdown', html: 'html', csv: 'csv', svg: 'svg', json: 'json' };

/** The formats each kind can be written in; the first is the default. Docs and decks are Markdown or HTML. */
const FORMATS: Record<ArtifactType, ArtifactFormat[]> = {
  Doc: ['markdown', 'html'], Deck: ['markdown', 'html'], DesignSystem: ['markdown', 'html'], Video: ['markdown', 'html', 'json'],
  Prototype: ['html'], LiveArtifact: ['html'], Sheet: ['csv', 'html'], Image: ['svg', 'html'],
};

const looksHtml = (s: string) => /^\s*(<!doctype html|<html[\s>]|<(head|body|main|section|div|style|svg)[\s>])/i.test(s);

/** The format a save is in: the one asked for if this kind takes it, else HTML when the content is HTML, else the default. */
export function resolveFormat(type: ArtifactType, content: string, requested?: string): ArtifactFormat {
  const allowed = FORMATS[type];
  const r = (requested === 'md' ? 'markdown' : requested) as ArtifactFormat | undefined;
  if (r && allowed.includes(r)) return r;
  if (allowed.includes('html') && looksHtml(content)) return 'html';
  return allowed[0];
}

export function formatOf(ver: Pick<ArtifactVersion, 'format' | 'file'>): ArtifactFormat {
  return ver.format || EXT_FORMAT[ver.file.split('.').pop() || ''] || 'markdown';
}

const latestFormat = (a: Artifact) => formatOf(a.versions[a.versions.length - 1] || { file: 'v.md' });

export function exportsFor(a: Artifact): { label: string; ext: string; format: string }[] {
  const f = latestFormat(a);
  const md = f === 'markdown' ? [{ label: 'Markdown', ext: '.md', format: 'md' }] : [];
  const html = { label: 'Single-file HTML', ext: '.html', format: 'html' }, pdf = { label: 'PDF', ext: '.pdf', format: 'pdf' };
  if (a.type === 'Deck') return [{ label: 'PowerPoint', ext: '.pptx', format: 'pptx' }, pdf, { label: 'Single-file HTML deck', ext: '.html', format: 'html' }, ...md];
  if (a.type === 'Sheet') return f === 'csv' ? [{ label: 'CSV', ext: '.csv', format: 'csv' }, { label: 'HTML table', ext: '.html', format: 'html' }] : [html];
  if (f === 'svg') return [{ label: 'SVG', ext: '.svg', format: 'svg' }, html];
  if (f === 'json') return [{ label: 'Storyboard JSON', ext: '.json', format: 'json' }];
  return [...md, html, pdf];
}

export function normalizeType(t: string): ArtifactType {
  const s = (t || '').toLowerCase();
  if (/deck|slide|present|pptx/.test(s)) return 'Deck';
  if (/proto|html|page|web|site|app|screen/.test(s)) return 'Prototype';
  if (/sheet|csv|table|xlsx|spread/.test(s)) return 'Sheet';
  if (/image|svg|picture/.test(s)) return 'Image';
  if (/video|motion/.test(s)) return 'Video';
  if (/live/.test(s)) return 'LiveArtifact';
  if (/design.?system/.test(s)) return 'DesignSystem';
  return 'Doc';
}

export class ArtifactStore {
  private rt: WorkspaceRuntime;
  constructor(rt: WorkspaceRuntime) { this.rt = rt; }

  dir(a: Pick<Artifact, 'id' | 'project'>): string {
    const p = this.rt.project(a.project);
    if (!p) throw new Error('Project not found');
    return join(expandHome(p.folder), '.harness', 'artifacts', a.id);
  }

  private manifest(a: Artifact): void {
    writeJson(join(this.dir(a), 'manifest.json'), { id: a.id, title: a.title, type: a.type, 'od.mode': odMode(a.type), project: a.project, originSession: a.originSession, originChat: a.chat, designSystem: a.designSystem, versions: a.versions, createdAt: a.createdAt, updatedAt: a.updatedAt });
  }

  get(id: string): Artifact { const a = this.rt.state.artifacts.find((x) => x.id === id); if (!a) throw Object.assign(new Error('Artifact not found'), { status: 404 }); return a; }

  /** Artifacts from a chat go to that chat's project at any depth; from the library a project must be picked (R-13). */
  create(o: { project?: string; chat?: string | null; type: string; title: string; content: string; note?: string; format?: string }): Artifact {
    const chat = o.chat ? this.rt.chat(o.chat) : undefined;
    const project = chat ? chat.project : o.project;
    if (!project || !this.rt.project(project)) throw new Error('Pick a project for this artifact');
    const type = normalizeType(o.type);
    const now = Date.now();
    const a: Artifact = { id: uid('art'), project, chat: chat?.id ?? null, originSession: chat?.sid ?? null, type, title: (o.title || type).slice(0, 120), version: 1, versions: [], designSystem: this.rt.ws.designSystem, createdAt: now, updatedAt: now };
    mkdirSync(this.dir(a), { recursive: true });
    const format = resolveFormat(type, o.content, o.format);
    const file = `v1.${FILE_EXT[format]}`;
    writeText(join(this.dir(a), file), o.content);
    a.versions.push({ v: 1, at: now, chat: chat?.id ?? null, note: o.note || 'created', file, format });
    this.rt.state.artifacts.push(a);
    this.manifest(a);
    this.rt.audit('artifact.create', `${type} · ${a.title}${chat ? ' · from ' + chat.title : ''}`);
    this.rt.changed();
    return a;
  }

  /** A revision from another chat keeps the original project and adds that chat to the history. */
  revise(id: string, content: string, o: { chat?: string | null; note?: string; title?: string; format?: string } = {}): Artifact {
    const a = this.get(id);
    const v = a.version + 1;
    // A revision keeps the format unless it asks for another or is plainly HTML (a Markdown deck rebuilt in HTML).
    const format = resolveFormat(a.type, content, o.format || (looksHtml(content) ? 'html' : latestFormat(a)));
    const file = `v${v}.${FILE_EXT[format]}`;
    writeText(join(this.dir(a), file), content);
    a.version = v;
    a.updatedAt = Date.now();
    if (o.title) a.title = o.title;
    a.versions.push({ v, at: a.updatedAt, chat: o.chat ?? null, note: o.note || 'revised', file, format });
    this.manifest(a);
    this.rt.audit('artifact.version', `${a.title} v${v}`);
    this.rt.changed();
    return a;
  }

  read(id: string, v?: number): { artifact: Artifact; version: number; content: string; format: ArtifactFormat } {
    const a = this.get(id);
    const ver = a.versions.find((x) => x.v === (v || a.version)) || a.versions[a.versions.length - 1];
    return { artifact: a, version: ver.v, content: readText(join(this.dir(a), ver.file)), format: formatOf(ver) };
  }

  /** A version as the panel shows it: its source, plus the page Open Design stores (decks in Open Design's framework). */
  async view(id: string, v?: number): Promise<ReturnType<ArtifactStore['read']> & { html: string }> {
    await openDesign.deckFramework();
    const r = this.read(id, v);
    return { ...r, html: toHtml(r.artifact, r.content, false, r.format) };
  }

  restore(id: string, v: number): Artifact {
    const { content } = this.read(id, v);
    return this.revise(id, content, { note: `restored v${v}` });
  }

  /** Moving the folder keeps provenance: the origin-chat link stays. */
  move(id: string, project: string): Artifact {
    const a = this.get(id);
    if (!this.rt.project(project)) throw new Error('Project not found');
    if (a.project === project) return a;
    const from = this.dir(a);
    const to = this.dir({ id: a.id, project });
    mkdirSync(join(to, '..'), { recursive: true });
    if (existsSync(from)) { try { renameSync(from, to); } catch { cpSync(from, to, { recursive: true }); rmSync(from, { recursive: true, force: true }); } }
    a.project = project;
    this.manifest(a);
    this.rt.audit('artifact.move', `${a.title} → ${this.rt.project(project)?.name}`);
    this.rt.changed();
    return a;
  }

  /** Move the files of every artifact a chat made after the chat moved (runtime.moveChat already re-pointed them). */
  relocate(ids: string[], fromProject: string): void {
    for (const id of ids) {
      const a = this.get(id);
      const from = this.dir({ id, project: fromProject });
      const to = this.dir(a);
      if (from !== to && existsSync(from)) { mkdirSync(join(to, '..'), { recursive: true }); try { renameSync(from, to); } catch { cpSync(from, to, { recursive: true }); rmSync(from, { recursive: true, force: true }); } this.manifest(a); }
    }
  }

  remove(id: string): void {
    const a = this.get(id);
    rmSync(this.dir(a), { recursive: true, force: true });
    this.rt.state.artifacts = this.rt.state.artifacts.filter((x) => x.id !== id);
    this.rt.audit('artifact.delete', a.title);
    this.rt.changed();
  }

  /**
   * Hand the latest version to Open Design: lint it, then write it into the project's Open Design project
   * (rooted at <project>/.harness/artifacts), where Open Design records its own version history.
   * With Open Design as the artifact engine a failed save throws; with the built-in store it is skipped.
   */
  async syncToOpenDesign(id: string, source: 'ai' | 'manual' | 'restore' = 'ai'): Promise<boolean> {
    const a = this.get(id);
    const ver = a.versions[a.versions.length - 1];
    const od = odSettings(this.rt.ws);
    if (od.engine === 'builtin') { ver.od = null; this.rt.save(); return false; }
    const { content, format } = this.read(id, ver.v);
    await openDesign.deckFramework();
    const html = toHtml(a, content, false, format);
    let failure = '';
    const r = await openDesign.saveVersion(this.rt, a, html, source, { lint: od.lint, markdown: format === 'markdown' ? content : undefined }).catch((e) => { failure = String((e as Error).message || e); this.rt.audit('open-design.error', failure, 'workspaceai'); return null; });
    if (!r) { ver.od = null; this.rt.save(); throw new Error(`Open Design could not save “${a.title}” v${ver.v}${failure ? ': ' + failure : ''}. WorkspaceAI kept a copy; resync it from Settings › Open Design.`); }
    ver.od = r;
    this.manifest(a);
    this.rt.audit('open-design.save', `${a.title} v${ver.v} · ${r.lint.length} lint finding${r.lint.length === 1 ? '' : 's'}`, 'workspaceai');
    this.rt.changed();
    return true;
  }

  // Every artifact goes through Open Design when it is the engine: it must be up before anything is written.
  private async ready(): Promise<void> { if (odSettings(this.rt.ws).engine === 'open-design') await openDesign.require(this.rt); }
  async createVia(o: Parameters<ArtifactStore['create']>[0]): Promise<Artifact> { await this.ready(); const a = this.create(o); await this.syncToOpenDesign(a.id, 'ai'); return this.get(a.id); }
  async reviseVia(id: string, content: string, o: Parameters<ArtifactStore['revise']>[2] = {}, source: 'ai' | 'manual' = 'ai'): Promise<Artifact> { await this.ready(); this.revise(id, content, o); await this.syncToOpenDesign(id, source); return this.get(id); }
  async restoreVia(id: string, v: number): Promise<Artifact> { await this.ready(); this.restore(id, v); await this.syncToOpenDesign(id, 'restore'); return this.get(id); }

  /** Push the latest version of every artifact Open Design doesn't have yet (made while it was off or failing). */
  async resyncOpenDesign(): Promise<{ synced: number; failed: { title: string; error: string }[] }> {
    await openDesign.require(this.rt);
    let synced = 0; const failed: { title: string; error: string }[] = [];
    for (const a of this.rt.state.artifacts) {
      if (a.versions[a.versions.length - 1]?.od) continue;
      try { if (await this.syncToOpenDesign(a.id, 'manual')) synced++; } catch (e) { failed.push({ title: a.title, error: (e as Error).message }); }
    }
    return { synced, failed };
  }

  /** HTML goes through Open Design's standalone export when available; PDF/PPTX need its desktop renderer. */
  async exportVia(id: string, format: string, v?: number): Promise<{ filename: string; mime: string; body: string | Buffer; via: 'open-design' | 'harness'; note?: string }> {
    const a = this.get(id);
    const latest = !v || v === a.version;
    const name = (ext: string) => this.export(id, 'html', v).filename.replace(/\.html$/, '.' + ext);
    if ((format === 'html' || format === 'pdf' || format === 'pptx') && latest && a.versions[a.versions.length - 1]?.od) {
      const r = await openDesign.exportFile(this.rt, a, format).catch((e) => ({ ok: false as const, error: String((e as Error).message) }));
      if (r.ok && r.body != null) return { filename: name(format), body: r.body, mime: r.mime || 'application/octet-stream', via: 'open-design' };
      if (format === 'pptx') throw new Error(r.error || 'PowerPoint export failed in Open Design');
      if (format === 'pdf') return { ...this.export(id, 'pdf', v), via: 'harness', note: (r.error || 'Open Design could not render a PDF') + '; printed from HTML instead' };
    }
    if (format === 'pptx') throw new Error('PowerPoint export needs Open Design');
    return { ...this.export(id, format, v), via: 'harness' };
  }

  export(id: string, format: string, v?: number): { filename: string; mime: string; body: string } {
    const { artifact: a, content, version, format: src } = this.read(id, v);
    const base = a.title.replace(/[^\w\- ]+/g, '').trim().replace(/\s+/g, '-').toLowerCase() || 'artifact';
    const fn = (ext: string) => `${base}-v${version}.${ext}`;
    if ((format === 'md' ? 'markdown' : format) === src && src !== 'html') return { filename: fn(format), mime: { md: 'text/markdown', csv: 'text/csv', svg: 'image/svg+xml', json: 'application/json' }[format] as string, body: content };
    if (format === 'html' || format === 'pdf') return { filename: fn('html'), mime: 'text/html', body: toHtml(a, content, format === 'pdf', src) };
    throw new Error('Export format not available for ' + a.type);
  }
}

export function odMode(t: ArtifactType): string {
  return { Doc: 'doc', Deck: 'deck', Prototype: 'prototype', Sheet: 'sheet', Image: 'image', Video: 'video', LiveArtifact: 'live-artifact', DesignSystem: 'design-system' }[t];
}

export function splitSlides(md: string): string[] { return md.split(/\n-{3,}\n/).map((s) => s.trim()).filter(Boolean); }

const PAGE_CSS = 'body{font-family:Inter,system-ui,sans-serif;background:#faf9f5;color:#141413;max-width:820px;margin:40px auto;padding:0 24px;line-height:1.6}h1,h2,h3,h4{font-family:Georgia,serif;font-weight:500;line-height:1.2}a{color:#cc785c}code{font:0.9em ui-monospace,monospace;background:#efe9de;padding:1px 4px;border-radius:4px}pre{background:#efe9de;padding:12px 14px;border-radius:8px;overflow:auto}pre code{background:none;padding:0}blockquote{margin:0;padding-left:16px;border-left:3px solid #cc785c;color:#5e5d59}hr{border:0;border-top:1px solid #e6dfd8;margin:28px 0}table{border-collapse:collapse}td,th{border:1px solid #e6dfd8;padding:6px 10px;text-align:left}.slide{aspect-ratio:16/9;border:1px solid #e6dfd8;border-radius:10px;padding:40px;margin:0 0 24px;page-break-after:always;display:flex;flex-direction:column;justify-content:center}';

/** Slide typography for Markdown decks on Open Design's 1920×1080 canvas (its per-deck style slot). */
const MD_DECK_CSS = `
    .md-slide{height:100%;padding:120px 160px;display:flex;flex-direction:column;justify-content:center;gap:40px;font-family:Inter,system-ui,sans-serif}
    .slide.dark{background:var(--fg)}
    .slide.dark .md-slide,.slide.dark .md-slide p,.slide.dark .md-slide li,.slide.dark .md-slide td,.slide.dark .md-slide th{color:var(--bg)}
    .slide.hero .md-slide h1{font-size:168px}
    .md-slide h1{font:500 128px/1.02 Georgia,serif;letter-spacing:-0.025em}
    .md-slide h2{font:500 96px/1.06 Georgia,serif;letter-spacing:-0.02em}
    .md-slide h3,.md-slide h4{font:600 56px/1.2 Inter,system-ui,sans-serif}
    .md-slide p,.md-slide li,.md-slide td,.md-slide th{font-size:46px;line-height:1.4;color:var(--fg)}
    .md-slide ul,.md-slide ol{padding-left:1.1em;display:flex;flex-direction:column;gap:18px}
    .md-slide blockquote{border-left:6px solid var(--accent);padding-left:36px;color:var(--muted)}
    .md-slide table{border-collapse:collapse}.md-slide td,.md-slide th{border-bottom:2px solid #e6dfd8;padding:14px 28px 14px 0;text-align:left}
    .md-slide code{font-family:ui-monospace,monospace;font-size:0.9em}
    .md-slide a{color:var(--accent)}`;

/** A Markdown deck as an Open Design deck: its fixed framework with one slide per `---` section. */
export function markdownDeck(title: string, md: string, skeleton: string): string {
  const parts = splitSlides(md);
  // Open Design's theme rhythm: a hero cover, then never three same-theme slides in a row, a hero close on long decks.
  const theme = (i: number) => (i === 0 ? 'hero dark' : parts.length >= 8 && i === parts.length - 1 ? 'hero light' : i % 3 === 0 ? 'dark' : 'light');
  const slides = parts.map((s, i) => `      <section class="slide ${theme(i)}${i ? '' : ' active'}" data-od-id="s${i + 1}" data-screen-label="${String(i + 1).padStart(2, '0')}"><div class="md-slide">${renderMarkdown(s, { shift: 0 })}</div></section>`).join('\n');
  const open = skeleton.indexOf('>', skeleton.indexOf('id="deck-stage"')) + 1;
  const chrome = skeleton.indexOf('<!-- Framework chrome');
  const close = skeleton.lastIndexOf('</div>', skeleton.lastIndexOf('</div>', chrome) - 1);
  if (open <= 0 || chrome < 0 || close < open) return '';
  let out = skeleton.slice(0, open) + '\n' + slides + '\n    ' + skeleton.slice(close);
  // The framework's own comment names a slide tag; Open Design's lint would count it as an unlabelled slide.
  out = out.replace('<title><!-- SLOT: deck title --></title>', `<title>${escHtml(title)}</title>`).replace('inside <section class="slide"> bodies', 'inside the slide sections');
  const slot = out.indexOf('*/', out.indexOf('SLOT: per-deck styles'));
  return slot > 0 ? out.slice(0, slot + 2) + MD_DECK_CSS + out.slice(slot + 2) : out;
}

const PRINT = '<script>window.onload=()=>window.print()</script>';

/** The single HTML page for a version: HTML as written, Markdown rendered (decks in Open Design's deck framework). */
export function toHtml(a: Artifact, content: string, print: boolean, format: ArtifactFormat = latestFormat(a)): string {
  const wrap = (body: string) => `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>${escHtml(a.title)}</title><style>${PAGE_CSS}</style></head><body>${body}${print ? PRINT : ''}</body></html>`;
  if (format === 'html' || format === 'svg') {
    const page = format === 'svg' ? `<!doctype html><html><body style="margin:0;display:grid;place-items:center;min-height:100vh">${content}</body></html>` : content;
    return print ? (page.includes('</body>') ? page.replace('</body>', PRINT + '</body>') : page + PRINT) : page;
  }
  if (format === 'csv') return wrap('<table>' + content.split('\n').filter(Boolean).map((r, i) => '<tr>' + r.split(',').map((c) => (i ? `<td>${escHtml(c)}</td>` : `<th>${escHtml(c)}</th>`)).join('') + '</tr>').join('') + '</table>');
  if (format === 'json') return wrap(`<pre><code>${escHtml(content)}</code></pre>`);
  if (a.type === 'Deck') {
    const skeleton = openDesign.deckSkeleton();
    const deck = skeleton && !print ? markdownDeck(a.title, content, skeleton) : '';
    return deck || wrap(splitSlides(content).map((s) => `<section class="slide">${renderMarkdown(s, { shift: 0 })}</section>`).join('\n'));
  }
  return wrap(renderMarkdown(content, { shift: 0 }));
}
