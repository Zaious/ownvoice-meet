// Settings (a JSON file in the app config folder) and API keys (the OS keychain: Windows
// Credential Manager, macOS Keychain). Keys never go back to a web view; windows only learn
// whether one is set. For development, ANTHROPIC_API_KEY / OPENAI_API_KEY / GEMINI_API_KEY (from
// the environment, or .env.local in debug builds) are used when the keychain has none.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SETTINGS_MAX_BYTES: u64 = 256 * 1024;
const KEYCHAIN_SERVICE: &str = "ownvoice-meet";
/// Keys the app can hold. "local" is optional (a vLLM server started with --api-key needs it).
pub const PROVIDERS: [&str; 4] = ["anthropic", "openai", "gemini", "local"];

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Catalog id of the speech-to-text option (catalog.rs).
    pub stt_id: String,
    /// BCP-47 code such as en-US; empty lets the model detect the language.
    pub stt_language: String,
    /// Catalog id of the translation model.
    pub llm_id: String,
    /// "auto" = the lowest thinking setting the model accepts (fastest); or low / medium / high.
    pub llm_effort: String,
    pub context_lines: usize,
    /// `中文 = English` per line.
    pub glossary: String,
    /// OpenAI-compatible transcription server, e.g. a self-hosted faster-whisper server.
    pub local_stt_url: String,
    pub local_stt_model: String,
    /// OpenAI-compatible chat server, e.g. vLLM or Ollama.
    pub local_llm_url: String,
    pub local_llm_model: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            stt_id: "gemini-live".into(),
            stt_language: "en-US".into(),
            llm_id: "claude-haiku-5-5".into(),
            llm_effort: "auto".into(),
            context_lines: 12,
            glossary: String::new(),
            local_stt_url: "http://localhost:8000/v1".into(),
            local_stt_model: "Systran/faster-whisper-large-v3".into(),
            local_llm_url: "http://localhost:11434/v1".into(),
            local_llm_model: String::new(),
        }
    }
}

pub fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("OWNVOICE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    app.path().app_config_dir().map_err(|e| e.to_string())
}

pub fn load(app: &AppHandle) -> Settings {
    let Ok(path) = config_dir(app).map(|d| d.join("settings.json")) else { return Settings::default() };
    if std::fs::metadata(&path).map(|m| m.len() > SETTINGS_MAX_BYTES).unwrap_or(true) {
        return Settings::default();
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default()
}

/// Atomic save: write a temporary file, then rename it over the old one.
pub fn save(app: &AppHandle, s: &Settings) -> Result<(), String> {
    let dir = config_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    let tmp = dir.join("settings.json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join("settings.json")).map_err(|e| e.to_string())
}

fn entry(provider: &str) -> Result<keyring::Entry, String> {
    if !PROVIDERS.contains(&provider) {
        return Err(format!("unknown provider {provider}"));
    }
    keyring::Entry::new(KEYCHAIN_SERVICE, provider).map_err(|e| e.to_string())
}

/// How a provider is named to the user.
pub fn display_name(provider: &str) -> &str {
    match provider {
        "anthropic" => "Anthropic",
        "openai" => "OpenAI",
        "gemini" => "Gemini",
        "local" => "本機伺服器",
        other => other,
    }
}

/// Store a key; an empty key removes it.
pub fn set_key(provider: &str, key: &str) -> Result<(), String> {
    let e = entry(provider)?;
    let key = key.trim();
    if key.is_empty() {
        return match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        };
    }
    e.set_password(key).map_err(|e| e.to_string())
}

pub fn get_key(provider: &str) -> Option<String> {
    let from_keychain = entry(provider).ok().and_then(|e| e.get_password().ok()).filter(|k| !k.is_empty());
    from_keychain.or_else(|| {
        let var = match provider {
            "gemini" => "GEMINI_API_KEY",
            "openai" => "OPENAI_API_KEY",
            "local" => "LOCAL_API_KEY",
            _ => "ANTHROPIC_API_KEY",
        };
        std::env::var(var).ok().filter(|k| !k.is_empty())
    })
}
