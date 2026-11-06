import { describe, it, expect } from 'vitest';
import { chatCommands, commandAction, findHarnessCommand, insertMention, mentionAt, parseSlash } from '../src/commands.ts';

describe('@ references', () => {
  it('finds the one being typed at the caret', () => {
    expect(mentionAt('@', 1)).toEqual({ start: 0, query: '' });
    expect(mentionAt('look at @src/ma', 15)).toEqual({ start: 8, query: 'src/ma' });
    expect(mentionAt('look at @src now', 16)).toBeNull();
    expect(mentionAt('look at @src now', 12)).toEqual({ start: 8, query: 'src' });
    expect(mentionAt('mail me@example.com', 19)).toBeNull();
  });
  it('inserts a reference and says where the caret goes', () => {
    expect(insertMention('see @ma please', 4, 7, 'src/main.ts')).toEqual({ text: 'see @src/main.ts  please', caret: 17 });
    expect(insertMention('@sr', 0, 3, 'src/', true)).toEqual({ text: '@src/', caret: 5 });
    expect(insertMention('@my', 0, 3, 'my file.md').text).toBe('@"my file.md" ');
  });
});

describe('slash commands', () => {
  it('maps each harness command to the agent’s own equivalent', () => {
    const status = findHarnessCommand('status')!;
    expect(commandAction(status, 'claude')).toEqual({ kind: 'ui', action: 'status', then: '/context' });
    expect(commandAction(status, 'grok')).toEqual({ kind: 'ui', action: 'status', then: '/session-info' });
    expect(commandAction(status, 'opencode')).toEqual({ kind: 'ui', action: 'status' });
    const review = findHarnessCommand('review')!;
    expect(commandAction(review, 'claude')).toEqual({ kind: 'native', command: '/code-review' });
    expect(commandAction(review, 'grok')).toEqual({ kind: 'native', command: '/review' });
    expect(commandAction(review, 'opencode')).toEqual({ kind: 'oc-command', name: 'review' });
    expect(commandAction(findHarnessCommand('compact')!, 'opencode')).toEqual({ kind: 'oc-compact' });
    // No equivalent: not offered.
    expect(commandAction(findHarnessCommand('init')!, 'grok')).toBeNull();
    expect(commandAction(findHarnessCommand('loop')!, 'opencode')).toBeNull();
    // Aliases.
    expect(findHarnessCommand('clear')?.name).toBe('new');
    expect(findHarnessCommand('cost')?.name).toBe('usage');
  });

  it('lists harness commands first, then the agent’s own ones it does not already cover', () => {
    const list = chatCommands('claude', 'Claude Code', [{ name: 'compact', description: 'theirs' }, { name: 'model' }, { name: 'clear' }, { name: 'simplify', description: 'Clean up code', hint: '[target]' }]);
    expect(list.filter((c) => c.name === 'compact')).toEqual([expect.objectContaining({ source: 'harness', runsAs: 'Claude Code /compact' })]);
    expect(list.some((c) => c.source === 'agent' && (c.name === 'model' || c.name === 'clear'))).toBe(false);
    expect(list.at(-1)).toEqual({ name: 'simplify', description: 'Clean up code', hint: '[target]', source: 'agent' });
    expect(list.find((c) => c.name === 'status')?.runsAs).toBe('status card + Claude Code /context');
    expect(chatCommands('grok', 'Grok', []).some((c) => c.name === 'init')).toBe(false);
  });

  it('parses a command and its arguments', () => {
    expect(parseSlash('/effort high')).toEqual({ name: 'effort', args: 'high' });
    expect(parseSlash('  /data:analyze  why did it drop?\nmore ')).toEqual({ name: 'data:analyze', args: 'why did it drop?\nmore' });
    expect(parseSlash('not a /command')).toBeNull();
    expect(parseSlash('/')).toBeNull();
    expect(parseSlash('/usr/bin is a path')).toBeNull();
  });
});
