// The reply window (optional): type Chinese, get English to read aloud in large text.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Automation, Reply, Session } from '../shared/types.ts';
import { privacyNote, viewControls } from '../shared/view.ts';

const prompter = document.querySelector<HTMLElement>('#prompter')!;
const input = document.querySelector<HTMLTextAreaElement>('#input')!;

function show(r: Reply | null): void {
  prompter.replaceChildren();
  if (!r) return;
  const zh = document.createElement('div');
  zh.className = 'zh';
  zh.textContent = r.zh;
  const en = document.createElement('div');
  en.className = 'en';
  en.textContent = r.en || '…';
  prompter.append(zh, en);
}

function submit(): void {
  const zh = input.value.trim();
  if (!zh) return;
  input.value = '';
  show({ id: 0, zh, en: '', done: false });
  invoke('compose_reply', { zh }).catch((err) => show({ id: 0, zh, en: `[${String(err)}]`, done: true }));
}

input.addEventListener('keydown', (e) => {
  // Enter while the IME is composing only confirms the characters.
  if (e.key !== 'Enter' || e.shiftKey || e.isComposing) return;
  e.preventDefault();
  submit();
});

const bar = document.querySelector<HTMLElement>('#bar')!;
await viewControls('reply', bar, privacyNote(bar));

void listen<Reply>('reply', (e) => show(e.payload));
const snap = await invoke<Session>('snapshot');
show(snap.reply);
input.focus();

// Scripted demos (debug builds only): type a reply after a delay, one character at a time, and send it.
const auto = await invoke<Automation | null>('automation');
if (auto?.reply_zh) {
  const text = auto.reply_zh;
  setTimeout(() => {
    let i = 0;
    const typing = setInterval(() => {
      input.value = text.slice(0, ++i);
      if (i >= text.length) {
        clearInterval(typing);
        setTimeout(submit, 700);
      }
    }, 70);
  }, auto.reply_after_ms ?? 0);
}
