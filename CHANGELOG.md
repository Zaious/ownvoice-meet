# 版本沿革

OwnVoice Meet 每一版改了什麼，寫給使用者看。格式參考 [Keep a Changelog](https://keepachangelog.com/zh-TW/1.1.0/)，版本號採[語意化版本](https://semver.org/lang/zh-TW/)。下載在 [GitHub Releases](https://github.com/Zaious/ownvoice-meet/releases)。

## [Unreleased]

- 本機辨識：「聽」可以選在電腦上辨識，免費、聲音不送出去、不需要 key。英文用 NVIDIA Parakeet，中文、日文、韓文用 SenseVoice，自動偵測時中英夾雜也聽得懂。第一次要下載約 910 MB 的模型；沒有灰字，每句講完約 0.5–1 秒出字。

- 標出講者：每句字幕前面標「講者 1、講者 2…」，點一下可以改成名字，逐字稿和摘要跟著改。在電腦上用聲紋分辨，不送出聲音、不花錢；第一次勾選會下載約 31 MB 的模型（設定 → 收音與存檔）。

## [0.4.0] - 2026-11-23

- 不用 API key 也能翻譯：設定的「翻」選「ChatGPT 方案（gpt-6-luna，不用 API key）」，按 Continue with ChatGPT 登入。字幕翻譯、回覆、即時摘要和講稿「補對照」改用你的 ChatGPT 方案額度（Go、Plus、Pro、Business），不另外計費。
- 完全不用 key 的組合：Windows 11 即時輔助字幕（聽）加 ChatGPT 方案（翻）。
- 這是 OpenAI 官方的 Sign in with ChatGPT，目前還在預覽；只能做文字，語音辨識不行。用量和 ChatGPT、Codex 共用方案額度，設定裡的「管理用量」可以查看或設上限。
- 實測每句翻好約 2.3 秒（中位數），比用 API key 慢約 1 秒。
- 「清除所有 key」也會登出 ChatGPT。

## [0.3.2] - 2026-11-23

- 不只英文：別人說日文、韓文、德文、法文、西班牙文、越南文、泰文、印尼文也聽得懂，字幕照樣翻成中文，即時摘要也照樣做（設定的「別人說的語言」）。
- 回覆視窗和講稿「補對照」可以改成你要說的語言（設定的「我說的語言」，預設英文）；術語表改成「中文 = 這個語言」。
- 兩個語言設定都是下拉選單；以前手動填的代碼會保留。

## [0.3.1] - 2026-11-22

- 設定改了就自動儲存，關掉視窗也不會掉；「儲存」按鈕固定在視窗底部。之前按鈕夾在設定頁中間，沒按就關掉會全部不見。

## [0.3.0] - 2026-11-22

- Mac 版（測試中）：Intel 和 Apple 晶片都能用，macOS 11 以上，有 Apple 簽章與公證；用麥克風收音，收不到電腦播放的聲音。
- 即時摘要：「摘要」視窗列出到目前為止的重點，每幾分鐘更新一次；開啟存檔時一併存成 `summary.md`。
- 字幕：Gemini 把好幾句接成一行時，句子之間會補回空格。

## [0.2.3] - 2026-11-22

- 啟動 20 秒後問 GitHub 一次有沒有新版，有就在設定按鈕上提醒（可以在設定關掉）。

## [0.2.2] - 2026-11-22

- 安靜 3 分鐘自動暫停辨識、20 分鐘自動結束，忘了關也不會一直扣錢；兩個時間都可以調。

## [0.2.1] - 2026-11-21

- 修正：字幕每一行只顯示一小截。

## [0.2.0] - 2026-11-21

- 研討會會場也能用：麥克風收音（可選麥克風、調增益、6 秒麥克風檢查）。
- 錄音與逐字稿存在電腦上（預設不存）；分段，名稱可以照議程；網路太差時可以只錄音、不辨識。

## [0.1.3] - 2026-10-17

- 中文翻譯一律是台灣繁體。
- 英文字幕不再黏字。
- 設定的「聽」「翻」可以收合。

## [0.1.2] - 2026-10-17

- 回覆視窗跟著 app 一起打開，不要的人可以關掉。
- 新增「清除」按鈕；API key 貼錯欄會提醒。

## [0.1.1] - 2026-10-17

- 第一個公開版本：字幕與中文翻譯、回覆提詞（打中文、唸英文）、中英對照講稿。

[Unreleased]: https://github.com/Zaious/ownvoice-meet/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/Zaious/ownvoice-meet/releases/tag/v0.4.0
[0.3.2]: https://github.com/Zaious/ownvoice-meet/releases/tag/v0.3.2
[0.3.1]: https://github.com/Zaious/ownvoice-meet/releases/tag/v0.3.1
[0.3.0]: https://github.com/Zaious/ownvoice-meet/releases/tag/v0.3.0
[0.2.3]: https://github.com/Zaious/ownvoice-meet/releases/tag/v0.2.3
[0.2.2]: https://github.com/Zaious/ownvoice-meet/releases/tag/v0.2.2
[0.2.1]: https://github.com/Zaious/ownvoice-meet/releases/tag/v0.2.1
[0.2.0]: https://github.com/Zaious/ownvoice-meet/releases/tag/v0.2.0
[0.1.3]: https://github.com/Zaious/ownvoice-meet/releases/tag/v0.1.3
[0.1.2]: https://github.com/Zaious/ownvoice-meet/releases/tag/v0.1.2
[0.1.1]: https://github.com/Zaious/ownvoice-meet/releases/tag/v0.1.1
