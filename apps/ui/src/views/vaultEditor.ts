// Live Preview over Markdown source: formatting shows in place and syntax appears on the line being edited. The document
// is always the note's own text, so nothing is ever rewritten except what the user changes.
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { syntaxTree } from '@codemirror/language';
import { Annotation, type ChangeSpec, EditorSelection, EditorState, type Range } from '@codemirror/state';
import { Decoration, type DecorationSet, drawSelection, EditorView, keymap, ViewPlugin, type ViewUpdate, WidgetType } from '@codemirror/view';

const fromDisk = Annotation.define<boolean>();
const hidden = Decoration.replace({});
const WIKILINK = /(?<!!)\[\[([^[\]\n|]+)(\|[^[\]\n]+)?\]\]/g;
const MARK_CLASS: Record<string, string> = { Emphasis: 'cm-md-em', StrongEmphasis: 'cm-md-strong', Strikethrough: 'cm-md-strike', InlineCode: 'cm-md-code' };

/** CRLF only when every line uses it. Any other mix stays as it is: '\n' splits it and a stray \r stays in its line. */
const lineSep = (text: string) => (/\r\n/.test(text) && !/(^|[^\r])\n/.test(text) ? '\r\n' : '\n');

/** Where a leading YAML block ends. Without this the parser reads it as a rule and a heading. */
function frontmatterEnd(state: EditorState): number {
  if (state.doc.line(1).text.trimEnd() !== '---') return 0;
  for (let n = 2; n <= state.doc.lines; n++) if (/^(---|\.\.\.)\s*$/.test(state.doc.line(n).text)) return state.doc.line(n).to;
  return 0;
}

class CheckboxWidget extends WidgetType {
  constructor(readonly checked: boolean) { super(); }
  eq(o: CheckboxWidget) { return o.checked === this.checked; }
  toDOM(view: EditorView) {
    const box = document.createElement('input');
    box.type = 'checkbox';
    box.className = 'cm-md-check';
    box.checked = this.checked;
    box.disabled = view.state.readOnly;
    box.setAttribute('aria-label', this.checked ? 'Done task' : 'Open task');
    box.onmousedown = (e) => {
      e.preventDefault();
      const at = view.posAtDOM(box) + 1; // the character between the brackets; nothing else changes
      view.dispatch({ changes: { from: at, to: at + 1, insert: this.checked ? ' ' : 'x' } });
    };
    return box;
  }
  ignoreEvent() { return true; }
}

function decorate(view: EditorView): DecorationSet {
  const { state } = view;
  const out: Range<Decoration>[] = [];
  const fm = frontmatterEnd(state);
  const activeLines = new Set<number>();
  for (const r of state.selection.ranges) for (let n = state.doc.lineAt(r.from).number; n <= state.doc.lineAt(r.to).number; n++) activeLines.add(n);
  const active = (pos: number) => activeLines.has(state.doc.lineAt(pos).number);
  const hide = (from: number, to: number) => { if (from < to && !active(from)) out.push(hidden.range(from, to)); };
  const lineClass = (from: number, to: number, cls: string) => {
    for (let n = state.doc.lineAt(from).number; n <= state.doc.lineAt(to).number; n++) out.push(Decoration.line({ class: cls }).range(state.doc.line(n).from));
  };
  if (fm) lineClass(0, fm, 'cm-md-frontmatter');
  for (const { from, to } of view.visibleRanges) {
    syntaxTree(state).iterate({ from, to, enter: (n) => {
      if (n.to <= fm && n.name !== 'Document') return false; // the root starts at 0 too; it has to be entered
      const heading = /^ATXHeading(\d)$/.exec(n.name);
      if (heading) lineClass(n.from, n.from, 'cm-md-h' + heading[1]);
      else if (n.name === 'HeaderMark') {
        if (n.node.parent?.name.startsWith('ATXHeading')) hide(n.from, Math.min(n.to + (state.sliceDoc(n.to, n.to + 1) === ' ' ? 1 : 0), state.doc.lineAt(n.from).to));
      } else if (n.name === 'EmphasisMark' || n.name === 'StrikethroughMark') hide(n.from, n.to);
      else if (n.name === 'CodeMark' && n.node.parent?.name === 'InlineCode') hide(n.from, n.to);
      else if (n.name === 'FencedCode') { lineClass(n.from, n.to, 'cm-md-fence'); return false; }
      else if (n.name === 'CodeBlock') return false;
      else if (n.name === 'Blockquote') lineClass(n.from, n.to, 'cm-md-quote');
      else if (n.name === 'TaskMarker') {
        if (!active(n.from)) out.push(Decoration.replace({ widget: new CheckboxWidget(state.sliceDoc(n.from + 1, n.to - 1).toLowerCase() === 'x') }).range(n.from, n.to));
      } else if (n.name === 'Link') {
        out.push(Decoration.mark({ class: 'cm-md-link' }).range(n.from, n.to));
        const marks = n.node.getChildren('LinkMark');
        if (marks.length >= 2 && !active(n.from)) { hide(marks[0].from, marks[0].to); hide(marks[1].from, n.to); }
        return false;
      } else if (MARK_CLASS[n.name] && n.from < n.to) out.push(Decoration.mark({ class: MARK_CLASS[n.name] }).range(n.from, n.to));
      return undefined;
    } });
    const text = state.doc.sliceString(from, to, '\n'); // one character per line break, like editor positions, even in CRLF notes
    for (const m of text.matchAll(WIKILINK)) {
      const start = from + m.index, end = start + m[0].length;
      if (start < fm || inCode(state, start)) continue;
      out.push(Decoration.mark({ class: 'cm-md-wikilink' }).range(start, end));
      hide(start, start + 2 + (m[2] ? m[1].length + 1 : 0));
      hide(end - 2, end);
    }
  }
  return Decoration.set(out, true);
}

