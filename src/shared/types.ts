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

export interface Session {
  segments: Segment[];
  reply: Reply | null;
  script: string;
  listening: boolean;
  status: string;
}
