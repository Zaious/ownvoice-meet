// The script window (optional): a bilingual script with tabs, cues and copy blocks.
// The text also goes to the Rust side, where both translation directions use it as context, and is
// saved so it is still there next time. Edit mode writes it in place with a live preview; "補對照"
// fills in the missing half of each line (src-tauri/src/gloss.rs).

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Automation, Session } from '../shared/types.ts';
import { privacyNote, viewControls } from '../shared/view.ts';
import { missingGloss, parse } from './format.ts';
import { TEMPLATE } from './template.ts';

const body = document.querySelector<HTMLElement>('#body')!;
const tabs = document.querySelector<HTMLElement>('#tabs')!;
const note = document.querySelector<HTMLElement>('#note')!;
const file = document.querySelector<HTMLInputElement>('#file')!;
const editor = document.querySelector<HTMLElement>('#editor')!;
const src = document.querySelector<HTMLTextAreaElement>('#src')!;
const editBtn = document.querySelector<HTMLButtonElement>('#edit')!;
const glossBtn = document.querySelector<HTMLButtonElement>('#gloss')!;
const EMPTY = '還沒有講稿。按「編輯」直接寫，或「開啟講稿…」讀 .md 檔。只寫英文或只寫中文都可以，按「補對照」補上另一半。';

let current = '';
let editing = false;

function el(tag: string, cls: string, text: string): HTMLElement {
  const e = document.createElement(tag);
  e.className = cls;
  e.textContent = text;
  return e;
}

function render(source: string): void {
  const top = body.scrollTop;
  const { sections, items } = parse(source);
  body.replaceChildren();
  tabs.replaceChildren();
  if (!source.trim()) {
    body.append(el('div', 'empty', EMPTY));
    note.textContent = '';
    return;
  }
  for (const it of items) {
    if (it.kind === 'section') {
      const h = el('h2', '', it.text);
      h.id = `s${it.index}`;
      body.append(h);
    } else if (it.kind === 'copy') {
      const box = el('div', 'copy', '');
      const btn = el('button', '', '📋 複製');
      btn.addEventListener('click', async () => {
        await navigator.clipboard.writeText(it.text);
        btn.textContent = '✓ 已複製';
        setTimeout(() => (btn.textContent = '📋 複製'), 1200);
      });
      box.append(el('pre', '', it.text), btn);
      body.append(box);
    } else {
      body.append(el('div', it.kind, it.kind === 'question' ? `Q: ${it.text}` : it.text));
    }
  }
  sections.forEach((title, i) => {
    const b = el('button', '', title);
    b.addEventListener('click', () => document.getElementById(`s${i}`)?.scrollIntoView({ block: 'start' }));
    tabs.append(b);
  });
  const missing = missingGloss(items);
  note.textContent = missing.length ? `${missing.length} 句還沒有對照，按「補對照」自動補上。` : '';
  body.scrollTop = top;
}

async function save(text: string): Promise<void> {
  try {
    await invoke('set_script', { text });
    current = text;
  } catch (e) {
    note.textContent = String(e);
  }
}

// ---------- edit mode ----------
let saveTimer: number | undefined;
let previewTimer: number | undefined;

function setEditing(on: boolean): void {
  editing = on;
  editor.hidden = !on;
  editBtn.setAttribute('aria-pressed', String(on));
  body.classList.toggle('preview', on);
  if (on) {
    src.value = current;
    src.focus();
  }
}

src.addEventListener('input', () => {
  clearTimeout(previewTimer);
  previewTimer = window.setTimeout(() => render(src.value), 250);
  clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => void save(src.value), 1000);
});

editBtn.addEventListener('click', async () => {
  if (editing) {
    clearTimeout(saveTimer);
    await save(src.value);
  }
  setEditing(!editing);
});

document.querySelector('#done')!.addEventListener('click', async () => {
  clearTimeout(saveTimer);
  await save(src.value);
  setEditing(false);
});

document.querySelector('#template')!.addEventListener('click', () => {
  if (!src.value.trim()) {
    src.value = TEMPLATE;
  } else {
    const at = src.selectionStart;
    src.value = `${src.value.slice(0, at)}\n${TEMPLATE}\n${src.value.slice(at)}`;
  }
  src.dispatchEvent(new Event('input'));
});

// ---------- fill in the other half ----------
glossBtn.addEventListener('click', async () => {
  const text = editing ? src.value : current;
  if (!text.trim()) {
    note.textContent = '先寫一點講稿，再按「補對照」。';
    return;
  }
  clearTimeout(saveTimer);
  glossBtn.disabled = true;
  note.textContent = '補對照中…';
  try {
    const n = await invoke<number>('gloss_script', { text });
    note.textContent = n ? `補好了 ${n} 行。` : '每一行都已經有對照了。';
  } catch (e) {
    note.textContent = `補對照失敗：${String(e)}`;
  } finally {
    glossBtn.disabled = false;
  }
});

// ---------- open a file ----------
document.querySelector('#open')!.addEventListener('click', () => file.click());
file.addEventListener('change', async () => {
  const f = file.files?.[0];
  if (!f) return;
  await save(await f.text());
  if (editing) src.value = current;
  file.value = '';
});

const bar = document.querySelector<HTMLElement>('.bar')!;
await viewControls('script', bar, privacyNote(bar));

void listen<string>('script', (e) => {
  current = e.payload;
  // Our own typing comes back here too; only replace the editor text when it changed elsewhere
  // (補對照, a file, Demo), so the cursor does not jump.
  if (editing && e.payload !== src.value) src.value = e.payload;
  render(e.payload);
});
const snap = await invoke<Session>('snapshot');
current = snap.script;
render(current);

// Scripted demos (debug builds only): start on a given tab.
const auto = await invoke<Automation | null>('automation');
if (auto?.script_tab) {
  [...tabs.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent === auto.script_tab)?.click();
}
