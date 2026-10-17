import { describe, expect, it } from 'vitest';
import { hourly, hourlyLine, priceLine } from '../src/settings/price.ts';

describe('switcher prices', () => {
  it('speech is priced per minute, compared per hour', () => {
    const gemini = { price: { unit: 'per_minute', usd: 0.009 } } as const;
    expect(priceLine(gemini.price)).toBe('$0.009 / 分鐘音訊');
    // a listed price keeps its digits
    expect(priceLine({ unit: 'per_minute', usd: 0.017 })).toBe('$0.017 / 分鐘音訊');
    expect(hourly(gemini)).toBeCloseTo(0.54, 6);
    expect(hourlyLine(gemini)).toBe('每小時約 $0.54');
  });

  it('language models use the 600-sentence hour (matches the Rust catalog test)', () => {
    const haiku = { price: { unit: 'per_m_tok', input: 0.1, cached: 0.01, output: 0.5 } } as const;
    expect(hourly(haiku)).toBeCloseTo(0.033, 6);
    expect(priceLine(haiku.price)).toBe('輸入 $0.1 · 快取 $0.01 · 輸出 $0.5 / 百萬 token');
  });

  it('local and on-device options cost nothing', () => {
    expect(hourlyLine({ price: { unit: 'free' } })).toBe('每小時 $0');
  });
});
