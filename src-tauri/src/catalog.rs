// Every speech-to-text and translation option the app offers, with list prices and a description
// in one fixed format (speed, strength, caution, needs). The settings switcher, the usage costs and
// the docs all read from here, so a price changes in one place.
//
// Prices are USD list prices, checked 2026-10-17 on the providers' pricing pages
// (developers.openai.com/api/docs/pricing, ai.google.dev/gemini-api/docs/pricing, Anthropic's
// model table). Local and on-device options cost nothing to the app.

use serde::Serialize;

#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Stt,
    Llm,
}

#[derive(Clone, Copy, Serialize)]
#[serde(tag = "unit", rename_all = "snake_case")]
pub enum Price {
    /// Speech-to-text billed per minute of audio.
    PerMinute { usd: f64 },
    /// Language models billed per million tokens.
    PerMTok { input: f64, cached: f64, output: f64 },
    Free,
}

#[derive(Clone, Copy, Serialize)]
pub struct Model {
    /// Stable id stored in settings.
    pub id: &'static str,
    pub kind: Kind,
    /// Whose key it needs: anthropic, openai, gemini; or local / apple / windows (none).
    pub provider: &'static str,
    /// Model name sent to the API; empty for local models (taken from settings).
    pub api_model: &'static str,
    pub label: &'static str,
    /// How it is called. Speech: gemini-live, openai-live, chunked, windows-live-captions.
    /// Language: anthropic, openai-compat.
    pub transport: &'static str,
    /// Lowest thinking setting the model accepts, sent with every request ("" = leave default).
    pub min_effort: &'static str,
    pub price: Price,
    pub price_note: &'static str,
    pub speed: &'static str,
    pub strength: &'static str,
    pub caution: &'static str,
    pub needs: &'static str,
    /// False for options that are planned but not built yet.
    pub available: bool,
}

