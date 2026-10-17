# Third-party notices

OwnVoice Meet contains logic ported from the projects below. Their code is not copied: it was read
and reimplemented in Rust, with the changes listed. Dependencies pulled in by Cargo and npm carry
their own licences and are not listed here.

## LiveCaptions-Translator

- Source: https://github.com/SakiRinn/LiveCaptions-Translator (read at commit `a6fee12757b15edbeef7c60f8895e0be694801e1`)
- Author: SakiRinn and contributors
- Licence: Apache License 2.0 (https://www.apache.org/licenses/LICENSE-2.0)

What was ported, and where it lives here:

| Their file | Ideas taken | Here |
|---|---|---|
| `src/utils/LiveCaptionsHandler.cs` | Start `LiveCaptions.exe`, find the window of class `LiveCaptionsDesktopWindow`, read the `Name` of the UI Automation element with AutomationId `CaptionsTextBlock`, move the window on screen if it is off screen or tiny, hide it (minimize + `WS_EX_TOOLWINDOW`), start it again when the element is no longer available | `src-tauri/src/live_captions.rs` |
| `src/Translator.cs` (`SyncLoop`) | Poll every 25 ms; take the last sentence ending in `. ? ! 。 ？ ！`; merge a sentence under 10 bytes; translate a sentence when it ends or after it has been unchanged for 50 polls; treat a revised sentence as the same one when they are more than 60% similar | `src-tauri/src/sentences.rs` |
| `src/utils/RegexPatterns.cs`, `src/utils/TextUtil.cs` | The acronym, punctuation-spacing and CJK-punctuation clean-ups, `ReplaceNewlines`, `isCJChar`, `Similarity` | `src-tauri/src/sentences.rs` |
| `src/models/TranslationTaskQueue.cs` | A translation that finishes cancels the older ones still running, and older results never replace newer ones | `src-tauri/src/captions.rs` (early translation of long sentences) |

Changes from the original:

- A point or comma between digits (`3.5`, `1,200`) is not a sentence end or a place to add a space.
- The acronym rules apply only after a single capital letter, so `OK. So` and `AWS. Then` keep their periods.
- A very short sentence joins the next sentence instead of the one before it.
- The text already finished is found again by position, so each sentence becomes one caption line, and text shown before listening started is skipped.
- A Live Captions window the user already had open is reused and given back when listening stops, instead of every running Live Captions being killed.
- Early translations are throttled by time (after 4 s of a growing sentence, at most every 2 s) rather than by the number of text changes.
