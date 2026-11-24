# 安全政策

**繁體中文** · [English](#security-policy)

## 支援的版本

只有[最新版](https://github.com/Zaious/ownvoice-meet/releases/latest)會收到安全修正，修正以新版本發布。免安裝版不會自己更新：有新版時，從最新版下載頁下載新的 zip。

## 回報漏洞

請用 GitHub 的私密回報：到這個 repo 的 **Security** 分頁，按 **Report a vulnerability**。這需要一個 GitHub 帳號。請不要開公開的 issue。

我是唯一的維護者，不承諾回覆或修正的時間。

## 範圍

算：

- 更新的簽章驗證與更新路徑（安裝版與 Mac 版的程式內更新）。
- 你在程式裡輸入的 API key，以及 ChatGPT 登入的憑證怎麼保存。
- ChatGPT 登入時，在本機 `127.0.0.1` 開的暫時連接埠。

不算：

- 第三方服務（語音辨識、翻譯模型、GitHub）本身的錯誤或中斷。
- Windows 對沒有程式碼簽章的程式顯示的 SmartScreen 警告（README 的「下載」已說明）。

---

# Security policy

[繁體中文](#安全政策) · **English**

## Supported versions

Only the [latest release](https://github.com/Zaious/ownvoice-meet/releases/latest) gets security fixes, and a fix comes as a new release. The portable zip does not update itself; download the new one from the latest release.

## Reporting a vulnerability

Use GitHub's private reporting: the **Security** tab of this repository, then **Report a vulnerability**. It needs a GitHub account. Please do not open a public issue.

I am the only maintainer and do not promise a time for a reply or a fix.

## In scope

- Update signature checking and the update path (in-app updates of the installer and the Mac version).
- Storage of the API keys you enter in the app and of the ChatGPT sign-in.
- The temporary port on `127.0.0.1` opened while you sign in with ChatGPT.

## Out of scope

- Errors or outages of third-party services (speech recognition, language models, GitHub).
- The Windows SmartScreen warning for an app that is not code-signed (explained under Download in the README).
