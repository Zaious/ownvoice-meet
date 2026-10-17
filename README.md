# OwnVoice Meet

**Understand the meeting. Answer in your own voice.**

For people who join meetings, talks and Q&A in English but read it better than they speak it. Type what you mean in your own language; OwnVoice Meet turns it into short, natural English that you read aloud. The words are yours, and so is the voice.

> Status: 0.1.0, Windows. Working with real services. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## What it does

Three windows; use the ones you need and put each where it helps.

| Window | What you get |
|---|---|
| **Reply** | Type in Chinese, press Enter, read the English aloud in large type. It knows what was just said in the meeting and what is in your script, so it uses the right names and terms. `Ctrl+Shift+Space` brings it up while Zoom or Teams has focus. Put it next to your webcam. |
| **Captions** | What others say, in English as they speak, with the meaning in Chinese under each sentence. |
| **Script & notes** | Your talk, prepared answers and notes, each line with its meaning, in tabs, with copy buttons for links and commands. |

## What it does not do

It does not answer for you. There are no AI-written replies and no synthetic voice: you decide what to say, and you say it. OwnVoice Meet helps with the English.

## Providers and cost

Bring your own keys; Settings links to each provider's key page, and keys are kept in the system keychain. Pick one option for listening and one for translation:

| Listening | Translation and replies |
|---|---|
| Gemini 3.5 Transcribe Live (live, about $0.54/hour) | Claude Haiku 5.5, Sonnet 5.5 |
| OpenAI gpt-live-transcribe (live, about $1.02/hour) | OpenAI gpt-6-luna, gpt-6.1-sol |
| OpenAI gpt-4o-mini-transcribe (per sentence, about $0.18/hour) | Gemini 3.5 Flash-Lite, 3.8 Flash |
| A Whisper server on your own machine (free) | vLLM or Ollama on your own machine (free) |

Translation costs a few cents an hour on the fastest models (for example Claude Haiku 5.5, about $0.03 for a busy hour). Settings shows every option with its price and measured speed, and tracks estimated usage. Prices are list prices; your provider's bill is the authority.

Running everything locally is a matter of URLs in Settings: see [docs/LOCAL.md](docs/LOCAL.md). Apple on-device options (macOS 26) are planned.

## Privacy

Meeting audio goes to the speech service you choose, and text to the language model you choose, under your own account. Some meetings are confidential, and some places require everyone's consent before a conversation is transcribed: check before you turn it on. Use paid-tier keys where the provider uses free-tier data for training (Gemini does). Local servers keep everything on your machine.

## Download

Windows 10 and 11, from [Releases](https://github.com/Zaious/ownvoice-meet/releases):

- **Installer** (`OwnVoice.Meet_<version>_x64-setup.exe`): installs for your user only, no administrator rights. Settings > Check for updates finds a newer version, and Update installs it after checking its signature.
- **Portable** (`OwnVoice.Meet_<version>_x64_portable.zip`): unzip anywhere and run `OwnVoice Meet.exe`. It never updates itself; Check for updates links to the download page.

The builds are not code-signed yet, so Windows may say "Windows protected your PC": choose **More info**, then **Run anyway**. Both need the Microsoft Edge WebView2 Runtime, which Windows 11 includes. OwnVoice Meet contacts GitHub only when you press Check for updates.

## Building from source

Requirements: Node.js 20+, Rust (stable), and on Windows the WebView2 runtime (included in Windows 11).

```
npm install
npx tauri dev
```

Press **Demo** to see captions without audio or keys. For development, copy `env.example` to `.env.local` (git-ignored) and fill in keys; debug builds read it.

Checks: `npm test`, `npm run typecheck`, and `cargo test --lib` in `src-tauri`; CI runs them on every push. Tests that call real services or need speakers are marked ignored; [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) lists them.

## 中文說明

**聽得懂會議，用自己的聲音回答。**

給英文「看得懂、說不太出口」的人：想說什麼就用中文打，OwnVoice Meet 把它變成好唸的口語英文短句，你照著唸。話是你自己的，聲音也是你自己的。旁邊有對方發言的英文字幕和中文意思，還能開一份中英對照的講稿與 Q&A 備忘。

它不會替你作答：沒有 AI 代寫的回答，也沒有合成語音。

下載（Windows 10、11）：到 [Releases](https://github.com/Zaious/ownvoice-meet/releases) 選安裝版（`…_x64-setup.exe`，只裝給目前使用者、不需系統管理員權限，可以在設定裡「檢查更新」後自動更新）或免安裝版（`…_x64_portable.zip`，解壓縮就能用，不會自己更新）。程式還沒有數位簽章，Windows 若顯示「Windows 已保護您的電腦」，按「其他資訊」再按「仍要執行」。

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). A work based on OwnVoice Meet must carry the NOTICE; the name "OwnVoice Meet" is not licensed for other works.
