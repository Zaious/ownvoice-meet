// The settings window: the speech and translation switchers (prices and descriptions from the Rust
// catalog), local server fields, API keys (write-only; only whether a key is set comes back), the
// glossary, estimated usage, and the version with "Check for updates".

import { getVersion } from '@tauri-apps/api/app';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { openUrl } from '@tauri-apps/plugin-opener';
import type { CatalogModel, LiveCaptionsSetup, Settings, UsageReport, UsageRow, Views } from '../shared/types.ts';
import { setView } from '../shared/view.ts';
import { hourlyLine, money, priceLine } from './price.ts';
import { findNewer, installKind, installNewer, RELEASES_URL, type InstallKind, type Newer } from './updates.ts';

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
  followLiveCaptions(chosen('stt') === 'windows-live-captions');
}

// ---------- Windows Live Captions: a checklist that checks itself ----------
// First use needs the user's consent in Live Captions and the meeting language's speech files.
// The app cannot click those for the user, so it shows each step, opens the right place, and ticks
// the step off as soon as Windows reports it done. Switching Live Captions' language is done by the app.
const lcBox = document.querySelector<HTMLElement>('#stt-lc')!;
let lcTimer: ReturnType<typeof setInterval> | undefined;

function followLiveCaptions(on: boolean): void {
  lcBox.hidden = !on;
  if (lcTimer) clearInterval(lcTimer);
  lcTimer = undefined;
  if (!on) return;
  void checkLiveCaptions();
  lcTimer = setInterval(() => void checkLiveCaptions(), 2000);
}

function openButton(label: string, what: string): HTMLButtonElement {
  const b = document.createElement('button');
  b.type = 'button';
  b.textContent = label;
  b.addEventListener('click', () => void invoke('live_captions_open', { what }).catch((e) => (saved.textContent = String(e))));
  return b;
}

function lcStep(done: boolean, title: string, ...how: (string | HTMLElement)[]): HTMLElement {
  const li = el('li', done ? 'done' : '');
  const body = el('div', '');
  body.append(el('strong', '', title));
  const box = el('div', 'how');
  for (const h of how) box.append(typeof h === 'string' ? el('div', '', h) : h);
  body.append(box);
  li.append(el('span', 'mark', done ? '✓' : '○'), body);
  return li;
}

async function checkLiveCaptions(): Promise<void> {
  const s = await invoke<LiveCaptionsSetup>('live_captions_setup', { language: field('stt_language').value.trim() });
  if (lcBox.hidden) return;
  const english = s.meeting.toLowerCase().startsWith('en');
  const lang = english ? '英文' : s.meeting;
  const pick = english ? 'English (United States)' : s.meeting;
  const steps = el('ol', '');
  steps.append(
    lcStep(s.consented, '1. 同意使用即時輔助字幕（只需一次）',
      '按下面的按鈕，螢幕上方會出現「您要繼續並設定即時輔助字幕嗎？」，請按「是，繼續」。',
      openButton('開啟即時輔助字幕', 'live-captions')),
    lcStep(s.speech !== null, `2. 下載${lang}語音（只需一次，約幾分鐘）`,
      `接著出現「正準備在下列項目中顯示即時字幕」時，按語言旁邊的 ˅，選「${pick}」，再按「繼續」，等它下載完。`,
      `如果沒有出現那一行：按下面的按鈕開啟 Windows 語言設定，按「新增語言」，搜尋「${pick}」，勾選語音相關的項目後安裝。`,
      openButton('開啟 Windows 語言設定', 'language-settings')),
    lcStep(s.ready, `3. 即時輔助字幕改用${lang}`,
      `第 2 步完成後，App 會自動切換，不用動手。`),
  );
  const left = [s.consented, s.speech !== null, s.ready].filter((d) => !d).length;
  const verdict = el('p', 'verdict', s.ready ? '✓ 都準備好了，可以按「聆聽」。' : `還差 ${left} 步。這個清單每 2 秒自動檢查一次，做完會自己打勾。`);
  lcBox.replaceChildren(el('h3', '', '第一次使用前的準備'), steps, verdict);
}

field('stt_language').addEventListener('change', () => {
  if (!lcBox.hidden) void checkLiveCaptions();
});

async function showKeyState(): Promise<void> {
  const status = await invoke<Record<string, boolean>>('key_status');
  for (const node of document.querySelectorAll<HTMLElement>('.keystate')) {
    const set = status[node.dataset.provider ?? ''] ?? false;
    node.textContent = set ? '（已設定）' : '（未設定）';
    node.style.color = set ? 'var(--zh)' : 'var(--muted)';
  }
}

// Key pages open in the browser (only these three are allowed: capabilities/settings-links.json).
// Key pages and About links open in the browser; capabilities/settings-links.json and
// settings-about.json list the only addresses allowed.
for (const link of document.querySelectorAll<HTMLAnchorElement>('a.getkey, a.extlink')) {
  link.addEventListener('click', (e) => {
    e.preventDefault();
    void openUrl(link.href);
  });
}

