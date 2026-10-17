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
  stt_id: string;
  stt_language: string;
  llm_id: string;
  llm_effort: string;
  context_lines: number;
  glossary: string;
  local_stt_url: string;
  local_stt_model: string;
  local_llm_url: string;
  local_llm_model: string;
}

/** Mirrors catalog.rs. */
export type Price =
  | { unit: 'per_minute'; usd: number }
  | { unit: 'per_m_tok'; input: number; cached: number; output: number }
  | { unit: 'free' };

export interface CatalogModel {
  id: string;
  kind: 'stt' | 'llm';
  provider: string;
  label: string;
  price: Price;
  price_note: string;
  speed: string;
  strength: string;
  caution: string;
  needs: string;
  available: boolean;
}

/** Mirrors live_captions.rs Setup: how far Windows Live Captions is set up. */
export interface LiveCaptionsSetup {
  installed: boolean;
  consented: boolean;
  /** Installed speech files for the meeting language, e.g. "en-US". */
  speech: string | null;
  language: string | null;
  meeting: string;
  ready: boolean;
}

/** Mirrors usage.rs. */
export interface UsageRow {
  id: string;
  label: string;
  provider: string;
  audio_secs: number;
  requests: number;
  /** Of `requests`, translations of a sentence still being spoken. */
  early: number;
  input: number;
  cached: number;
  cache_write: number;
  output: number;
  usd: number;
}

export interface UsageReport {
  session: UsageRow[];
  today: UsageRow[];
  month: UsageRow[];
}

export interface Session {
  segments: Segment[];
  reply: Reply | null;
  script: string;
  listening: boolean;
  status: string;
}
