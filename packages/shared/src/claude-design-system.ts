// Migrates a Claude design system: the .zip its page downloads becomes one DESIGN.md. Only text is read and nothing
// is written to disk or run, so paths and previews in the archive can't do anything. Format: Claude's Design System
// type (README.md, tokens.json, design-system.json, components/<Name>/README.md).

/** How much of a workspace system's DESIGN.md agents are handed (`artifact_guide`). */
export const DESIGN_MD_LIMIT = 32000;
const MAX_FILE = 2_000_000;
const TEXT = /\.(md|json)$/i;

async function inflate(data: Uint8Array): Promise<Uint8Array> {
  const reader = new Blob([data as BlobPart]).stream().pipeThrough(new DecompressionStream('deflate-raw')).getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.length;
    if (size > MAX_FILE) { await reader.cancel(); throw new Error('This .zip has a file too large for a design system'); }
    chunks.push(value);
  }
  const out = new Uint8Array(size);
  let at = 0;
  for (const c of chunks) { out.set(c, at); at += c.length; }
  return out;
}

/** The text files in a .zip by path, plus the paths of everything else. */
export async function readZip(bytes: Uint8Array): Promise<{ text: Map<string, string>; other: string[] }> {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let end = bytes.length - 22;
  const stop = Math.max(0, end - 65535);
  while (end >= stop && view.getUint32(end, true) !== 0x06054b50) end--;
  if (end < stop) throw new Error('This file is not a .zip');
  const count = view.getUint16(end + 10, true);
  let at = view.getUint32(end + 16, true);
  if (count === 0xffff || at === 0xffffffff) throw new Error('This .zip is too large to be a design system');
  const names = new TextDecoder();
  const text = new Map<string, string>();
  const other: string[] = [];
  for (let i = 0; i < count; i++) {
    if (at + 46 > bytes.length || view.getUint32(at, true) !== 0x02014b50) throw new Error('This .zip is damaged');
    const flags = view.getUint16(at + 8, true), method = view.getUint16(at + 10, true);
    const packed = view.getUint32(at + 20, true), nameLen = view.getUint16(at + 28, true);
    const local = view.getUint32(at + 42, true);
    const name = names.decode(bytes.subarray(at + 46, at + 46 + nameLen));
    at += 46 + nameLen + view.getUint16(at + 30, true) + view.getUint16(at + 32, true);
    if (name.endsWith('/')) continue;
    if (!TEXT.test(name)) { other.push(name); continue; }
    if (flags & 1) throw new Error('This .zip is password protected');
    if (method !== 0 && method !== 8) throw new Error('This .zip uses a compression method WorkspaceAI can’t read');
    if (local + 30 > bytes.length) throw new Error('This .zip is damaged');
    const start = local + 30 + view.getUint16(local + 26, true) + view.getUint16(local + 28, true);
    if (start + packed > bytes.length) throw new Error('This .zip is damaged');
    if (method === 0 && packed > MAX_FILE) throw new Error('This .zip has a file too large for a design system');
    const raw = bytes.subarray(start, start + packed);
    text.set(name, new TextDecoder().decode(method === 8 ? await inflate(raw) : raw));
  }
  return { text, other };
}

interface Token { name: string; value: unknown; usage?: string }
const cell = (v: unknown) => String(v ?? '').replace(/\s*\n\s*/g, ' ').replace(/\|/g, '\\|');
const row = (cells: unknown[]) => `| ${cells.map(cell).join(' | ')} |`;
const table = (head: string[], rows: unknown[][]) => [row(head), row(head.map(() => '---')), ...rows.map(row)].join('\n');
const list = (v: unknown): Token[] => (Array.isArray((v as { tokens?: unknown })?.tokens) ? (v as { tokens: Token[] }).tokens : []);
const title = (s: string) => s.replace(/([a-z])([A-Z])/g, '$1 $2').replace(/^./, (c) => c.toUpperCase());

function colours(color: unknown): string {
  const tokens = list(color);
  if (!tokens.length) return '';
  const given = (color as { themes?: { id: string; name?: string }[] }).themes;
  const themes = given?.length ? given : [{ id: 'value', name: 'Value' }];
  const byName = new Map(tokens.map((t) => [t.name, t]));
  const value = (t: Token | undefined, theme: string, depth = 0): string => {
    const v = t?.value;
    const raw = typeof v === 'string' ? v : (v as Record<string, string> | undefined)?.[theme] ?? (v as Record<string, string> | undefined)?.[themes[0].id];
    const alias = /^\{(.+)\}$/.exec(String(raw ?? ''));
    return alias && depth < 16 ? value(byName.get(alias[1]), theme, depth + 1) : String(raw ?? '');
  };
  return `## Colour tokens\n\n${table(['Token', ...themes.map((t) => t.name || t.id), 'Use'], tokens.map((t) => [t.name, ...themes.map((m) => value(t, m.id)), t.usage]))}`;
}

