# eztrans

A live interpreter for online meetings (Zoom, Teams, Meet, or anything that plays through your speakers):

- **Captions**: what others say, in English, with the Chinese meaning under each line.
- **Reply** (optional): type what you want to say in Chinese, read the English aloud.
- **Script** (optional): your talk, prepared answers and notes, side by side with their meaning.

> Status: 0.1.0 skeleton. The windows, Demo mode and the script panel work; audio capture, speech-to-text and the language model are being connected. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Privacy

eztrans sends meeting audio to the speech-to-text service you choose, and caption and reply text to the language model you choose, using your own keys. Some meetings are confidential, and some places require everyone's consent before a conversation is transcribed. Check before you turn it on. You can point both at servers you run yourself.

## Building from source

Requirements: Node.js 20+, Rust (stable), and on Windows the WebView2 runtime (included in Windows 11).

```
npm install
npx tauri dev
```

Press **Demo** to see captions without audio or keys. Open a script with the **講稿** window; the format is shown in [examples/scripts/demo.md](examples/scripts/demo.md).

Checks: `npm test`, `npm run typecheck`.

## 中文說明

eztrans 是線上會議的即時口譯小工具：上面看對方說的英文和中文意思，想發言時打中文、照著唸英文，旁邊還能開講稿與備忘。雲端為主，使用你自己的 API key；也可以改接自己架的伺服器。

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). A work based on eztrans must carry the NOTICE; the name "eztrans" is not licensed for other works.
