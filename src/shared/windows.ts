// Opens, focuses and closes the optional windows (reply, script) from the captions window.
// The creation wait follows Basesmall's panel manager: the created event can fire before its
// listener is attached, so the window is also looked up by label.

import { WebviewWindow } from '@tauri-apps/api/webviewWindow';

export type OptionalWindow = 'reply' | 'script' | 'settings';

// Reading windows are transparent so their background can fade (shared/view.ts); settings is not.
const SPEC: Record<OptionalWindow, { url: string; title: string; width: number; height: number; transparent?: boolean }> = {
  reply: { url: 'reply.html', title: 'OwnVoice Meet · Reply', width: 640, height: 360, transparent: true },
  script: { url: 'script.html', title: 'OwnVoice Meet · Script', width: 620, height: 760, transparent: true },
  settings: { url: 'settings.html', title: 'OwnVoice Meet · Settings', width: 560, height: 720 },
};

const creating = new Map<OptionalWindow, Promise<WebviewWindow | null>>();

export async function isOpen(name: OptionalWindow): Promise<boolean> {
  return (await WebviewWindow.getByLabel(name)) !== null;
}

/** Open the window if it is closed, and bring it to the front. */
export async function openWindow(name: OptionalWindow, focus = true): Promise<WebviewWindow | null> {
  const existing = (await WebviewWindow.getByLabel(name)) ?? (await creating.get(name));
  if (existing) {
    if (focus) await existing.setFocus();
    return existing;
  }
  const spec = SPEC[name];
  const made = new Promise<WebviewWindow | null>((resolve) => {
    const w = new WebviewWindow(name, {
      url: spec.url,
      title: spec.title,
      width: spec.width,
      height: spec.height,
      minWidth: 280,
      minHeight: 140,
      alwaysOnTop: true,
      transparent: spec.transparent ?? false,
      focus,
    });
    void w.once('tauri://created', () => resolve(w));
    void w.once('tauri://error', () => resolve(null));
    const started = Date.now();
    const poll = setInterval(() => {
      void WebviewWindow.getByLabel(name).then((found) => {
        if (found || Date.now() - started > 5000) {
          clearInterval(poll);
          resolve(found);
        }
      });
    }, 50);
  });
  creating.set(name, made);
  try {
    return await made;
  } finally {
    creating.delete(name);
  }
}

export async function closeWindow(name: OptionalWindow): Promise<void> {
  await (await creating.get(name));
  await (await WebviewWindow.getByLabel(name))?.close();
}

export async function toggleWindow(name: OptionalWindow): Promise<boolean> {
  if (await isOpen(name)) {
    await closeWindow(name);
    return false;
  }
  return (await openWindow(name)) !== null;
}
