// The summary window (optional): the main points so far, brought up to date every few minutes from
// the finished caption lines (summary.rs). It sums up what others said; it never answers for you.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { SummaryView, Views } from '../shared/types.ts';
import { privacyNote } from '../shared/view.ts';

const box = document.querySelector<HTMLElement>('#summary')!;
const state = document.querySelector<HTMLElement>('#state')!;
const now = document.querySelector<HTMLButtonElement>('#now')!;
const EMPTY = box.innerHTML;

function show(v: SummaryView): void {
  if (v.text.trim()) {
    const list = document.createElement('ul');
    for (const raw of v.text.split('\n')) {
      const line = raw.replace(/^\s*[-•*]\s*/, '').trim();
      if (!line) continue;
      const li = document.createElement('li');
      li.textContent = line;
      list.append(li);
    }
    box.replaceChildren(list);
  } else {
    box.innerHTML = EMPTY;
  }
  const parts: string[] = [];
  if (v.working) parts.push('整理中…');
  else if (v.updated) parts.push(`${v.updated} 更新，共 ${v.lines} 句`);
  if (v.error) parts.push(v.error);
  state.textContent = parts.join('　');
  now.disabled = v.working;
}

now.addEventListener('click', async () => {
  const sent = await invoke<boolean>('summarize_now').catch(() => false);
  if (!sent) state.textContent = '目前沒有新的句子可以摘要';
});

void listen<SummaryView>('summary', (e) => show(e.payload));
show(await invoke<SummaryView>('get_summary'));

const bar = document.querySelector<HTMLElement>('#bar')!;
const note = privacyNote(bar);
note(await invoke<Views>('get_view'));
void listen<Views>('view', (e) => note(e.payload));
