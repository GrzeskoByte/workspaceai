// R-41: a Browser step opens a page and returns what it read, through Playwright MCP.
import { describe, it, expect } from 'vitest';
import { createServer } from 'node:http';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { browseOnce } from '../src/browser.ts';

describe('Browser step via Playwright MCP (R-41)', () => {
  it('opens a page and returns its text', async () => {
    const srv = createServer((_q, r) => r.writeHead(200, { 'content-type': 'text/html' }).end('<html><head><title>Castform beta</title></head><body><h1>Castform beta</h1><p>Mold wall minimum is 8 mm.</p></body></html>'));
    await new Promise<void>((r) => srv.listen(0, '127.0.0.1', () => r()));
    const a = srv.address();
    const url = `http://127.0.0.1:${typeof a === 'object' && a ? a.port : 0}/`;
    try {
      const r = await browseOnce(url, { profileDir: join(mkdtempSync(join(tmpdir(), 'harness-pw-')), 'profile') });
      expect(r.title).toBe('Castform beta');
      expect(r.text).toContain('Mold wall minimum is 8 mm.');
    } finally { srv.close(); }
  });
  it('refuses non-http URLs', async () => {
    await expect(browseOnce('file:///etc/passwd')).rejects.toThrow(/http/);
  });
});
