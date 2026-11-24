# 參與 OwnVoice Meet

**繁體中文** · [English](#contributing-to-ownvoice-meet)

Issue 與 PR 用中文或英文都可以。開 issue 需要一個免費的 GitHub 帳號。

## 歡迎什麼

- 錯誤回報：寫出版本、作業系統、「聽」和「翻」各選了哪個服務，以及怎麼重現。
- 修正錯誤、改善字幕與翻譯的流程、新增語音辨識或翻譯服務、修正文件。

改動比較大時（新的服務、新的視窗、改設定檔格式），請先開 issue 說明想改什麼，再動手。

## 不收什麼

- **替使用者作答的功能**，例如 AI 代寫的回答，或用合成語音替你說話。OwnVoice Meet 的前提是說什麼由你決定、由你自己說出口（見 README 的「[它不做什麼](README.md#它不做什麼)」）。
- **使用分析（analytics），或 README「[隱私](README.md#隱私)」沒有列出的連線。** README 列出程式會連到的每一個主機；需要新的連線時，PR 要同時更新 `README.md` 與 `README.en.md` 的隱私一節，並說明為什麼需要它。

## PR 前要過的檢查

需要的 Node.js 與 Rust 版本、怎麼啟動，見 README 的「[從原始碼建置](README.md#從原始碼建置)」。送 PR 前，在本機跑過這些：

```bash
npm ci
npm run typecheck
npm test
```

```bash
cd src-tauri
cargo test --lib
```

CI 在每次 push 和每個 PR 都會跑這些檢查，並在 Windows 與 macOS 上建置（見 [ci.yml](.github/workflows/ci.yml)）。需要真實服務或喇叭的測試標為 ignored，不在 CI 裡跑，清單在 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)；改到這些部分時，請在 PR 說明裡寫你怎麼手動測過。

- 使用者看得到的改動，在 [CHANGELOG.md](CHANGELOG.md) 的 `Unreleased` 加一句，寫給使用者看。
- 介面字串用繁體中文（台灣用語）。

## AI 輔助的貢獻

- 可以用。實質用 AI 產生內容（程式、文件、翻譯）時，在 PR 說明裡寫出來；commit 可以加 `Assisted-by: {工具}` 標記。
- 你要能解釋每一行，並為它負責。
- 依 Apache License 2.0 第 5 條，你送出的貢獻依同一授權提供；其中 AI 輔助產生的部分，權利由你負責確認。
- 不需要簽 CLA。若你在 commit 加 `Signed-off-by`，必須是你本人加的，不得讓 AI 工具代簽或自動加上。

---

# Contributing to OwnVoice Meet

[繁體中文](#參與-ownvoice-meet) · **English**

Issues and pull requests are welcome in Chinese or English. Opening an issue needs a free GitHub account.

## Welcome

- Bug reports: say which version and operating system, which listening and translation services you picked, and how to reproduce the problem.
- Bug fixes, improvements to the captions and translation flow, new speech or translation services, corrections to the documentation.

For a larger change (a new service, a new window, a change to the settings file format), open an issue first to say what you want to change.

## Not accepted

- **Features that answer for the user**, such as AI-written replies or a synthetic voice speaking for you. OwnVoice Meet rests on you deciding what to say and saying it yourself (see [What it does not do](README.en.md#what-it-does-not-do) in the README).
- **Analytics, or any connection not listed under [Privacy](README.en.md#privacy) in the README.** The README lists every host the app connects to; a pull request that needs a new connection updates the Privacy section of both `README.md` and `README.en.md` and says why the connection is needed.

## Checks before a pull request

The Node.js and Rust versions and how to start the app are in [Build from source](README.en.md#build-from-source). Before sending a pull request, run these locally:

```bash
npm ci
npm run typecheck
npm test
```

```bash
cd src-tauri
cargo test --lib
```

CI runs these checks on every push and every pull request, and builds the app on Windows and macOS (see [ci.yml](.github/workflows/ci.yml)). Tests that need real services or speakers are marked ignored and do not run in CI; [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) lists them. If you change those parts, say in the pull request how you tested them by hand.

- For a change users will notice, add a line under `Unreleased` in [CHANGELOG.md](CHANGELOG.md) (in Traditional Chinese), written for users.
- Interface text is in Traditional Chinese (as used in Taiwan).

## AI-assisted contributions

- They are welcome. If AI produced a substantial part of the content (code, documentation, translation), say so in the pull request; a commit can carry an `Assisted-by: {tool}` trailer.
- You must be able to explain every line, and you answer for it.
- Under section 5 of the Apache License 2.0, what you submit is offered under the same licence; for anything AI helped produce, making sure you have the rights to it is on you.
- No CLA is needed. If you add `Signed-off-by` to a commit, you add it yourself; an AI tool must not sign for you or add it automatically.
