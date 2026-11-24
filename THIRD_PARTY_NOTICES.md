# Third-party notices

OwnVoice Meet contains logic ported from the projects below, and uses the speech models listed.
Ported code is not copied: it was read and reimplemented in Rust, with the changes listed. Other
dependencies pulled in by Cargo and npm carry their own licences and are not listed here.

## Local translation: Qwen3 1.7B with llama.cpp

- llama.cpp (https://github.com/ggml-org/llama.cpp), MIT License: built from source into the app through the `llama-cpp-2` crate (MIT or Apache-2.0).
- Qwen3 1.7B by the Qwen team, Alibaba Cloud (https://huggingface.co/Qwen/Qwen3-1.7B), Apache License 2.0; the Q4_K_M GGUF quantization by Unsloth (https://huggingface.co/unsloth/Qwen3-1.7B-GGUF). Downloaded on first use, not shipped with the app.

## Local speech recognition: two models

Downloaded on first use, not shipped with the app, run with sherpa-onnx (below):
- Parakeet TDT 0.6B v3 by NVIDIA (https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3), Creative Commons Attribution 4.0 (https://creativecommons.org/licenses/by/4.0/); the int8 ONNX export by the sherpa-onnx project. NVIDIA does not endorse OwnVoice Meet.
- SenseVoice Small by Alibaba's FunAudioLLM team (https://github.com/FunAudioLLM/SenseVoice), FunASR Model Open Source License Agreement 1.1 (https://github.com/modelscope/FunASR/blob/main/MODEL_LICENSE); the int8 ONNX export of 2024-07-17 by the sherpa-onnx project.

## Speaker labels: sherpa-onnx and two models

- sherpa-onnx (https://github.com/k2-fsa/sherpa-onnx), by the Next-gen Kaldi team, Apache License 2.0: linked into the app through its Rust crate, with ONNX Runtime (Microsoft, MIT License) inside it.
- Downloaded on first use, not shipped with the app, from sherpa-onnx's releases:
  - Silero VAD v5 (https://github.com/snakers4/silero-vad), MIT License: finds where speech starts and ends.
  - CAM++ speaker embedding, trained on VoxCeleb, from 3D-Speaker (https://github.com/modelscope/3D-Speaker), Apache License 2.0: the voice prints.

## LiveCaptions-Translator

- Source: https://github.com/SakiRinn/LiveCaptions-Translator (read at commit `a6fee12757b15edbeef7c60f8895e0be694801e1`)
- Author: SakiRinn and contributors
- Licence: Apache License 2.0 (https://www.apache.org/licenses/LICENSE-2.0)

What was ported, and where it lives here:

| Their file | Ideas taken | Here |
|---|---|---|
| `src/utils/LiveCaptionsHandler.cs` | Start `LiveCaptions.exe`, find the window of class `LiveCaptionsDesktopWindow`, read the `Name` of the UI Automation element with AutomationId `CaptionsTextBlock`, move the window on screen if it is off screen or tiny, hide it (minimize + `WS_EX_TOOLWINDOW`), start it again when the element is no longer available | `src-tauri/src/live_captions.rs` |
| `src/Translator.cs` (`SyncLoop`) | Poll every 25 ms; sentences end in `. ? ! 。 ？ ！`; a sentence under 10 bytes is merged; a sentence counts as finished after the text has been unchanged for a while (their 50 polls) | `src-tauri/src/sentences.rs` |
| `src/utils/RegexPatterns.cs`, `src/utils/TextUtil.cs` | The acronym, punctuation-spacing and CJK-punctuation clean-ups, `ReplaceNewlines`, `isCJChar` | `src-tauri/src/sentences.rs` |
| `src/models/TranslationTaskQueue.cs` | A translation that finishes cancels the older ones still running, and older results never replace newer ones | `src-tauri/src/captions.rs` (early translation of long sentences) |

Changes from the original:

- A point or comma between digits (`3.5`, `1,200`) is not a sentence end or a place to add a space.
- The acronym rules apply only after a single capital letter, so `OK. So` and `AWS. Then` keep their periods.
- A very short sentence joins the next sentence instead of the one before it.
- Sentences are cut where the words have settled: the place reached is found again by words (ignoring case and punctuation), and a sentence end counts only once it has stayed put, because current Live Captions adds a provisional period after the word being recognised and moves or removes periods for a few seconds. Text shown before listening started is skipped.
- Newer Live Captions also exposes the text as the Name of `CaptionsScrollViewer`; both ids are read.
- A Live Captions window the user already had open is reused and given back when listening stops, instead of every running Live Captions being killed.
- Early translations are throttled by time (after 4 s of a growing sentence, at most every 2 s) rather than by the number of text changes.
