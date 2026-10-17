// The script window (optional): a bilingual script with tabs, cues and copy blocks.
// The text also goes to the Rust side, where both translation directions use it as context.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Session } from '../shared/types.ts';
import { privacyNote, viewControls } from '../shared/view.ts';
import { missingGloss, parse } from './format.ts';

const body = document.querySelector<HTMLElement>('#body')!;
const tabs = document.querySelector<HTMLElement>('#tabs')!;
const note = document.querySelector<HTMLElement>('#note')!;
const file = document.querySelector<HTMLInputElement>('#file')!;

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
    body.append(el('div', 'empty', '開啟一份講稿（.md）。格式見 examples/scripts/demo.md。'));
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
  note.textContent = missing.length ? `${missing.length} 句沒有中文對照` : '';
  body.scrollTop = top;
}

document.querySelector('#open')!.addEventListener('click', () => file.click());
file.addEventListener('change', async () => {
  const f = file.files?.[0];
  if (!f) return;
  try {
    await invoke('set_script', { text: await f.text() });
  } catch (e) {
    note.textContent = String(e);
  }
  file.value = '';
});

const bar = document.querySelector<HTMLElement>('.bar')!;
await viewControls('script', bar, privacyNote(bar));

void listen<string>('script', (e) => render(e.payload));
const snap = await invoke<Session>('snapshot');
render(snap.script);
