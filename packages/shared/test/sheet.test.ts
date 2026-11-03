// Action sheets read an agent's own /usage and /context output into meters and breakdowns (real Claude Code text).
import { describe, it, expect } from 'vitest';
import { readSheet } from '../../../apps/ui/src/views/ActionSheet.tsx';

const USAGE = `You are currently using your subscription to power your Claude Code usage

Current session: 67% used · resets Oct 30, 9:20pm (Europe/Madrid)
Current week (all models): 19% used · resets Nov 6, 10am (Europe/Madrid)

What's contributing to your usage?
Approximate · sessions on this machine only · shares overlap

Last 24h · 870 requests · 2 sessions
  90%  Start fresh when you switch tasks · run /clear
  60%  Stop sessions you no longer need · some ran 8 hours or more
  Top skills: /loop 22%`;

const CONTEXT = `## Context Usage

**Model:** claude-opus-5-5  
**Tokens:** 24.9k / 1m (2%)

### Estimated usage by category

| Category | Tokens | Percentage |
|----------|--------|------------|
| System prompt | 2.8k | 0.3% |
| Messages | 10 | 0.0% |
| Free space | 942.1k | 94.2% |

### MCP Tools

| Tool | Server | Tokens |
|------|--------|--------|
| mcp__a__b | a | 154 |`;

describe('action sheet', () => {
  it('reads plan usage into meters and ranked tips', () => {
    const r = readSheet(USAGE);
    expect(r.intro).toEqual(['You are currently using your subscription to power your Claude Code usage']);
    expect(r.meters).toEqual([
      { label: 'Current session', pct: 67, sub: 'resets Oct 30, 9:20pm (Europe/Madrid)' },
      { label: 'Current week (all models)', pct: 19, sub: 'resets Nov 6, 10am (Europe/Madrid)' },
    ]);
    expect(r.ranked).toEqual([{ heading: 'Last 24h · 870 requests · 2 sessions', rows: [
      { pct: 90, text: 'Start fresh when you switch tasks · run /clear' },
      { pct: 60, text: 'Stop sessions you no longer need · some ran 8 hours or more' },
      { pct: -1, text: 'Top skills: /loop 22%' },
    ] }]);
  });

  it('reads the context window and its categories', () => {
    const r = readSheet(CONTEXT);
    expect(r.model).toBe('claude-opus-5-5');
    expect(r.meters).toEqual([{ label: 'Context window', pct: 2, sub: '24.9k of 1m tokens' }]);
    expect(r.categories.map((c) => [c.name, c.pct])).toEqual([['System prompt', 0.3], ['Messages', 0], ['Free space', 94.2]]);
    expect(r.rest).toContain('### MCP Tools'); // the rest stays available as written
    expect(r.rest).not.toContain('Context Usage');
  });
});