function typography(type: any): string {
  if (!type) return '';
  const families = Object.entries(type.families || {});
  const styles = (type.groups || []).flatMap((g: any) => (g.styles || []).map((s: any) => [s.name, s.family || g.family, s.fontSize, s.lineHeight, s.fontWeight, s.letterSpacing, s.usage]));
  const parts = [
    families.length ? table(['Family', 'Stack'], families) : '',
    styles.length ? table(['Style', 'Family', 'Size', 'Line height', 'Weight', 'Tracking', 'Use'], styles) : '',
  ].filter(Boolean);
  return parts.length ? `## Type\n\n${parts.join('\n\n')}` : '';
}

const perTheme = (v: unknown) => (v && typeof v === 'object' ? Object.entries(v).map(([k, x]) => `${k}: ${x}`).join('; ') : v);

/** Drop a document's leading H1, and move its other headings down `by` levels. */
function section(md: string, by: number): string {
  const body = md.replace(/^\s*#\s+.*\n?/, '');
  let fenced = false;
  return body.split('\n').map((l) => {
    if (/^\s*(```|~~~)/.test(l)) fenced = !fenced;
    return !fenced && /^#{1,5}\s/.test(l) ? '#'.repeat(by) + l : l;
  }).join('\n').trim();
}

export interface MigratedSystem { title: string; markdown: string; notCarried: string[] }

/** One Claude design system as a DESIGN.md: tokens first, since agents read only the start of a long one. */
export function claudeSystemToDesignMd(zip: { text: Map<string, string>; other: string[] }, fallbackName: string): MigratedSystem {
  const marker = /(^|\/)(design-system\.json|tokens\.json|README\.md)$/i;
  const root = [...zip.text.keys()].filter((p) => marker.test(p)).map((p) => p.slice(0, p.lastIndexOf('/') + 1)).sort((a, b) => a.length - b.length)[0];
  if (root === undefined) throw new Error('This .zip is not a Claude design system: it has no README.md or tokens.json');
  const files = new Map<string, string>();
  for (const [p, t] of zip.text) if (p.startsWith(root)) files.set(p.slice(root.length), t);
  const json = (name: string): any => {
    const raw = files.get(name);
    if (raw === undefined) return undefined;
    try { return JSON.parse(raw); } catch { throw new Error(`This design system’s ${name} is not valid JSON`); }
  };
  const tokens = json('tokens.json');
  const index = json('design-system.json');
  const readme = files.get('README.md') ?? files.get('readme.md') ?? '';
  if (!tokens && !readme.trim()) throw new Error('This .zip is not a Claude design system: it has no README.md or tokens.json');
  const name = String(index?.title || tokens?.name || /^#\s+(.+)$/m.exec(readme)?.[1] || fallbackName).replace(/\s+/g, ' ').trim().slice(0, 120);

  const summary = section(readme, 0).split(/\n\s*\n/)[0]?.trim() || '';
  const plain = summary && !/^[#|<>\-*`]/.test(summary);
  const families = Object.keys(tokens || {}).filter((k) => !['color', 'type'].includes(k) && list(tokens[k]).length);
  const sections = [...files.keys()].filter((p) => /\.md$/i.test(p) && !/^readme\.md$/i.test(p) && !/^(components|archived|api|assets)\//.test(p)).sort();
  const components = [...files.keys()].flatMap((p) => { const m = /^components\/([^/]+)\/README\.md$/.exec(p); return m ? [[m[1], files.get(p)!] as const] : []; }).sort((a, b) => a[0].localeCompare(b[0]));

  const left = zip.other.filter((p) => p.startsWith(root)).map((p) => p.slice(root.length)).filter((p) => !/^(api|archived)\//.test(p) && !/^(tokens\.css|manifest\.json|SKILL\.md|index\.html)$/.test(p));
  const count = (re: RegExp) => left.filter((p) => re.test(p)).length;
  const notCarried = ([[count(/^components\/[^/]+\/preview\.html$/), 'component preview'], [count(/^fonts\//), 'font file'], [count(/^assets\//), 'logo or image file'], [count(/^components\/(bundle\.|lib\/|index\.d\.ts)/), 'component code file']] as const)
    .filter(([n]) => n > 0).map(([n, what]) => `${n} ${what}${n === 1 ? '' : 's'}`);

  const book = section(readme, 1).replace(plain ? summary : '\0', '').trim();
  const markdown = [
    `# ${name}`,
    plain ? `> ${summary.replace(/\s*\n\s*/g, ' ')}` : '',
    colours(tokens?.color),
    typography(tokens?.type),
    ...families.map((k) => `## ${title(k)}\n\n${table(['Token', 'Value', 'Use'], list(tokens[k]).map((t) => [t.name, perTheme(t.value), t.usage]))}`),
    book ? `## Brand book\n\n${book}` : '',
    ...sections.map((p) => section(files.get(p)!, 1)),
    components.length ? `## Components\n\n${components.map(([c, md]) => `### ${c}\n\n${section(md, 2)}`).join('\n\n')}` : '',
    notCarried.length ? `## Not carried over\n\nMigrated from a Claude design system, as text. Still in Claude: ${notCarried.join(', ')}. Don’t approximate a logo or a licensed font: set the name in plain type and use the fallbacks in the font stacks above.` : '',
  ].filter(Boolean).join('\n\n') + '\n';
  return { title: name, markdown, notCarried };
}
