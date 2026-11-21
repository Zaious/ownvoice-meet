import { describe, expect, it } from 'vitest';
import { judgeLevel } from '../src/settings/level.ts';

describe('microphone check', () => {
  it('tells silence, quiet, fine and too loud apart', () => {
    expect(judgeLevel(0)).toContain('幾乎沒有聲音');
    expect(judgeLevel(0.01)).toContain('偏小');
    expect(judgeLevel(0.2)).toBe('音量正常，可以開始');
    expect(judgeLevel(0.9)).toContain('偏大');
  });
});
