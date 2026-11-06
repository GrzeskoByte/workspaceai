import { describe, it, expect } from 'vitest';
import { renderMarkdown, gitlabMarkdown } from '../src/markdown.ts';

describe('lists', () => {
  it('keeps a flat list as it was', () => {
    expect(renderMarkdown('- a\n- b')).toBe('<ul><li>a</li><li>b</li></ul>');
    expect(renderMarkdown('1. a\n2. b')).toBe('<ol><li>a</li><li>b</li></ol>');
  });
  it('nests by indent and comes back out', () => {
    expect(renderMarkdown('- a\n  - b\n    1. c\n- d')).toBe('<ul><li>a<ul><li>b<ol><li>c</li></ol></li></ul></li><li>d</li></ul>');
  });
  it('does not end on a blank line between items, but does before text', () => {
    expect(renderMarkdown('- a\n\n- b\n\nend')).toBe('<ul><li>a</li><li>b</li></ul><p>end</p>');
  });
  it('shows task items as disabled checkboxes', () => {
    expect(renderMarkdown('- [ ] a\n- [x] b')).toBe('<ul><li class="task"><input type="checkbox" disabled> a</li><li class="task"><input type="checkbox" disabled checked> b</li></ul>');
  });
  it('escapes list text', () => { expect(renderMarkdown('- [ ] <img src=x>')).toContain('&lt;img'); });
});

describe('links', () => {
  it('links a bare address without its trailing punctuation', () => {
    expect(renderMarkdown('See https://a.io/x?y=1&z=2.')).toBe('<p>See <a href="https://a.io/x?y=1&amp;z=2" target="_blank" rel="noreferrer noopener">https://a.io/x?y=1&amp;z=2</a>.</p>');
  });
  it('leaves a written link alone', () => {
    expect(renderMarkdown('[a](https://a.io)')).toBe('<p><a href="https://a.io" target="_blank" rel="noreferrer noopener">a</a></p>');
  });
  it('never makes a link of another scheme', () => {
    expect(renderMarkdown('[a](javascript:alert(1))')).not.toContain('<a ');
  });
});

describe('gitlabMarkdown', () => {
  const url = 'https://gl.io/g/app/-/issues/11';
  it('drops comments and raw tags', () => {
    expect(gitlabMarkdown('<!-- template -->\n<details><summary>More</summary>\nText</details>', url)).toBe('More\nText');
  });
  it('turns images and relative links into links to GitLab', () => {
    expect(gitlabMarkdown('![shot](/uploads/ab/s.png){width=10}', url)).toBe('[shot](https://gl.io/g/app/uploads/ab/s.png){width=10}');
    expect(gitlabMarkdown('![](https://i.io/a.png)', url)).toBe('[Image](https://i.io/a.png)');
    expect(gitlabMarkdown('[mr](/g/app/-/merge_requests/2)', url)).toBe('[mr](https://gl.io/g/app/-/merge_requests/2)');
  });
});
