// Mirrors src-tauri/src/session.rs. The Rust side owns the state; windows render it.

export interface Segment {
  id: number;
  en: string;
  zh: string;
  final: boolean;
  /** Who said it (speakers.rs), once known: the name shown and the speaker's number. */
  speaker?: string;
  who?: number;
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
  /** The language the user speaks: replies and the script's 補對照 ("en", "ja", ...). */
  speak_language: string;
  llm_id: string;
  llm_effort: string;
  context_lines: number;
  glossary: string;
  local_stt_url: string;
  local_stt_model: string;
  local_llm_url: string;
  local_llm_model: string;
  /** "loopback" (what the computer plays) or "mic". */
  audio_source: string;
  mic_device: string;
  mic_gain: number;
  /** "off", "text" or "audio" (recording and transcript). */
  save_mode: string;
  archive_dir: string;
  agenda: string;
  /** Minutes of quiet before the speech service is let go (0 = never). */
  idle_pause_min: number;
  /** Minutes of quiet before listening ends by itself (0 = never). */
  idle_end_min: number;
  /** Ask GitHub for a newer version 20 s after start and show a reminder. */
  update_check: boolean;
  /** Minutes between live summary updates (0 = no live summary). */
  summary_every_min: number;
  /** Label caption lines by speaker, from voice prints worked out on this computer. */
  speaker_labels: boolean;
  view: Views;
}

/** chatgpt.rs Status: the Sign in with ChatGPT connection. */
export interface ChatgptStatus {
  signed_in: boolean;
  plan: boolean;
  email: string;
}

/** summary.rs View: the live summary. */
export interface SummaryView {
  text: string;
  updated: string;
  lines: number;
  working: boolean;
  error: string;
}

/** archive.rs SegmentInfo: the saved session's current segment. */
export interface SegmentInfo {
  index: number;
  name: string;
  recording: boolean;
  dir: string;
}

export interface MicCheck {
  device: string;
  peak: number;
  average: number;
}

/** Mirrors settings.rs View / Views / ViewPatch. */
export interface View {
  font_px: number;
  opacity: number;
}

export interface Views {
  captions: View;
  reply: View;
  script: View;
  hide_from_capture: boolean;
  zh_only: boolean;
  /** The reply window opens with the app; closing it turns this off. */
  reply_open: boolean;
}

export interface ViewPatch {
  window?: 'captions' | 'reply' | 'script';
  font_px?: number;
  opacity?: number;
  hide_from_capture?: boolean;
  zh_only?: boolean;
}

/** Debug builds only: what OWNVOICE_AUTOMATION asks the windows to do at start (lib.rs). */
export interface Automation {
  open?: string[];
  listen?: boolean;
  script_tab?: string;
  /** Typed into the reply window after reply_after_ms, then sent. */
  reply_zh?: string;
  reply_after_ms?: number;
}

/** Mirrors catalog.rs. */
export type Price =
  | { unit: 'per_minute'; usd: number }
  | { unit: 'per_m_tok'; input: number; cached: number; output: number }
  | { unit: 'free' }
  | { unit: 'plan' };

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
