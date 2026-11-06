import { describe, it, expect } from 'vitest';
import { deflateRawSync } from 'node:zlib';
import { readZip, claudeSystemToDesignMd } from '../src/claude-design-system.ts';

/** A minimal .zip writer: each file deflated, or stored when `store` is set. */
function zip(files: Record<string, string | Uint8Array>, store = false): Uint8Array {
  const locals: Buffer[] = [], centrals: Buffer[] = [];
  let offset = 0;
  for (const [name, content] of Object.entries(files)) {
    const raw = Buffer.from(content as string), body = store ? raw : deflateRawSync(raw), n = Buffer.from(name), method = store ? 0 : 8;
    const local = Buffer.alloc(30); local.writeUInt32LE(0x04034b50, 0); local.writeUInt16LE(method, 8); local.writeUInt32LE(body.length, 18); local.writeUInt32LE(raw.length, 22); local.writeUInt16LE(n.length, 26);
    const central = Buffer.alloc(46); central.writeUInt32LE(0x02014b50, 0); central.writeUInt16LE(method, 10); central.writeUInt32LE(body.length, 20); central.writeUInt32LE(raw.length, 24); central.writeUInt16LE(n.length, 28); central.writeUInt32LE(offset, 42);
    locals.push(local, n, body); centrals.push(central, n);
    offset += local.length + n.length + body.length;
  }
  const cd = Buffer.concat(centrals), eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0); eocd.writeUInt16LE(Object.keys(files).length, 8); eocd.writeUInt16LE(Object.keys(files).length, 10); eocd.writeUInt32LE(cd.length, 12); eocd.writeUInt32LE(offset, 16);
  return new Uint8Array(Buffer.concat([...locals, cd, eocd]));
}

const TOKENS = JSON.stringify({
  name: 'From tokens', color: { themes: [{ id: 'light', name: 'Light' }, { id: 'dark', name: 'Dark' }], tokens: [
    { name: 'ink', value: { light: '#111111', dark: '#eeeeee' }, usage: 'Text' },
    { name: 'link', value: '{ink}', usage: 'Links | anchors' },
    { name: 'paper', value: { light: '#ffffff' } },
  ] },
  type: { families: { sans: 'Inter, sans-serif' }, groups: [{ name: 'Text', family: 'sans', styles: [{ name: 'body', fontSize: '15px', lineHeight: 1.5, fontWeight: 400 }] }] },
  spacing: { tokens: [{ name: 'space-4', value: '16px', usage: 'Padding' }] },
  shadow: { tokens: [{ name: 'raise', value: { light: '0 1px 2px #000', dark: 'none' } }] },
});
const SYSTEM = {
  'design-system.json': JSON.stringify({ title: 'Acme' }),
  'tokens.json': TOKENS,
  'README.md': '# Acme\n\nSharp and blue.\n\n## Colour\n\n```md\n# not a heading\n```\n',
  'Voice.md': '# Voice\n\n## Tone\n\nPlain.',
  'components/Button/README.md': '# Button\n\n## States\n\nPrimary only.',
  'components/Button/preview.html': '<script>alert(1)</script>',
  'components/bundle.css': 'a{}',
  'fonts/Acme.woff2': new Uint8Array([1, 2, 3]),
  'assets/Logos/mark.svg': '<svg/>',
  'assets/Logos/README.md': 'Logo notes',
  'api/tokens.md': 'generated',
  'tokens.css': 'generated',
};

describe('claude design system migration', () => {
  it('reads stored and deflated archives, sorting text from the rest', async () => {
    for (const store of [false, true]) {
      const r = await readZip(zip({ 'README.md': '# Hi', 'fonts/a.woff2': new Uint8Array([0]), 'dir/': '' }, store));
      expect(r.text.get('README.md')).toBe('# Hi');
      expect(r.other).toEqual(['fonts/a.woff2']);
    }
  });

  it('turns tokens, the brand book and component guidelines into one DESIGN.md', async () => {
    const { title, markdown, notCarried } = claudeSystemToDesignMd(await readZip(zip(SYSTEM)), 'file');
    expect(title).toBe('Acme');
    expect(markdown.startsWith('# Acme\n\n> Sharp and blue.\n')).toBe(true);
    expect(markdown).toContain('| ink | #111111 | #eeeeee | Text |');
    expect(markdown).toContain('| link | #111111 | #eeeeee | Links \\| anchors |');
    expect(markdown).toContain('| paper | #ffffff | #ffffff |  |');
    expect(markdown).toContain('| body | sans | 15px | 1.5 | 400 |  |  |');
    expect(markdown).toContain('| space-4 | 16px | Padding |');
    expect(markdown).toContain('light: 0 1px 2px #000; dark: none');
    expect(markdown).toContain('### Colour');
    expect(markdown).toContain('```md\n# not a heading');
    expect(markdown).toContain('### Tone');
    expect(markdown).toContain('### Button\n\n#### States\n\nPrimary only.');
    expect(markdown.indexOf('## Colour tokens')).toBeLessThan(markdown.indexOf('### Colour'));
    expect(markdown).not.toContain('alert(1)');
    expect(markdown).not.toContain('generated');
    expect(notCarried).toEqual(['1 component preview', '1 font file', '1 logo or image file', '1 component code file']);
  });

  it('finds the system inside a wrapping folder or project/', async () => {
    const wrapped = Object.fromEntries(Object.entries(SYSTEM).map(([k, v]) => ['Acme/project/' + k, v]));
    const { markdown } = claudeSystemToDesignMd(await readZip(zip({ ...wrapped, 'Acme/project/components/Button/README.md': '# Button\n\nInside.' })), 'file');
    expect(markdown).toContain('### Button\n\nInside.');
    expect(markdown).toContain('| ink |');
  });

  it('names it from its tokens, its heading or the file, in that order', async () => {
    const name = async (f: Record<string, string>) => claudeSystemToDesignMd(await readZip(zip(f)), 'file-name').title;
    expect(await name({ 'tokens.json': TOKENS })).toBe('From tokens');
    expect(await name({ 'README.md': '# Heading\n\nText' })).toBe('Heading');
    expect(await name({ 'README.md': 'Just text' })).toBe('file-name');
  });

  it('refuses what is not a design system, with a reason', async () => {
    await expect(readZip(new TextEncoder().encode('not a zip, and long enough to look for an end record'))).rejects.toThrow('not a .zip');
    const empty = await readZip(zip({ 'photo.png': 'x', 'notes.txt': 'y' }));
    expect(() => claudeSystemToDesignMd(empty, 'f')).toThrow('not a Claude design system');
    const bad = await readZip(zip({ 'tokens.json': '{oops', 'README.md': '# A' }));
    expect(() => claudeSystemToDesignMd(bad, 'f')).toThrow('tokens.json is not valid JSON');
  });

  it('refuses a file that inflates past the limit', async () => {
    await expect(readZip(zip({ 'README.md': 'x'.repeat(3_000_000) }))).rejects.toThrow('too large');
  });
});
