# Architecture

OwnVoice Meet helps someone take part in an online meeting held in a language they read better than they speak. It does three jobs, each in its own window:

| Window | Required | What it does |
|---|---|---|
| Captions (`main`) | yes | Captures the system audio (what the meeting app plays), shows English captions (gray while someone is still talking, white once a sentence ends) with the Chinese meaning streamed under each line. |
| Reply (`reply`) | optional | You type Chinese; it shows short spoken English to read aloud, in large type. |
| Script (`script`) | optional | A bilingual script and notes: tabs, stage cues, prepared Q&A, copy-to-clipboard blocks. |

Separate windows let each one sit where it helps: captions along the bottom of the screen, the reply text next to the webcam so you look at the other person while reading, the script on a second monitor.

## One state, owned by Rust

```
 system audio ──► capture ──► speech-to-text ──► translate ──┐
 (WASAPI loopback)   (Rust)     (cloud / local)    (LLM)       ▼
                                                     Session (Rust, Mutex)
 reply window ── compose_reply ───────── LLM ───────►  segments · reply · script
 script window ── set_script ────────────────────────►        │
                                                              ▼ events: caption, reply, script, status
                                          captions · reply · script windows render
```

- `src-tauri/src/session.rs` holds the only copy of the meeting state. Windows never own state: on open they call `snapshot`, then follow events.
- Audio, speech-to-text and language-model calls will all run on the Rust side. Audio stays out of the web views, and API keys never reach them.
- The script is context for both directions: it is sent with every translation request as a cached system block (names, terms and prepared answers come out the way the script has them).

## Providers (cloud first, local through the same doors)

Every option, with list prices and a description in one format (speed, strength, caution, needs), is defined once in `src-tauri/src/catalog.rs`. The settings switcher, the usage costs and these docs read from it.

| Job | Options | Wire format |
|---|---|---|
| Listening | Gemini 3.5 Transcribe Live, OpenAI gpt-live-transcribe | streaming WebSocket (`gemini.rs`, `openai_rt.rs`) |
| | OpenAI gpt-4o-mini-transcribe, local Whisper | one WAV per sentence to `/audio/transcriptions` (`stt_chunk.rs`, cut by `vad.rs`) |
| | Apple SpeechAnalyzer | planned (macOS) |
| Translation and replies | Claude Haiku 5.5, Sonnet 5.5 | Anthropic Messages API, streaming |
| | gpt-6-luna, gpt-6.1-sol, Gemini 3.5 Flash-Lite, Gemini 3.8 Flash, local vLLM / Ollama | Chat Completions, streaming (OpenAI, Gemini's OpenAI-compatible endpoint, local servers) |
| | Apple Foundation Models | planned (macOS) |

A local setup is a URL and a model name in settings; `docs/LOCAL.md` explains the servers. Each request uses the model's lowest thinking setting unless the user picks one (Claude `low`, OpenAI `none`, Gemini `minimal`), since a subtitle that arrives late is worth less.

Keys are the user's own (bring your own key), stored in the OS keychain and never sent back to a web view. Debug builds also read a git-ignored `.env.local` (see `.env.example`).

Usage (`usage.rs`) counts audio seconds sent to speech services and the token counts each language-model response reports, per model and per local day, and prices them from the catalog. These are estimates; each provider's billing page is the authority.

## Behaviors carried over from the prototype

The first version was a PyQt app used at a live conference demo. These held up and are kept:

- The caption translation prompt must never comment on recognition errors; its whole output is shown as a subtitle.
- Replies are short spoken sentences (about 15 words at most), keep the speaker's meaning and tone, and reuse the script's wording when what was typed matches a point in it.
- A glossary (`中文 = English` per line) applies in both directions and also primes speech recognition with names.
- Captions stay pinned to the bottom unless the reader scrolls up; only the reader's own scrolling changes that (`src/shared/follow.ts`). The prototype lost the bottom when wrapped lines finished laying out late.

## Windows and permissions

- Opening and closing the optional windows follows Basesmall's panel manager (`src/shared/windows.ts`): the created event can fire before its listener is attached, so the window is also looked up by label.
- Closing the captions window quits the app; the optional windows go with it.
- `Ctrl+Shift+Space` (global) opens and focuses the reply window while the meeting app has focus.
- Capabilities are per window type: `capabilities/main.json` may create windows and register the shortcut; `capabilities/optional.json` only draws and calls the app's own commands.

## Status

Checked against real services and hardware (2026-10-17):
- Listening end to end with real system audio (`scripts/e2e-listen.ps1`, two TTS sentences, translated by Claude Haiku 5.5): Gemini 3.5 Transcribe Live, OpenAI gpt-live-transcribe, OpenAI gpt-4o-mini-transcribe and local Whisper all transcribed both sentences word for word.
- Gemini Live's `interimInputTranscription.text` carries the whole hypothesis so far (each message replaces the gray line), confirmed from the raw message log. It also sends `voiceActivity` messages, which are ignored.
- OpenAI realtime accepts the transcription session at `?intent=transcription`.
- Translation of one sentence, 3-5 runs each (full sentence): Gemini 3.5 Flash-Lite 0.84-1.15 s, Claude Haiku 5.5 0.94-1.21 s, gpt-6-luna 0.91-1.95 s, gpt-6.1-sol 1.33-2.19 s, Gemini 3.8 Flash 4.2-7.1 s (too slow for live captions). gpt-6.1-sol rejects `reasoning_effort: none` and Gemini 3.8 Flash rejects `minimal`; the catalog uses `low` for both. Claude Sonnet 5.5 was not reachable from the test account (a workspace rate limit).
- Every cloud path reports a rejected key as a readable error, without retrying.
- Settings links open the three providers' key pages (and only those: `capabilities/settings-links.json`).

Not yet: translation quality beyond single sentences (a blind comparison is planned), local vLLM / Ollama, Apple on-device options and macOS system audio (ScreenCaptureKit), the updater, release workflow.

## Checking a build

- `npm test` runs the script-format tests; `cargo test --lib` (in `src-tauri`) runs the Rust unit tests.
- Hardware and network checks are `#[ignore]`d: `cargo test --lib -- --ignored --nocapture loopback` (needs a speaker as default output), `_bad` (rejected keys on every cloud path), `live_models` (translates with every model whose key is in `.env.local`, with timings and cost), `gemini_real`, `openai_rt_real`.
- `scripts/e2e-listen.ps1` runs the whole listening chain against a settings folder of your choice.
- `npm run typecheck`.
- `npx tauri build --debug --no-bundle`, then `scripts/smoke-windows.ps1 -Exe src-tauri\target\debug\ownvoice-meet.exe -Out <folder>` launches the app in Demo mode, clicks through the windows with UI Automation and saves screenshots.
