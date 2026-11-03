// Small, safe Markdown renderer shared by the app and the supervisor: everything is escaped first, then a few
// constructs (headings, lists, code, quotes, tables, links) are rendered.

export function esc(s: string): string { return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;'); }

function inline(s: string): string {
  return esc(s)
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>')
    .replace(/(^|[^*])\*([^*\n]+)\*/g, '$1<i>$2</i>')
    .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, '<a href="$2" target="_blank" rel="noreferrer noopener">$1</a>');
}

/** `shift` demotes headings (chat replies render `#` as h2); artifacts keep them as written. */
export function renderMarkdown(md: string, o: { shift?: number } = {}): string {
  const shift = o.shift ?? 1;
  const out: string[] = [];
  const lines = md.replace(/\r/g, '').split('\n');
  let i = 0;
  while (i < lines.length) {
    const l = lines[i];
    if (l.startsWith('```')) {
      const body: string[] = []; i++;
      while (i < lines.length && !lines[i].startsWith('```')) body.push(lines[i++]);
      i++;
      out.push(`<pre><code>${esc(body.join('\n'))}</code></pre>`);
      continue;
    }
    const h = /^(#{1,4})\s+(.*)$/.exec(l);
    if (h) { const n = Math.min(6, h[1].length + shift); out.push(`<h${n}>${inline(h[2])}</h${n}>`); i++; continue; }
    if (/^\s*(-{3,}|\*{3,})\s*$/.test(l)) { out.push('<hr>'); i++; continue; }
    if (/^>\s?/.test(l)) {
      const q: string[] = [];
      while (i < lines.length && /^>\s?/.test(lines[i])) q.push(lines[i++].replace(/^>\s?/, ''));
      out.push(`<blockquote>${renderMarkdown(q.join('\n'), o)}</blockquote>`);
      continue;
    }
    if (/^\s*\|.*\|\s*$/.test(l) && /^\s*\|?\s*:?-+:?\s*(\||$)/.test(lines[i + 1] || '')) {
      const cells = (r: string) => r.trim().replace(/^\||\|$/g, '').split('|').map((c) => inline(c.trim()));
      const head = cells(l); i += 2;
      const rows: string[][] = [];
      while (i < lines.length && /^\s*\|.*\|\s*$/.test(lines[i])) rows.push(cells(lines[i++]));
      out.push(`<table><thead><tr>${head.map((c) => `<th>${c}</th>`).join('')}</tr></thead><tbody>${rows.map((r) => `<tr>${r.map((c) => `<td>${c}</td>`).join('')}</tr>`).join('')}</tbody></table>`);
      continue;
    }
    if (/^\s*([-*]|\d+\.)\s+/.test(l)) {
      const ordered = /^\s*\d+\./.test(l);
      const items: string[] = [];
      while (i < lines.length && /^\s*([-*]|\d+\.)\s+/.test(lines[i])) items.push(`<li>${inline(lines[i].replace(/^\s*([-*]|\d+\.)\s+/, ''))}</li>`), i++;
      out.push(`<${ordered ? 'ol' : 'ul'}>${items.join('')}</${ordered ? 'ol' : 'ul'}>`);
      continue;
    }
    if (!l.trim()) { i++; continue; }
    // The first line always belongs to the paragraph (a lone `| a |` row is text), so the loop always advances.
    const para: string[] = [lines[i++]];
    while (i < lines.length && lines[i].trim() && !lines[i].startsWith('```') && !/^(#{1,4})\s/.test(lines[i]) && !/^>/.test(lines[i]) && !/^\s*(-{3,}|\*{3,})\s*$/.test(lines[i]) && !/^\s*([-*]|\d+\.)\s+/.test(lines[i])) para.push(lines[i++]);
    out.push(`<p>${para.map(inline).join('<br>')}</p>`);
  }
  return out.join('');
}
