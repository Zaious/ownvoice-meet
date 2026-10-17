// The settings window: the speech and translation switchers (prices and descriptions from the Rust
// catalog), local server fields, API keys (write-only; only whether a key is set comes back), the
// glossary, and estimated usage.

import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import type { CatalogModel, Settings, UsageReport, UsageRow } from '../shared/types.ts';
import { hourlyLine, money, priceLine } from './price.ts';

const PROVIDERS = ['anthropic', 'openai', 'gemini', 'local'] as const;
const form = document.querySelector<HTMLFormElement>('#form')!;
const saved = document.querySelector<HTMLElement>('#saved')!;
const field = <T extends HTMLElement = HTMLInputElement>(name: string) => form.elements.namedItem(name) as unknown as T;

function el(tag: string, cls: string, text = ''): HTMLElement {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  e.textContent = text;
  return e;
}

/** One card per option: name, price, then the same four lines for every option. */
function renderChoices(list: HTMLElement, models: CatalogModel[], group: string, selected: string): void {
  list.replaceChildren();
  for (const m of models) {
    const card = el('label', m.available ? 'choice' : 'choice unavailable');
    const radio = document.createElement('input');
    radio.type = 'radio';
    radio.name = group;
    radio.value = m.id;
    radio.checked = m.id === selected;
    radio.disabled = !m.available;
    const head = el('div', 'choice-head');
    head.append(radio, el('strong', '', m.label), el('span', 'hourly', m.available ? hourlyLine(m) : '規劃中'));
    const rows = el('dl', 'choice-rows');
    for (const [k, v] of [
      ['價格', `${priceLine(m.price)}（${m.price_note}）`],
      ['速度', m.speed],
      ['強項', m.strength],
      ['注意', m.caution],
      ['需要', m.needs],
    ] as const) {
      rows.append(el('dt', '', k), el('dd', '', v));
    }
    card.append(head, rows);
    list.append(card);
  }
}

function chosen(group: string): string {
  return form.querySelector<HTMLInputElement>(`input[name="${group}"]:checked`)?.value ?? '';
}

function showLocalFields(): void {
  document.querySelector<HTMLElement>('#stt-local')!.hidden = chosen('stt') !== 'local-whisper';
  document.querySelector<HTMLElement>('#llm-local')!.hidden = chosen('llm') !== 'local-llm';
}

async function showKeyState(): Promise<void> {
  const status = await invoke<Record<string, boolean>>('key_status');
  for (const node of document.querySelectorAll<HTMLElement>('.keystate')) {
    const set = status[node.dataset.provider ?? ''] ?? false;
    node.textContent = set ? '（已設定）' : '（未設定）';
    node.style.color = set ? 'var(--zh)' : 'var(--muted)';
  }
}

// Key pages open in the browser (only these three are allowed: capabilities/settings-links.json).
for (const link of document.querySelectorAll<HTMLAnchorElement>('a.getkey')) {
  link.addEventListener('click', (e) => {
    e.preventDefault();
    void openUrl(link.href);
  });
}

// ---------- usage ----------
let report: UsageReport | null = null;
let range: keyof UsageReport = 'session';

function amount(r: UsageRow): string {
  if (r.audio_secs > 0) return `${(r.audio_secs / 60).toFixed(1)} 分鐘音訊`;
  return `${r.requests} 次 · 輸入 ${r.input.toLocaleString()} · 快取 ${r.cached.toLocaleString()} · 輸出 ${r.output.toLocaleString()} token`;
}

function renderUsage(): void {
  const table = document.querySelector<HTMLTableElement>('#usage')!;
  table.replaceChildren();
  const rows = report?.[range] ?? [];
  if (!rows.length) {
    table.append(el('caption', 'hint', '還沒有用量。'));
    return;
  }
  for (const r of rows) {
    const tr = document.createElement('tr');
    tr.append(el('td', '', r.label), el('td', 'muted', amount(r)), el('td', 'num', money(r.usd)));
    table.append(tr);
  }
  const total = document.createElement('tr');
  total.className = 'total';
  total.append(el('td', '', '合計'), el('td', '', ''), el('td', 'num', money(rows.reduce((s, r) => s + r.usd, 0))));
  table.append(total);
}

async function loadUsage(): Promise<void> {
  report = await invoke<UsageReport>('usage_report');
  renderUsage();
}

for (const b of document.querySelectorAll<HTMLButtonElement>('#usage-tabs [data-range]')) {
  b.addEventListener('click', () => {
    range = b.dataset.range as keyof UsageReport;
    for (const o of document.querySelectorAll('#usage-tabs [data-range]')) o.setAttribute('aria-pressed', String(o === b));
    renderUsage();
  });
}
document.querySelector('#usage-refresh')!.addEventListener('click', () => void loadUsage());

// ---------- load and save ----------
async function load(): Promise<void> {
  const [s, catalog] = await Promise.all([invoke<Settings>('get_settings'), invoke<CatalogModel[]>('catalog')]);
  renderChoices(document.querySelector('#stt-list')!, catalog.filter((m) => m.kind === 'stt'), 'stt', s.stt_id);
  renderChoices(document.querySelector('#llm-list')!, catalog.filter((m) => m.kind === 'llm'), 'llm', s.llm_id);
  field('stt_language').value = s.stt_language;
  field<HTMLSelectElement>('llm_effort').value = s.llm_effort;
  field('context_lines').value = String(s.context_lines);
  field<HTMLTextAreaElement>('glossary').value = s.glossary;
  field('local_stt_url').value = s.local_stt_url;
  field('local_stt_model').value = s.local_stt_model;
  field('local_llm_url').value = s.local_llm_url;
  field('local_llm_model').value = s.local_llm_model;
  showLocalFields();
  await Promise.all([showKeyState(), loadUsage()]);
}

form.addEventListener('change', (e) => {
  if ((e.target as HTMLInputElement).type === 'radio') showLocalFields();
});

form.addEventListener('submit', async (e) => {
  e.preventDefault();
  const value: Settings = {
    stt_id: chosen('stt'),
    stt_language: field('stt_language').value.trim(),
    llm_id: chosen('llm'),
    llm_effort: field<HTMLSelectElement>('llm_effort').value,
    context_lines: Number(field('context_lines').value) || 0,
    glossary: field<HTMLTextAreaElement>('glossary').value,
    local_stt_url: field('local_stt_url').value.trim(),
    local_stt_model: field('local_stt_model').value.trim(),
    local_llm_url: field('local_llm_url').value.trim(),
    local_llm_model: field('local_llm_model').value.trim(),
  };
  try {
    await invoke('save_settings', { value });
    for (const provider of PROVIDERS) {
      const input = field(`key_${provider}`);
      if (input.value.trim()) await invoke('set_key', { provider, key: input.value.trim() });
      input.value = '';
    }
    await showKeyState();
    saved.textContent = '已儲存。翻譯立即生效；語音辨識下次按「聆聽」時生效。';
  } catch (err) {
    saved.textContent = `儲存失敗：${String(err)}`;
  }
});

document.querySelector('#clear-keys')!.addEventListener('click', async () => {
  for (const provider of PROVIDERS) await invoke('set_key', { provider, key: '' });
  await showKeyState();
  saved.textContent = '所有 key 都已從鑰匙圈移除（若有設環境變數或 .env.local，開發版仍會使用）。';
});

await load();
