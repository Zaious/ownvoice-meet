// Settings (a JSON file in the app config folder) and API keys (the OS keychain: Windows
// Credential Manager, macOS Keychain). Keys never go back to a web view; windows only learn
// whether one is set. For development, ANTHROPIC_API_KEY / OPENAI_API_KEY / GEMINI_API_KEY (from
// the environment, or .env.local in debug builds) are used when the keychain has none.

use std::path::PathBuf;
use std::sync::Mutex;

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
    /// How the windows look. Changed live from the windows (set_view), not by the settings form.
    pub view: Views,
}

/// Text size and background opacity of one reading window.
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct View {
    /// The window's base text size in CSS pixels; everything it reads scales with it.
    pub font_px: u32,
    /// Background opacity in percent. Text stays opaque, and the floor keeps it readable.
    pub opacity: u32,
}

impl Default for View {
    fn default() -> Self {
        Self { font_px: 15, opacity: 100 }
    }
}

pub const FONT_PX: std::ops::RangeInclusive<u32> = 10..=48;
pub const OPACITY: std::ops::RangeInclusive<u32> = 40..=100;

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct Views {
    pub captions: View,
    pub reply: View,
    pub script: View,
    /// Leave the reply and script windows out of screen sharing and screenshots.
    pub hide_from_capture: bool,
    /// Captions show only the translation (English stays until its translation arrives).
    pub zh_only: bool,
}

impl Default for Views {
    fn default() -> Self {
        Self { captions: View::default(), reply: View::default(), script: View::default(), hide_from_capture: true, zh_only: false }
    }
}

/// One change from a window; fields left out stay as they are.
#[derive(Deserialize, Default, Debug)]
#[serde(default)]
pub struct ViewPatch {
    /// "captions", "reply" or "script", for font_px and opacity.
    pub window: Option<String>,
    pub font_px: Option<u32>,
    pub opacity: Option<u32>,
    pub hide_from_capture: Option<bool>,
    pub zh_only: Option<bool>,
}

impl Views {
    pub fn apply(&mut self, p: &ViewPatch) -> Result<(), String> {
        if p.font_px.is_some() || p.opacity.is_some() {
            let v = match p.window.as_deref() {
                Some("captions") => &mut self.captions,
                Some("reply") => &mut self.reply,
                Some("script") => &mut self.script,
                other => return Err(format!("unknown window {other:?}")),
            };
            if let Some(f) = p.font_px {
                v.font_px = f.clamp(*FONT_PX.start(), *FONT_PX.end());
            }
            if let Some(o) = p.opacity {
                v.opacity = o.clamp(*OPACITY.start(), *OPACITY.end());
            }
        }
        if let Some(h) = p.hide_from_capture {
            self.hide_from_capture = h;
        }
        if let Some(z) = p.zh_only {
            self.zh_only = z;
        }
        Ok(())
    }

    /// A hand-edited file may hold anything; draw it within the same limits.
    fn clamped(mut self) -> Self {
        for v in [&mut self.captions, &mut self.reply, &mut self.script] {
            v.font_px = v.font_px.clamp(*FONT_PX.start(), *FONT_PX.end());
            v.opacity = v.opacity.clamp(*OPACITY.start(), *OPACITY.end());
        }
        self
    }
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
            view: Views::default(),
        }
    }
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("OWNVOICE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    app.path().app_config_dir().map_err(|e| e.to_string())
}

/// Read-modify-write of the settings file happens under this lock: windows change the view while
/// the settings window may be saving.
static WRITE: Mutex<()> = Mutex::new(());

pub fn load(app: &AppHandle) -> Settings {
    let mut s = load_raw(app);
    s.view = s.view.clamped();
    s
}

fn load_raw(app: &AppHandle) -> Settings {
    let Ok(path) = config_dir(app).map(|d| d.join("settings.json")) else { return Settings::default() };
    if std::fs::metadata(&path).map(|m| m.len() > SETTINGS_MAX_BYTES).unwrap_or(true) {
        return Settings::default();
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default()
}

/// Save the settings form. The view is not on the form, so it keeps what the windows last set.
pub fn save(app: &AppHandle, s: &Settings) -> Result<(), String> {
    let _w = WRITE.lock().map_err(|_| "settings lock is poisoned")?;
    let mut s = s.clone();
    s.view = load(app).view;
    write(app, &s)
}

/// Change part of the view and return all of it.
pub fn update_view(app: &AppHandle, p: &ViewPatch) -> Result<Views, String> {
    let _w = WRITE.lock().map_err(|_| "settings lock is poisoned")?;
    let mut s = load(app);
    s.view.apply(p)?;
    write(app, &s)?;
    Ok(s.view)
}

/// Atomic save: write a temporary file, then rename it over the old one.
fn write(app: &AppHandle, s: &Settings) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_files_get_the_default_view() {
        let s: Settings = serde_json::from_str(r#"{"stt_id":"gemini-live"}"#).unwrap();
        assert_eq!(s.view, Views::default());
        assert!(s.view.hide_from_capture, "notes stay out of screen sharing unless the user turns it off");
    }

    #[test]
    fn a_patch_changes_one_window_within_limits() {
        let mut v = Views::default();
        v.apply(&ViewPatch { window: Some("reply".into()), font_px: Some(99), opacity: Some(5), ..Default::default() }).unwrap();
        assert_eq!(v.reply, View { font_px: 48, opacity: 40 });
        assert_eq!(v.captions, View::default());
        v.apply(&ViewPatch { zh_only: Some(true), ..Default::default() }).unwrap();
        assert!(v.zh_only && v.hide_from_capture);
        assert!(v.apply(&ViewPatch { window: Some("settings".into()), font_px: Some(20), ..Default::default() }).is_err());
    }

    #[test]
    fn hand_edited_values_are_clamped_on_load() {
        let s: Settings = serde_json::from_str(r#"{"view":{"captions":{"font_px":2,"opacity":0}}}"#).unwrap();
        assert_eq!(s.view.clamped().captions, View { font_px: 10, opacity: 40 });
    }
}
