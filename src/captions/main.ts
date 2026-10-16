// The captions window (core): what others say, in English with the Chinese meaning under it.
// It also opens the optional windows and owns the global shortcut for the reply window.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { register, unregisterAll } from '@tauri-apps/plugin-global-shortcut';
import { FollowBottom } from '../shared/follow.ts';
import type { Segment, Session } from '../shared/types.ts';
import { openWindow, toggleWindow, type OptionalWindow } from '../shared/windows.ts';

const REPLY_SHORTCUT = 'CommandOrControl+Shift+Space';
const MAX_ROWS = 500;

const list = document.querySelector<HTMLElement>('#captions')!;
const status = document.querySelector<HTMLElement>('#status')!;
const listenBtn = document.querySelector<HTMLButtonElement>('#listen')!;
const follow = new FollowBottom(list);
const rows = new Map<number, HTMLElement>();

function render(seg: Segment): void {
  let row = rows.get(seg.id);
  if (!row) {
    list.querySelector('.empty')?.remove();
    row = document.createElement('div');
    row.innerHTML = '<div class="en"></div><div class="zh"></div>';
    list.append(row);
    rows.set(seg.id, row);
    if (rows.size > MAX_ROWS) {
      const [oldest] = rows.keys();
      if (oldest !== undefined) {
        rows.get(oldest)?.remove();
        rows.delete(oldest);
      }
    }
  }
  row.className = seg.final ? 'seg' : 'seg interim';
  row.querySelector('.en')!.textContent = seg.en;
  row.querySelector('.zh')!.textContent = seg.zh || (seg.final ? '…' : '');
  follow.settle();
}

function setListening(on: boolean): void {
  listenBtn.setAttribute('aria-pressed', String(on));
  listenBtn.textContent = on ? '■ 停止' : '▶ 聆聽';
  if (!on) document.querySelector<HTMLElement>('#level')!.textContent = '';
}

/** Keep a toggle button in step with its window, including when the user closes the window. */
async function track(name: OptionalWindow, btn: HTMLButtonElement): Promise<void> {
  const w = await WebviewWindow.getByLabel(name);
  btn.setAttribute('aria-pressed', String(w !== null));
  void w?.once('tauri://destroyed', () => btn.setAttribute('aria-pressed', 'false'));
}

for (const name of ['reply', 'script', 'settings'] as const) {
  const btn = document.querySelector<HTMLButtonElement>(`#${name}`)!;
  btn.addEventListener('click', async () => {
    await toggleWindow(name);
    await track(name, btn);
  });
}

listenBtn.addEventListener('click', async () => {
  const on = listenBtn.getAttribute('aria-pressed') !== 'true';
  try {
    setListening(await invoke<boolean>('set_listening', { on }));
  } catch (e) {
    status.textContent = String(e);
  }
});
document.querySelector('#demo')!.addEventListener('click', () => void invoke('start_demo'));

void listen<Segment>('caption', (e) => render(e.payload));
// A live hypothesis that ended with no final text.
void listen<number>('caption-remove', (e) => {
  rows.get(e.payload)?.remove();
  rows.delete(e.payload);
});
// Level meter: five bars over the loudness of the last 100 ms of system audio.
const level = document.querySelector<HTMLElement>('#level')!;
void listen<number>('level', (e) => {
  const bars = Math.min(5, Math.round(Math.sqrt(e.payload) * 12));
  level.textContent = '▮'.repeat(bars) + '▯'.repeat(5 - bars);
});
void listen<string>('status', (e) => (status.textContent = e.payload));
void listen<boolean>('listening', (e) => setListening(e.payload));

const snap = await invoke<Session>('snapshot');
snap.segments.forEach(render);
setListening(snap.listening);
status.textContent = snap.status;

// Type a reply without leaving the meeting app: the shortcut opens and focuses the reply window.
try {
  await unregisterAll();
  await register(REPLY_SHORTCUT, (e) => {
    if (e.state !== 'Pressed') return;
    void openWindow('reply').then(() => track('reply', document.querySelector('#reply')!));
  });
} catch (e) {
  status.textContent = `快捷鍵註冊失敗（可能被其他程式占用）：${String(e)}`;
}
