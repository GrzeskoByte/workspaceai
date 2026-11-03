// Browser steps use Playwright MCP (R-41): open a page and return what it read.
// Headless, with a browser profile per workspace so sign-ins never cross workspaces.
import { createRequire } from 'node:module';
import { mkdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

const require = createRequire(import.meta.url);

export function playwrightCli(): string { return join(dirname(require.resolve('@playwright/mcp/package.json')), 'cli.js'); }

export interface BrowserResult { url: string; title: string; text: string }

function textOf(r: unknown): string {
  const res = r as { content?: { type: string; text?: string }[]; isError?: boolean };
  const text = (res?.content || []).filter((c) => c.type === 'text').map((c) => c.text || '').join('\n');
  if (res?.isError) throw new Error(text.replace(/^### Error\n/, '').slice(0, 400));
  return text;
}

/** Chromium for Playwright: HARNESS_CHROMIUM, else a system Chromium, else Playwright's own download. */
export function chromiumArgs(): string[] {
  const exe = [process.env.HARNESS_CHROMIUM, '/usr/bin/chromium', '/usr/bin/chromium-browser', '/Applications/Chromium.app/Contents/MacOS/Chromium'].find((p) => p && existsSync(p));
  return exe ? ['--browser', 'chromium', '--executable-path', exe] : ['--browser', 'chromium'];
}

/** Open `url` in a headless browser through Playwright MCP and return the page's accessibility snapshot as text. */
export async function browseOnce(url: string, opts: { profileDir?: string; timeoutMs?: number } = {}): Promise<BrowserResult> {
  if (!/^https?:\/\//.test(url)) throw new Error('A Browser step needs an http(s) URL');
  const args = [playwrightCli(), '--headless', ...chromiumArgs()];
  if (opts.profileDir) { mkdirSync(opts.profileDir, { recursive: true }); args.push('--user-data-dir', opts.profileDir); } else args.push('--isolated');
  const transport = new StdioClientTransport({ command: process.execPath, args, stderr: 'pipe' });
  const client = new Client({ name: 'harness-browser-step', version: '0.1.0' });
  const timer = setTimeout(() => { void client.close(); }, opts.timeoutMs ?? 60000);
  try {
    await client.connect(transport);
    const nav = textOf(await client.callTool({ name: 'browser_navigate', arguments: { url } }));
    const title = /Page Title:\s*(.*)/.exec(nav)?.[1]?.trim() || '';
    const snapshot = textOf(await client.callTool({ name: 'browser_snapshot', arguments: {} })) || nav;
    return { url, title, text: snapshot.slice(0, 20000) };
  } finally {
    clearTimeout(timer);
    await client.close().catch(() => {});
  }
}