pub const MODELS: &[Model] = &[
    // ---------- listening (speech-to-text) ----------
    Model {
        id: "gemini-live",
        kind: Kind::Stt,
        provider: "gemini",
        api_model: "gemini-3.5-transcribe-live",
        label: "Gemini 3.5 Transcribe Live",
        transport: "gemini-live",
        min_effort: "",
        price: Price::PerMinute { usd: 0.009 },
        price_note: "官方估算：音訊輸入 $3.50 + 文字輸出 $21 / 百萬 token",
        speed: "即時串流：說話中就有灰字",
        strength: "主流服務中最便宜的即時串流",
        caution: "免費層資料會被 Google 用來改進產品，請用付費層 key",
        needs: "Gemini API key",
        available: true,
    },
    Model {
        id: "openai-live",
        kind: Kind::Stt,
        provider: "openai",
        api_model: "gpt-live-transcribe",
        label: "OpenAI gpt-live-transcribe",
        transport: "openai-live",
        min_effort: "",
        price: Price::PerMinute { usd: 0.017 },
        price_note: "官方價目",
        speed: "即時串流：說話中就有灰字",
        strength: "OpenAI 的即時轉錄",
        caution: "價格約 Gemini Live 的 2 倍",
        needs: "OpenAI API key",
        available: true,
    },
    Model {
        id: "openai-mini",
        kind: Kind::Stt,
        provider: "openai",
        api_model: "gpt-4o-mini-transcribe",
        label: "OpenAI gpt-4o-mini-transcribe",
        transport: "chunked",
        min_effort: "",
        price: Price::PerMinute { usd: 0.003 },
        price_note: "按 token 計（$1.25 / $5 每百萬），官方估約 $0.003 / 分",
        speed: "切句上傳：一句講完才出字，沒有灰字",
        strength: "雲端最便宜",
        caution: "長句要等停頓（或 12 秒上限）才出字",
        needs: "OpenAI API key",
        available: true,
    },
    Model {
        id: "windows-live-captions",
        kind: Kind::Stt,
        provider: "windows",
        api_model: "",
        label: "Windows 即時輔助字幕（Live Captions）",
        transport: "windows-live-captions",
        min_effort: "",
        price: Price::Free,
        price_note: "Windows 內建，在電腦上辨識",
        speed: "即時：說話中就有灰字",
        strength: "免費、不用 key，音訊不離開電腦",
        caution: "辨識語言在即時輔助字幕自己的 ⚙ ›「變更語言」裡選，要裝好會議語言的語音檔（語言不符時聆聽狀態會提醒）",
        needs: "Windows 11 22H2 以上（其他系統不會列出）",
        available: true,
    },
    Model {
        id: "local-whisper",
        kind: Kind::Stt,
        provider: "local",
        api_model: "",
        label: "本機 Whisper（OpenAI 相容伺服器）",
        transport: "chunked",
        min_effort: "",
        price: Price::Free,
        price_note: "自己的硬體",
        speed: "切句上傳；速度看你的硬體",
        strength: "音訊不離開你的電腦",
        caution: "要自己架伺服器，見 docs/LOCAL.md",
        needs: "伺服器網址與模型名稱",
        available: true,
    },
    Model {
        id: "apple-speech",
        kind: Kind::Stt,
        provider: "apple",
        api_model: "",
        label: "Apple 裝置上辨識（SpeechAnalyzer）",
        transport: "apple",
        min_effort: "",
        price: Price::Free,
        price_note: "裝置上執行",
        speed: "裝置上即時辨識",
        strength: "免費、音訊不離開裝置",
        caution: "規劃中；限 macOS 26 + Apple Silicon",
        needs: "Mac",
        available: false,
    },
    // ---------- translation and replies (language models) ----------
    Model {
        id: "claude-haiku-5-5",
        kind: Kind::Llm,
        provider: "anthropic",
        api_model: "claude-haiku-5-5",
        label: "Claude Haiku 5.5",
        transport: "anthropic",
        min_effort: "low",
        price: Price::PerMTok { input: 0.10, cached: 0.01, output: 0.50 },
        price_note: "快取讀取按輸入價一成估算（Anthropic 未另列）",
        speed: "最快一級：實測整句 0.9–1.2 秒",
        strength: "最便宜；已在實際會議用過",
        caution: "—",
        needs: "Anthropic API key",
        available: true,
    },
    Model {
        id: "claude-sonnet-5-5",
        kind: Kind::Llm,
        provider: "anthropic",
        api_model: "claude-sonnet-5-5",
        label: "Claude Sonnet 5.5",
        transport: "anthropic",
        min_effort: "low",
        price: Price::PerMTok { input: 2.00, cached: 0.20, output: 10.00 },
        price_note: "官方價目",
        speed: "中階，比 Haiku 慢",
        strength: "措辭與判斷較強",
        caution: "約 Haiku 的 20 倍價格；即時字幕通常用不到",
        needs: "Anthropic API key",
        available: true,
    },
    Model {
        id: "gpt-6-luna",
        kind: Kind::Llm,
        provider: "openai",
        api_model: "gpt-6-luna",
        label: "OpenAI gpt-6-luna",
        transport: "openai-compat",
        min_effort: "none",
        price: Price::PerMTok { input: 0.10, cached: 0.01, output: 0.50 },
        price_note: "官方價目",
        speed: "最快一級：實測整句 0.9–1.9 秒",
        strength: "與 Haiku 同價",
        caution: "與 Haiku 同價；實測速度波動較大",
        needs: "OpenAI API key",
        available: true,
    },
    Model {
        id: "gpt-6.1-sol",
        kind: Kind::Llm,
        provider: "openai",
        api_model: "gpt-6.1-sol",
        label: "OpenAI gpt-6.1-sol",
        transport: "openai-compat",
        min_effort: "low", // rejects "none" (checked against the API 2026-10-17)
        price: Price::PerMTok { input: 2.00, cached: 0.10, output: 10.00 },
        price_note: "官方價目",
        speed: "中階：實測整句 1.3–2.2 秒",
        strength: "OpenAI 中階主力",
        caution: "約 Luna 的 20 倍價格；即時字幕通常用不到",
        needs: "OpenAI API key",
        available: true,
    },
    Model {
        id: "gemini-3.5-flash-lite",
        kind: Kind::Llm,
        provider: "gemini",
        api_model: "gemini-3.5-flash-lite",
        label: "Gemini 3.5 Flash-Lite",
        transport: "openai-compat",
        min_effort: "minimal",
        price: Price::PerMTok { input: 0.30, cached: 0.03, output: 2.50 },
        price_note: "官方價目（輸出含思考 token）",
        speed: "最快一級：實測整句 0.8–1.1 秒",
        strength: "Google 最便宜的一級",
        caution: "約 Haiku 的 3–5 倍價格；免費層資料會被用來改進產品",
        needs: "Gemini API key",
        available: true,
    },
    Model {
        id: "gemini-3.8-flash",
        kind: Kind::Llm,
        provider: "gemini",
        api_model: "gemini-3.8-flash",
        label: "Gemini 3.8 Flash",
        transport: "openai-compat",
        min_effort: "low", // rejects "minimal" (checked against the API 2026-10-17)
        price: Price::PerMTok { input: 0.75, cached: 0.075, output: 3.75 },
        price_note: "優惠價至 2026-12-31，之後 $1.50 / $0.15 / $7.50",
        speed: "慢：實測整句 4.2–7.1 秒（思考無法關閉）",
        strength: "Google 最新的 Flash",
        caution: "對即時字幕太慢；明年起價格加倍",
        needs: "Gemini API key",
        available: true,
    },
    Model {
        id: "local-llm",
        kind: Kind::Llm,
        provider: "local",
        api_model: "",
        label: "本機模型（vLLM / Ollama，OpenAI 相容）",
        transport: "openai-compat",
        min_effort: "",
        price: Price::Free,
        price_note: "自己的硬體",
        speed: "看你的硬體與模型",
        strength: "文字不離開你的電腦",
        caution: "小模型的繁中與台灣用語品質落差大，請先試",
        needs: "伺服器網址與模型名稱",
        available: true,
    },
    Model {
        id: "apple-foundation",
        kind: Kind::Llm,
        provider: "apple",
        api_model: "",
        label: "Apple Foundation Models（裝置上）",
        transport: "apple",
        min_effort: "",
        price: Price::Free,
        price_note: "裝置上執行",
        speed: "裝置上執行",
        strength: "免費、文字不離開裝置",
        caution: "規劃中；限 macOS 26 + Apple Silicon",
        needs: "Mac",
        available: false,
    },
];

