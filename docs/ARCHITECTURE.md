# Architecture

eztrans helps someone take part in an online meeting held in a language they read better than they speak. It does three jobs, each in its own window:

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

| Job | Default | Local option |
|---|---|---|
| Speech-to-text | A streaming cloud service with interim results | Any server that speaks the OpenAI transcription API (for example a self-hosted faster-whisper server) |
| Translation and replies | Anthropic Claude, `claude-haiku-5-5` at low effort | Any OpenAI-compatible endpoint (vLLM, Ollama) |

There is no local code path: a local setup is a URL in settings. `docs/LOCAL.md` (to come) explains how to run those servers.

Keys are the user's own (bring your own key) and will be stored in the OS keychain, never in a plain file.

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

## Status (0.1.0 skeleton)

Working: the three windows, the shared session and its events, the script format and its window, the reply round trip (with a placeholder answer), Demo mode, the global shortcut.

Not yet connected: audio capture, speech-to-text, the language model, settings and the keychain, `docs/LOCAL.md`, the updater, release workflow. macOS needs ScreenCaptureKit (or a virtual audio device) for system audio and comes after Windows.

## Checking a build

- `npm test` runs the script-format tests.
- `npm run typecheck`.
- `npx tauri build --debug --no-bundle`, then `scripts/smoke-windows.ps1 -Exe src-tauri\target\debug\eztrans.exe -Out <folder>` launches the app in Demo mode, clicks through the windows with UI Automation and saves screenshots.
