// The settings window: providers, models, glossary, and API keys (write-only; only whether a key
// is set comes back from the Rust side).

import { invoke } from '@tauri-apps/api/core';
import type { Settings } from '../shared/types.ts';

const form = document.querySelector<HTMLFormElement>('#form')!;
const saved = document.querySelector<HTMLElement>('#saved')!;
const field = <T extends HTMLElement = HTMLInputElement>(name: string) => form.elements.namedItem(name) as unknown as T;

async function showKeyState(): Promise<void> {
  const status = await invoke<Record<string, boolean>>('key_status');
  for (const el of document.querySelectorAll<HTMLElement>('.keystate')) {
    const set = status[el.dataset.provider ?? ''] ?? false;
    el.textContent = set ? '（已設定）' : '（未設定）';
    el.style.color = set ? 'var(--zh)' : 'var(--muted)';
  }
}

async function load(): Promise<void> {
  const s = await invoke<Settings>('get_settings');
  field('stt_model').value = s.stt_model;
  field('stt_language').value = s.stt_language;
  field('llm_model').value = s.llm_model;
  field<HTMLSelectElement>('llm_effort').value = s.llm_effort;
  field('context_lines').value = String(s.context_lines);
  field<HTMLTextAreaElement>('glossary').value = s.glossary;
  await showKeyState();
}

form.addEventListener('submit', async (e) => {
  e.preventDefault();
  const value: Settings = {
    stt_model: field('stt_model').value.trim(),
    stt_language: field('stt_language').value.trim(),
    llm_model: field('llm_model').value.trim(),
    llm_effort: field<HTMLSelectElement>('llm_effort').value,
    context_lines: Number(field('context_lines').value) || 0,
    glossary: field<HTMLTextAreaElement>('glossary').value,
  };
  try {
    await invoke('save_settings', { value });
    for (const provider of ['gemini', 'anthropic']) {
      const input = field(`key_${provider}`);
      if (input.value.trim()) await invoke('set_key', { provider, key: input.value.trim() });
      input.value = '';
    }
    await showKeyState();
    saved.textContent = '已儲存。下次按「聆聽」時生效。';
  } catch (err) {
    saved.textContent = `儲存失敗：${String(err)}`;
  }
});

document.querySelector('#clear-keys')!.addEventListener('click', async () => {
  for (const provider of ['gemini', 'anthropic']) await invoke('set_key', { provider, key: '' });
  await showKeyState();
  saved.textContent = '兩把 key 都已從鑰匙圈移除（若有設環境變數，仍會使用環境變數）。';
});

await load();
