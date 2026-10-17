import { describe, expect, it } from 'vitest';
import { FONT_STEPS, stepFont } from '../src/shared/view.ts';

describe('stepFont', () => {
  it('moves one step at a time', () => {
    expect(stepFont(15, 1)).toBe(16);
    expect(stepFont(16, 1)).toBe(18);
    expect(stepFont(18, -1)).toBe(16);
  });
  it('snaps a size between steps to the next step that way', () => {
    expect(stepFont(17, 1)).toBe(18);
    expect(stepFont(17, -1)).toBe(16);
  });
  it('stops at the ends (the Rust side clamps to the same range)', () => {
    expect(stepFont(FONT_STEPS[0]!, -1)).toBe(10);
    expect(stepFont(48, 1)).toBe(48);
  });
});
