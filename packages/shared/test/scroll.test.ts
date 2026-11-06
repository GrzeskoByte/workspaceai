import { describe, it, expect } from 'vitest';
import { followsBottom } from '../src/util.ts';

describe('followsBottom', () => {
  it('lets go on any move up, even a small one near the bottom', () => {
    expect(followsBottom(true, 1000, 960, 40)).toBe(false);
  });
  it('stays let go while the reader moves down far from the bottom', () => {
    expect(followsBottom(false, 100, 500, 2000)).toBe(false);
  });
  it('follows again once the reader comes back near the bottom', () => {
    expect(followsBottom(false, 900, 960, 40)).toBe(true);
  });
  it('keeps following when new text grows the gap and the view jumps down', () => {
    expect(followsBottom(true, 1000, 1000, 300)).toBe(true);
  });
});
