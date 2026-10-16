// Settings (a JSON file in the app config folder) and API keys (the OS keychain: Windows
// Credential Manager, macOS Keychain). Keys never go back to a web view; windows only learn
// whether one is set. For development, GEMINI_API_KEY / ANTHROPIC_API_KEY are used when the
// keychain has none.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SETTINGS_MAX_BYTES: u64 = 256 * 1024;
const KEYCHAIN_SERVICE: &str = "eztrans";
pub const PROVIDERS: [&str; 2] = ["gemini", "anthropic"];

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub stt_model: String,
    /// BCP-47 code such as en-US; empty lets the model detect the language.
    pub stt_language: String,
    pub llm_model: String,
    pub llm_effort: String,
    pub context_lines: usize,
    /// `中文 = English` per line.
    pub glossary: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            stt_model: "gemini-3.5-transcribe-live".into(),
            stt_language: "en-US".into(),
            llm_model: "claude-haiku-5-5".into(),
            llm_effort: "low".into(),
            context_lines: 12,
            glossary: String::new(),
        }
    }
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("EZTRANS_CONFIG_DIR").filter(|d| !d.is_empty()) {
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
        let var = if provider == "gemini" { "GEMINI_API_KEY" } else { "ANTHROPIC_API_KEY" };
        std::env::var(var).ok().filter(|k| !k.is_empty())
    })
}
