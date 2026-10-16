// Mirrors src-tauri/src/session.rs. The Rust side owns the state; windows render it.

export interface Segment {
  id: number;
  en: string;
  zh: string;
  final: boolean;
}

export interface Reply {
  id: number;
  zh: string;
  en: string;
  done: boolean;
}

/** Mirrors src-tauri/src/settings.rs. API keys are not part of it. */
export interface Settings {
  stt_model: string;
  stt_language: string;
  llm_model: string;
  llm_effort: string;
  context_lines: number;
  glossary: string;
}

export interface Session {
  segments: Segment[];
  reply: Reply | null;
  script: string;
  listening: boolean;
  status: string;
}
