// Markdown in chat replies and artifacts, rendered by the shared safe renderer.
import { useMemo } from 'react';
import { renderMarkdown } from '../../../../packages/shared/src/markdown.ts';

export { renderMarkdown };

export function Markdown({ text }: { text: string }) {
  const html = useMemo(() => renderMarkdown(text), [text]);
  return <div className="md" dangerouslySetInnerHTML={{ __html: html }} />;
}