// ---------- local servers: 測試連線 ----------
document.querySelector('#test-stt')!.addEventListener('click', async () => {
  const out = document.querySelector<HTMLElement>('#test-stt-result')!;
  out.textContent = '測試中…';
  try {
    out.textContent = await invoke<string>('test_local_stt', { url: field('local_stt_url').value, model: field('local_stt_model').value });
  } catch (e) {
    out.textContent = `失敗：${String(e)}`;
  }
});
document.querySelector('#test-llm')!.addEventListener('click', async () => {
  const out = document.querySelector<HTMLElement>('#test-llm-result')!;
  out.textContent = '測試中…';
  try {
    const models = await invoke<string[]>('local_models', { url: field('local_llm_url').value });
    const list = document.querySelector<HTMLDataListElement>('#local-models')!;
    list.replaceChildren(...models.map((m) => Object.assign(document.createElement('option'), { value: m })));
    if (!field('local_llm_model').value.trim()) field('local_llm_model').value = models[0] ?? '';
    out.textContent = `連線成功：${models.length} 個模型（點模型名稱欄位可以選）`;
  } catch (e) {
    out.textContent = `失敗：${String(e)}`;
  }
});

// ---------- usage ----------
let report: UsageReport | null = null;
let range: keyof UsageReport = 'session';

function amount(r: UsageRow): string {
  if (r.audio_secs > 0) return `${(r.audio_secs / 60).toFixed(1)} 分鐘音訊`;
  const early = r.early > 0 ? `（其中 ${r.early} 次是句子沒講完先翻）` : '';
  return `${r.requests} 次${early} · 輸入 ${r.input.toLocaleString()} · 快取 ${r.cached.toLocaleString()} · 輸出 ${r.output.toLocaleString()} token`;
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

// ---------- version and updates ----------
// Nothing is asked until the user presses "Check for updates". Only an installed copy updates
// itself; a portable copy or a dev build gets the release page.
const checkBtn = document.querySelector<HTMLButtonElement>('#check-update')!;
const installBtn = document.querySelector<HTMLButtonElement>('#install-update')!;
const releaseLink = document.querySelector<HTMLAnchorElement>('#release-link')!;
const updateNote = document.querySelector<HTMLElement>('#update-note')!;
let appVersion = '';
let kind: InstallKind = 'other';
let newer: Newer | null = null;

const KIND_LABEL: Record<InstallKind, string> = { installed: '安裝版', portable: '免安裝版', other: '開發版' };

async function showVersion(): Promise<void> {
  [appVersion, kind] = await Promise.all([getVersion().catch(() => ''), installKind()]);
  document.querySelector('#version')!.textContent = `OwnVoice Meet ${appVersion}（${KIND_LABEL[kind]}）`;
}

checkBtn.addEventListener('click', async () => {
  checkBtn.disabled = true;
  installBtn.hidden = true;
  updateNote.textContent = '檢查中…';
  newer = await findNewer(appVersion, kind);
  checkBtn.disabled = false;
  if (!newer) {
    updateNote.textContent = '已是最新版本（或暫時連不到 GitHub）。';
    return;
  }
  releaseLink.href = newer.url;
  releaseLink.hidden = false;
  if (newer.update) {
    installBtn.hidden = false;
    updateNote.textContent = `有新版本 ${newer.tag}。按「更新並重新啟動」：下載、驗證簽章、安裝，然後重新開啟。`;
  } else {
    updateNote.textContent = kind === 'portable'
      ? `有新版本 ${newer.tag}。免安裝版不會自己更新：到下載頁下載新的 zip。`
      : `有新版本 ${newer.tag}，見下載頁。`;
  }
});

installBtn.addEventListener('click', async () => {
  if (!newer?.update) return;
  installBtn.disabled = checkBtn.disabled = true;
  try {
    await installNewer(newer, (pct) => { updateNote.textContent = pct === null ? '下載中…' : `下載中… ${pct}%`; });
  } catch (err) {
    updateNote.textContent = `更新失敗：${String(err)}。可以到下載頁手動安裝。`;
    installBtn.disabled = checkBtn.disabled = false;
  }
});

releaseLink.addEventListener('click', (e) => {
  e.preventDefault();
  void openUrl(releaseLink.href || RELEASES_URL);
});

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
  hideFromCapture.checked = s.view.hide_from_capture;
  await Promise.all([showKeyState(), loadUsage(), showVersion()]);
}

// Applies at once, like the controls in the reading windows; the save button does not touch it.
const hideFromCapture = document.querySelector<HTMLInputElement>('#hide_from_capture')!;
hideFromCapture.addEventListener('change', () => void setView({ hide_from_capture: hideFromCapture.checked }));
void listen<Views>('view', (e) => (hideFromCapture.checked = e.payload.hide_from_capture));

form.addEventListener('change', (e) => {
  if ((e.target as HTMLInputElement).type === 'radio') showLocalFields();
});

form.addEventListener('submit', async (e) => {
  e.preventDefault();
  // The view is not on this form; the Rust side keeps what the windows last set.
  const value: Omit<Settings, 'view'> = {
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
