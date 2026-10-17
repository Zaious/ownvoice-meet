// Text size and background opacity for a reading window (captions, reply, script). The values live
// in the Rust settings (settings.rs Views); a change from any window comes back as a "view" event,
// so every window follows it. Only the background fades: text stays opaque, and opacity stops at
// 40% because a fully clear window over a busy meeting cannot be read.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ViewPatch, Views } from './types.ts';

export type ViewWindow = 'captions' | 'reply' | 'script';

export const FONT_STEPS = [10, 11, 12, 13, 14, 15, 16, 18, 20, 22, 24, 28, 32, 36, 40, 44, 48];
export const OPACITY_MIN = 40;

/** The next text size up or down; a size between steps moves to the nearest step that way. */
export function stepFont(px: number, dir: 1 | -1): number {
  if (dir > 0) return FONT_STEPS.find((s) => s > px) ?? FONT_STEPS[FONT_STEPS.length - 1]!;
  return [...FONT_STEPS].reverse().find((s) => s < px) ?? FONT_STEPS[0]!;
}

function paint(font: number, opacity: number): void {
  const root = document.documentElement.style;
  root.setProperty('--base', `${font}px`);
  root.setProperty('--bg-alpha', String(opacity / 100));
}

export function setView(patch: ViewPatch): Promise<Views> {
  return invoke<Views>('set_view', { patch });
}

/**
 * Put A− / A+ and the opacity slider into `host`, and keep this window in step with the settings.
 * `onView` also hears every change (for the window's own options).
 */
export async function viewControls(name: ViewWindow, host: HTMLElement, onView?: (v: Views) => void): Promise<Views> {
  const box = document.createElement('span');
  box.className = 'viewctl';
  const smaller = button('A−', '字小一點');
  const bigger = button('A+', '字大一點');
  const fade = document.createElement('input');
  fade.type = 'range';
  fade.min = String(OPACITY_MIN);
  fade.max = '100';
  fade.step = '5';
  fade.title = '背景不透明度';
  fade.setAttribute('aria-label', '背景不透明度');
  box.append(smaller, bigger, fade);
  host.append(box);

  let current = await invoke<Views>('get_view');
  const show = (v: Views): void => {
    current = v;
    paint(v[name].font_px, v[name].opacity);
    fade.value = String(v[name].opacity);
    onView?.(v);
  };
  smaller.addEventListener('click', () => void setView({ window: name, font_px: stepFont(current[name].font_px, -1) }));
  bigger.addEventListener('click', () => void setView({ window: name, font_px: stepFont(current[name].font_px, 1) }));
  // Dragging redraws at once; letting go saves.
  fade.addEventListener('input', () => paint(current[name].font_px, Number(fade.value)));
  fade.addEventListener('change', () => void setView({ window: name, opacity: Number(fade.value) }));
  void listen<Views>('view', (e) => show(e.payload));
  show(current);
  return current;
}

function button(text: string, title: string): HTMLButtonElement {
  const b = document.createElement('button');
  b.type = 'button';
  b.textContent = text;
  b.title = title;
  b.setAttribute('aria-label', `${text} ${title}`);
  return b;
}

/** A note in the reply and script windows saying whether screen sharing can see them. */
export function privacyNote(host: HTMLElement): (v: Views) => void {
  const note = document.createElement('span');
  note.className = 'private';
  host.append(note);
  return (v) => {
    note.textContent = v.hide_from_capture ? '分享螢幕時隱藏' : '分享螢幕時看得到';
    note.title = v.hide_from_capture
      ? '這個視窗不會出現在螢幕分享、錄影與截圖裡（Windows 10 2004 以上）。可在「設定」關閉。'
      : '這個視窗會出現在螢幕分享裡。可在「設定」改成隱藏。';
    note.dataset.on = String(v.hide_from_capture);
  };
}
