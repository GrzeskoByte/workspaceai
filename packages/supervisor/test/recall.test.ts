// Memory is used on every message: matches from the graph and approved facts go to the agent with the message,
// and every agent is told to check memory itself. lean-ctx's config is read the way its CLI writes it.
import { describe, it, expect } from 'vitest';
import { queryTerms, rankNodes } from '../src/graphify.ts';
import { stripRecall } from '../src/recall.ts';
import { parseToml } from '../src/leanctx.ts';
import { isAuthFailure } from '../src/signin.ts';
import { memoryRule } from '../../shared/src/defaults.ts';

const graph: any = {
  source: 'graphify', nodes: [
    { id: 'src/auth/session.ts', label: 'session.ts', kind: 'file', community: 'c1', size: 10 },
    { id: 'refreshToken', label: 'refreshToken', kind: 'symbol', community: 'c1' },
    { id: 'docs/billing.md', label: 'billing.md', kind: 'file', community: 'c2' },
  ], edges: [{ from: 'src/auth/session.ts', to: 'refreshToken', kind: 'defines' }],
};

describe('memory recall', () => {
  it('looks up the words that matter', () => {
    expect(queryTerms('Why does the refreshToken in session.ts expire so fast?')).toEqual(['refreshtoken', 'session.ts', 'expire', 'fast']);
    expect(rankNodes(graph, queryTerms('fix refreshToken in session.ts'), 5).map((n: any) => n.id).sort()).toEqual(['refreshToken', 'src/auth/session.ts']);
    expect(rankNodes(graph, [], 5)).toEqual([]);
  });

  it('keeps the recall block out of the transcript', () => {
    expect(stripRecall('fix it\n\n<harness-memory>\nFacts:\n- x\n</harness-memory>')).toBe('fix it');
    expect(stripRecall('plain')).toBe('plain');
  });

  it('tells agents to use the tools their memory has', () => {
    expect(memoryRule('both')).toContain('graphify_query');
    expect(memoryRule('both')).toContain('memory_search');
    expect(memoryRule('facts')).not.toContain('graphify_query');
    expect(memoryRule('graph')).not.toContain('memory_search');
  });
});

describe('lean-ctx config', () => {
  it('reads root keys and sections', () => {
    expect(parseToml('compression_level = "lite"\nbm25_max_cache_mb = 128\n\n[archive]\nenabled = true # on\nextra = ["a", "b"]\n')).toEqual({
      compression_level: 'lite', bm25_max_cache_mb: 128, 'archive.enabled': true, 'archive.extra': ['a', 'b'],
    });
  });
});

describe('signed out', () => {
  it('knows an expired login from other failures', () => {
    expect(isAuthFailure('Internal error: Failed to authenticate: OAuth session expired and could not be refreshed')).toBe(true);
    expect(isAuthFailure('Invalid API key · Please run /login')).toBe(true);
    expect(isAuthFailure('Authentication required')).toBe(true);
    expect(isAuthFailure('Request timed out')).toBe(false);
    expect(isAuthFailure(undefined)).toBe(false);
  });
});
