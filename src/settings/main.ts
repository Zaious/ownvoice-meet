// The settings window: the sound source (computer audio or a microphone, with a check) and what is
// saved, the speech and translation switchers (prices and descriptions from the Rust catalog), local
// server fields, API keys (write-only; only whether a key is set comes back), the glossary,
// estimated usage, and the version with "Check for updates".

import { getVersion } from '@tauri-apps/api/app';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { openUrl } from '@tauri-apps/plugin-opener';
import type { CatalogModel, LiveCaptionsSetup, MicCheck, Settings, UsageReport, UsageRow, Views } from '../shared/types.ts';
import { setView } from '../shared/view.ts';
import { judgeLevel } from './level.ts';
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

// The 聽 and 翻 lists fold away (settings.html <details>); the summary line names the current choice.
const models = new Map<string, CatalogModel>();

function showPicked(): void {
  for (const group of ['stt', 'llm']) {
    const m = models.get(chosen(group));
    document.querySelector(`#${group}-picked`)!.textContent = m ? `${m.label} · ${hourlyLine(m)}` : '（未選）';
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
// With the automatic check on (the default), opening Settings checks at once; otherwise nothing is
// asked until the user presses "Check for updates". Only an installed copy updates itself, and only
// when the user presses Update; a portable copy or a dev build gets the release page.
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

checkBtn.addEventListener('click', () => void runCheck());

async function runCheck(): Promise<void> {
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
}

// Saved with the rest of the form, at once (the captions window reads it at the next start).
const updateCheck = document.querySelector<HTMLInputElement>('#update_check')!;
/** Set once the form shows the saved settings: saving before that would save empty fields. */
let ready = false;

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
  for (const m of catalog) models.set(m.id, m);
  showPicked();
  choose('stt_language', s.stt_language);
  choose('speak_language', s.speak_language || 'en');
  field<HTMLSelectElement>('llm_effort').value = s.llm_effort;
  field('context_lines').value = String(s.context_lines);
  field('summary_every_min').value = String(s.summary_every_min);
  field<HTMLTextAreaElement>('glossary').value = s.glossary;
  field('local_stt_url').value = s.local_stt_url;
  field('local_stt_model').value = s.local_stt_model;
  field('local_llm_url').value = s.local_llm_url;
  field('local_llm_model').value = s.local_llm_model;
  showLocalFields();
  hideFromCapture.checked = s.view.hide_from_capture;
  await loadMicrophones(s.mic_device);
  // A Mac cannot record what it plays: the microphone only.
  if ((await invoke<string>('platform').catch(() => '')) === 'macos') {
    const loop = form.querySelector<HTMLInputElement>('input[name="audio_source"][value="loopback"]');
    if (loop) {
      loop.disabled = true;
      loop.parentElement?.append('（Mac 不支援，請用麥克風）');
    }
  }
  setSource(s.audio_source === 'mic' ? 'mic' : 'loopback');
  field<HTMLSelectElement>('mic_gain').value = String(nearestGain(s.mic_gain));
  field<HTMLSelectElement>('save_mode').value = s.save_mode || 'off';
  field('archive_dir').value = s.archive_dir;
  field<HTMLTextAreaElement>('agenda').value = s.agenda;
  field('idle_pause_min').value = String(s.idle_pause_min);
  field('idle_end_min').value = String(s.idle_end_min);
  updateCheck.checked = s.update_check;
  ready = true;
  await Promise.all([showKeyState(), loadUsage(), showVersion()]);
  if (s.update_check) void runCheck();
}

// ---------- sound source and saving ----------
const micBox = document.querySelector<HTMLElement>('#mic-box')!;

function source(): string {
  return form.querySelector<HTMLInputElement>('input[name="audio_source"]:checked')?.value ?? 'loopback';
}

function setSource(value: string): void {
  const radio = form.querySelector<HTMLInputElement>(`input[name="audio_source"][value="${value}"]`);
  if (radio) radio.checked = true;
  micBox.hidden = value !== 'mic';
}

/** Selects `value` in a dropdown, adding it when the list does not have it (set by hand before). */
function choose(name: string, value: string): void {
  const select = field<HTMLSelectElement>(name);
  if (![...select.options].some((o) => o.value === value)) select.add(new Option(value, value));
  select.value = value;
}

/** A minutes field: empty means the default, anything else at least 0. */
function minutesField(name: string, fallback: number): number {
  const raw = field(name).value.trim();
  const n = Number(raw);
  return raw === '' || !Number.isFinite(n) ? fallback : Math.max(0, n);
}

function nearestGain(g: number): number {
  return [1, 2, 4, 8].reduce((best, x) => (Math.abs(x - g) < Math.abs(best - g) ? x : best), 1);
}

/** The microphones this computer has; a saved one that is missing stays listed, marked. */
async function loadMicrophones(saved: string): Promise<void> {
  const select = field<HTMLSelectElement>('mic_device');
  const names = await invoke<string[]>('list_microphones').catch(() => [] as string[]);
  select.replaceChildren(new Option('預設麥克風', ''));
  for (const n of names) select.append(new Option(n, n));
  if (saved && !names.includes(saved)) select.append(new Option(`${saved}（現在找不到）`, saved));
  select.value = saved;
}

for (const radio of form.querySelectorAll<HTMLInputElement>('input[name="audio_source"]')) {
  radio.addEventListener('change', () => {
    setSource(source());
    // A talk in the room is usually worth keeping; the user can still pick 不存.
    const save = field<HTMLSelectElement>('save_mode');
    if (source() === 'mic' && save.value === 'off') save.value = 'audio';
  });
}

const micMeter = document.querySelector<HTMLElement>('#mic-meter')!;
const micResult = document.querySelector<HTMLElement>('#mic-result')!;
void listen<number>('mic-level', (e) => {
  const bars = Math.min(10, Math.round(Math.sqrt(e.payload) * 24));
  micMeter.textContent = '▮'.repeat(bars) + '▯'.repeat(10 - bars);
});

document.querySelector('#mic-check')!.addEventListener('click', async (e) => {
  const btn = e.currentTarget as HTMLButtonElement;
  btn.disabled = true;
  micResult.textContent = '檢查中，請讓現場有人說話…';
  try {
    const r = await invoke<MicCheck>('mic_check', {
      device: field<HTMLSelectElement>('mic_device').value,
      gain: Number(field<HTMLSelectElement>('mic_gain').value) || 1,
    });
    micResult.textContent = `${r.device}：最大 ${Math.round(r.peak * 100)}%。${judgeLevel(r.peak)}`;
  } catch (err) {
    micResult.textContent = `檢查失敗：${String(err)}`;
  } finally {
    btn.disabled = false;
    micMeter.textContent = '';
  }
});

document.querySelector('#open-archive')!.addEventListener('click', async () => {
  const out = document.querySelector<HTMLElement>('#archive-result')!;
  try {
    await invoke('open_archive_dir');
    out.textContent = '';
  } catch (err) {
    out.textContent = `打不開：${String(err)}`;
  }
});

// Applies at once, like the controls in the reading windows; the save button does not touch it.
const hideFromCapture = document.querySelector<HTMLInputElement>('#hide_from_capture')!;
hideFromCapture.addEventListener('change', () => void setView({ hide_from_capture: hideFromCapture.checked }));
void listen<Views>('view', (e) => (hideFromCapture.checked = e.payload.hide_from_capture));

form.addEventListener('change', (e) => {
  if ((e.target as HTMLInputElement).type === 'radio') {
    showLocalFields();
    showPicked();
  }
});

/** The form's settings. The view is not on this form; the Rust side keeps what the windows last set. */
function collect(): Omit<Settings, 'view'> {
  return {
    stt_id: chosen('stt'),
    stt_language: field('stt_language').value.trim(),
    speak_language: field<HTMLSelectElement>('speak_language').value,
    llm_id: chosen('llm'),
    llm_effort: field<HTMLSelectElement>('llm_effort').value,
    context_lines: Number(field('context_lines').value) || 0,
    glossary: field<HTMLTextAreaElement>('glossary').value,
    local_stt_url: field('local_stt_url').value.trim(),
    local_stt_model: field('local_stt_model').value.trim(),
    local_llm_url: field('local_llm_url').value.trim(),
    local_llm_model: field('local_llm_model').value.trim(),
    audio_source: source(),
    mic_device: field<HTMLSelectElement>('mic_device').value,
    mic_gain: Number(field<HTMLSelectElement>('mic_gain').value) || 1,
    save_mode: field<HTMLSelectElement>('save_mode').value,
    archive_dir: field('archive_dir').value.trim(),
    agenda: field<HTMLTextAreaElement>('agenda').value,
    idle_pause_min: minutesField('idle_pause_min', 3),
    idle_end_min: minutesField('idle_end_min', 20),
    update_check: updateCheck.checked,
    summary_every_min: minutesField('summary_every_min', 5),
  };
}

// Everything saves itself as it changes: a user who closed the window without finding the save
// button lost every setting (reported on 0.3.0). Saves run one after another, in order.
let saving: Promise<void> = Promise.resolve();
/** Saves the form; `keys`: also the typed keys. `clear` then empties those fields (a saved key never
 *  comes back to this window); not while someone may still be typing one. */
function save(keys: boolean, clear = keys): Promise<void> {
  if (!ready) return saving;
  saving = saving.then(async () => {
    try {
      await invoke('save_settings', { value: collect() });
      if (keys) {
        for (const provider of PROVIDERS) {
          const input = field(`key_${provider}`);
          const key = input.value.trim();
          if (!key) continue;
          await invoke('set_key', { provider, key });
          if (clear) input.value = '';
        }
        await showKeyState();
      }
      saved.textContent = `已自動儲存（${new Date().toLocaleTimeString('zh-TW', { hour: '2-digit', minute: '2-digit', hour12: false })}）。翻譯立即生效；語音辨識下次按「聆聽」時生效。`;
    } catch (err) {
      saved.textContent = `儲存失敗：${String(err)}`;
    }
  });
  return saving;
}

const isKey = (t: EventTarget | null): boolean => t instanceof HTMLInputElement && t.name.startsWith('key_');
let typing: number | undefined;
form.addEventListener('input', (e) => {
  window.clearTimeout(typing);
  // A key is saved after a pause in typing but stays in its field until the field is left (change).
  typing = window.setTimeout(() => void save(isKey(e.target), false), isKey(e.target) ? 1500 : 600);
});
form.addEventListener('change', (e) => {
  window.clearTimeout(typing);
  void save(isKey(e.target));
});
form.addEventListener('submit', (e) => {
  e.preventDefault();
  window.clearTimeout(typing);
  void save(true);
});

document.querySelector('#clear-keys')!.addEventListener('click', async () => {
  for (const provider of PROVIDERS) await invoke('set_key', { provider, key: '' });
  await showKeyState();
  saved.textContent = '所有 key 都已從鑰匙圈移除（若有設環境變數或 .env.local，開發版仍會使用）。';
});

await load();
