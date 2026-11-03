// Artifacts are Markdown or HTML: a save keeps the format it asks for (or HTML when it is HTML), and Markdown decks
// are laid onto Open Design's deck framework so the panel, Open Design and exports show the same deck.
import { describe, it, expect } from 'vitest';
import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { resolveFormat, formatOf, exportsFor, markdownDeck, toHtml } from '../src/artifacts.ts';
import { renderMarkdown } from '../../shared/src/markdown.ts';

const art = (type: any, file: string): any => ({ id: 'a', title: 'T', type, versions: [{ v: 1, file }] });

describe('artifact formats', () => {
  it('picks Markdown or HTML per kind', () => {
    expect(resolveFormat('Doc', '# Plan')).toBe('markdown');
    expect(resolveFormat('Doc', '<!doctype html><html></html>')).toBe('html');
    expect(resolveFormat('Doc', '# Plan', 'html')).toBe('html');
    expect(resolveFormat('Deck', '# One\n\n---\n\n# Two', 'md')).toBe('markdown');
    expect(resolveFormat('Prototype', '<div>x</div>', 'markdown')).toBe('html'); // prototypes are HTML only
    expect(resolveFormat('Sheet', 'a,b\n1,2')).toBe('csv');
    expect(formatOf({ file: 'v3.html' })).toBe('html');
    expect(formatOf({ file: 'v1.md', format: 'markdown' })).toBe('markdown');
  });

  it('offers the exports each format can give', () => {
    expect(exportsFor(art('Doc', 'v1.md')).map((f) => f.format)).toEqual(['md', 'html', 'pdf']);
    expect(exportsFor(art('Doc', 'v1.html')).map((f) => f.format)).toEqual(['html', 'pdf']);
    expect(exportsFor(art('Deck', 'v1.html')).map((f) => f.format)).toEqual(['pptx', 'pdf', 'html']);
  });

  it('renders Markdown tables, quotes and rules', () => {
    const html = renderMarkdown('# Title\n\n> said\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n---', { shift: 0 });
    expect(html).toBe('<h1>Title</h1><blockquote><p>said</p></blockquote><table><thead><tr><th>a</th><th>b</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table><hr>');
  });

  it('lays a Markdown deck onto a deck skeleton', () => {
    const skeleton = '<html><head><title><!-- SLOT: deck title --></title><style>/* SLOT: per-deck styles */</style></head><body><div class="deck-shell">\n<div class="deck-stage" id="deck-stage">\n<section class="slide active">SLOT</section>\n</div>\n</div>\n<!-- Framework chrome --><nav></nav></body></html>';
    const out = markdownDeck('Pitch <1>', '# Hello\n\n---\n\n- point', skeleton);
    expect(out).toContain('<title>Pitch &lt;1&gt;</title>');
    expect(out).toContain('<section class="slide hero dark active" data-od-id="s1" data-screen-label="01"><div class="md-slide"><h1>Hello</h1></div></section>');
    expect(out).toContain('<section class="slide light" data-od-id="s2" data-screen-label="02"><div class="md-slide"><ul><li>point</li></ul></div></section>');
    expect(out).not.toContain('SLOT</section>');
    expect(out).toContain('.md-slide{');
    expect(out).toContain('<!-- Framework chrome --><nav></nav>');
  });

  it.runIf(existsSync(join(homedir(), '.harness/vendor/open-design/apps/daemon/dist/prompts/deck-framework.js')))('uses Open Design’s own deck framework when installed', async () => {
    process.env.HARNESS_OD_DIR = join(homedir(), '.harness/vendor/open-design');
    const { openDesign } = await import('../src/opendesign.ts');
    const fw = await openDesign.deckFramework();
    expect(fw?.directive).toContain('Slide deck — fixed framework');
    expect(fw?.directive).not.toContain('Final handoff');
    const html = toHtml(art('Deck', 'v1.md'), '# One\n\n---\n\n# Two', false, 'markdown');
    expect(html).toContain('data-od-deck-protocol="1"');
    expect(html.match(/<div class="md-slide">/g)).toHaveLength(2);
    expect(html).toContain('<section class="slide hero dark active" data-od-id="s1" data-screen-label="01">');
  });
});

describe('Markdown edge cases', () => {
  it('finishes on short table separators and stray pipes', () => {
    expect(renderMarkdown('| a | b |\n| - | - |\n| 1 | 2 |', { shift: 0 })).toContain('<td>1</td>');
    expect(renderMarkdown('| lone |\ntext', { shift: 0 })).toBe('<p>| lone |<br>text</p>');
    expect(renderMarkdown('para\n---\nnext', { shift: 0 })).toBe('<p>para</p><hr><p>next</p>');
  });
});