function inCode(state: EditorState, pos: number): boolean {
  for (let n: ReturnType<ReturnType<typeof syntaxTree>['resolveInner']> | null = syntaxTree(state).resolveInner(pos, 1); n; n = n.parent) if (/^(InlineCode|FencedCode|CodeBlock)$/.test(n.name)) return true;
  return false;
}

const livePreview = ViewPlugin.fromClass(class {
  decorations: DecorationSet;
  constructor(view: EditorView) { this.decorations = decorate(view); }
  // Parsing finishes in the background on big notes: without the tree check, late regions stay unformatted.
  update(u: ViewUpdate) {
    if (u.docChanged || u.viewportChanged || u.selectionSet || syntaxTree(u.startState) !== syntaxTree(u.state)) this.decorations = decorate(u.view);
  }
}, { decorations: (v) => v.decorations });

/** The link under a click on formatted text. On the line being edited a click only places the cursor. */
function linkAt(view: EditorView, pos: number): { wiki: string } | { url: string } | null {
  const { state } = view;
  const line = state.doc.lineAt(pos);
  if (state.selection.ranges.some((r) => r.from <= line.to && r.to >= line.from)) return null;
  for (const m of line.text.matchAll(WIKILINK)) {
    const start = line.from + m.index;
    if (pos >= start && pos <= start + m[0].length && !inCode(state, start)) return { wiki: m[1].trim() };
  }
  for (let n: ReturnType<ReturnType<typeof syntaxTree>['resolveInner']> | null = syntaxTree(state).resolveInner(pos, 1); n; n = n.parent) {
    if (n.name !== 'Link') continue;
    const url = n.getChild('URL');
    return url ? { url: state.sliceDoc(url.from, url.to) } : null;
  }
  return null;
}

export type Format = 'bold' | 'italic' | 'heading' | 'list' | 'task' | 'link';

export interface NoteEditor {
  /** False when the editor couldn't hold the note exactly, so it opened read-only. */
  faithful: boolean;
  getText(): string;
  /** Applies a change from disk as one small edit, so the cursor stays where it was. Doesn't count as typing. */
  setText(text: string): void;
  format(kind: Format): void;
  focus(): void;
  destroy(): void;
}