pub fn find(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.id == id)
}

/// Estimated USD for a usage record.
pub fn cost(m: &Model, audio_secs: f64, input: u64, cached: u64, output: u64) -> f64 {
    match m.price {
        Price::PerMinute { usd } => usd * audio_secs / 60.0,
        Price::PerMTok { input: pi, cached: pc, output: po } => {
            (input as f64 * pi + cached as f64 * pc + output as f64 * po) / 1_000_000.0
        }
        Price::Free => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_every_kind_has_an_available_default() {
        let mut ids: Vec<_> = MODELS.iter().map(|m| m.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), MODELS.len());
        assert!(find("gemini-live").is_some_and(|m| m.available && m.kind == Kind::Stt));
        assert!(find("claude-haiku-5-5").is_some_and(|m| m.available && m.kind == Kind::Llm));
    }

    #[test]
    fn costs() {
        let gem = find("gemini-live").unwrap();
        assert!((cost(gem, 3600.0, 0, 0, 0) - 0.54).abs() < 1e-9);
        let haiku = find("claude-haiku-5-5").unwrap();
        // 180k input + 30k output ≈ $0.033, the estimate shown to the user
        assert!((cost(haiku, 0.0, 180_000, 0, 30_000) - 0.033).abs() < 1e-9);
    }
}
