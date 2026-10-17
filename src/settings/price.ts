// Price lines for the switcher, in one format for every option, and an hourly figure so speech and
// language models can be compared side by side.

import type { CatalogModel, Price } from '../shared/types.ts';

/** The translation workload behind the hourly estimate: 600 sentences, 300 tokens in, 50 out. */
export const HOUR_INPUT_TOKENS = 600 * 300;
export const HOUR_OUTPUT_TOKENS = 600 * 50;

const usd = (n: number): string => (n === 0 ? '$0' : n < 0.01 ? `$${n.toFixed(4)}` : `$${n.toFixed(2)}`);
/** A unit price as listed: keep its digits ($0.017 must not become $0.02). */
const exact = (n: number): string => `$${Number(n.toPrecision(4))}`;

export function priceLine(p: Price): string {
  switch (p.unit) {
    case 'per_minute':
      return `${exact(p.usd)} / 分鐘音訊`;
    case 'per_m_tok':
      return `輸入 $${p.input} · 快取 $${p.cached} · 輸出 $${p.output} / 百萬 token`;
    case 'free':
      return '不計費';
  }
}

export function hourly(m: Pick<CatalogModel, 'price'>): number {
  const p = m.price;
  switch (p.unit) {
    case 'per_minute':
      return p.usd * 60;
    case 'per_m_tok':
      return (HOUR_INPUT_TOKENS * p.input + HOUR_OUTPUT_TOKENS * p.output) / 1_000_000;
    case 'free':
      return 0;
  }
}

export function hourlyLine(m: Pick<CatalogModel, 'price'>): string {
  return m.price.unit === 'free' ? '每小時 $0' : `每小時約 ${usd(hourly(m))}`;
}

export function money(n: number): string {
  return usd(n);
}