export function createNoteEditor(o: { parent: HTMLElement; text: string; onChange: () => void; onWiki: (target: string) => void; readOnly?: boolean }): NoteEditor {
  const makeState = (text: string, readOnly: boolean) => {
    const sep = lineSep(text);
    return EditorState.create({ doc: text, extensions: [
      EditorState.lineSeparator.of(sep),
      EditorState.readOnly.of(readOnly),
      EditorView.editable.of(!readOnly),
      // The separator above is exact, so pasted text has to be converted to it first.
      EditorView.clipboardInputFilter.of((t) => t.replace(/\r\n?|\n/g, sep)),
      EditorView.contentAttributes.of({ 'aria-label': 'Note text' }),
      EditorView.lineWrapping,
      history(),
      drawSelection(),
      keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab, { key: 'Mod-b', run: () => { format('bold'); return true; } }, { key: 'Mod-i', run: () => { format('italic'); return true; } }]),
      markdown({ base: markdownLanguage }),
      livePreview,
      EditorView.updateListener.of((u) => { if (u.docChanged && !u.transactions.some((t) => t.annotation(fromDisk))) o.onChange(); }),
      EditorView.domEventHandlers({
        // Before the editor moves the cursor: that would turn the line back into source under the click.
        mousedown: (e, v) => {
          if (e.button !== 0) return false;
          const pos = v.posAtCoords({ x: e.clientX, y: e.clientY });
          const link = pos === null ? null : linkAt(v, pos);
          if (!link) return false;
          e.preventDefault();
          if ('wiki' in link) o.onWiki(link.wiki);
          else if (/^https?:\/\//i.test(link.url)) window.open(link.url, '_blank', 'noopener');
          return true;
        },
      }),
    ] });
  };
  const initial = makeState(o.text, !!o.readOnly);
  const faithful = initial.sliceDoc() === o.text;
  const view = new EditorView({ state: faithful ? initial : makeState(o.text, true), parent: o.parent });
  const wrap = (mark: string) => view.dispatch(view.state.changeByRange((r) => {
    const text = view.state.sliceDoc(r.from, r.to);
    const insert = mark + text + mark;
    return { changes: { from: r.from, to: r.to, insert }, range: EditorSelection.range(r.from + mark.length, r.to + mark.length) };
  }));
  const prefix = (mark: string) => {
    const lines = new Set<number>();
    for (const r of view.state.selection.ranges) for (let n = view.state.doc.lineAt(r.from).number; n <= view.state.doc.lineAt(r.to).number; n++) lines.add(n);
    view.dispatch({ changes: [...lines].map((n): ChangeSpec => {
      const line = view.state.doc.line(n);
      return line.text.startsWith(mark) ? { from: line.from, to: line.from + mark.length } : { from: line.from, insert: mark };
    }) });
  };
  const format = (kind: Format) => {
    if (view.state.readOnly) return;
    if (kind === 'bold') wrap('**');
    else if (kind === 'italic') wrap('*');
    else if (kind === 'heading') prefix('# ');
    else if (kind === 'list') prefix('- ');
    else if (kind === 'task') prefix('- [ ] ');
    else view.dispatch(view.state.changeByRange((r) => {
      const text = view.state.sliceDoc(r.from, r.to);
      return { changes: { from: r.from, to: r.to, insert: `[${text}](https://)` }, range: EditorSelection.cursor(r.from + text.length + 11) };
    }));
    view.focus();
  };
  return {
    faithful,
    getText: () => view.state.sliceDoc(),
    setText: (text) => {
      const cur = view.state.sliceDoc();
      if (cur === text) return;
      if (lineSep(text) !== view.state.lineBreak) { view.setState(makeState(text, view.state.readOnly)); return; }
      let from = 0;
      while (from < cur.length && from < text.length && cur[from] === text[from]) from++;
      let a = cur.length, b = text.length;
      while (a > from && b > from && cur[a - 1] === text[b - 1]) { a--; b--; }
      // Never split a CRLF pair: the editor treats it as one character.
      if (from > 0 && cur[from - 1] === '\r' && (cur[from] === '\n' || text[from] === '\n')) from--;
      if (cur[a] === '\n' && cur[a - 1] === '\r' && a > from) { a++; b++; }
      // Positions in the editor count a line break as one character, whichever the file uses.
      const pos = (i: number) => view.state.lineBreak.length === 1 ? i : i - (cur.slice(0, i).split('\r\n').length - 1);
      view.dispatch({ changes: { from: pos(from), to: pos(a), insert: text.slice(from, b) }, annotations: fromDisk.of(true) });
    },
    format,
    focus: () => view.focus(),
    destroy: () => view.destroy(),
  };
}
