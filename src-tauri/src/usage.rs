// Usage per model: audio seconds sent to speech-to-text, tokens to and from language models.
// Costs are estimates from the catalog's list prices; each provider's own billing page is the
// authority. Kept per local day in usage.json in the config folder, plus a tally for this run.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::catalog;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Tally {
    pub audio_secs: f64,
    pub requests: u64,
    pub input: u64,
    pub cached: u64,
    pub cache_write: u64,
    pub output: u64,
}

impl Tally {
    fn add(&mut self, o: &Tally) {
        self.audio_secs += o.audio_secs;
        self.requests += o.requests;
        self.input += o.input;
        self.cached += o.cached;
        self.cache_write += o.cache_write;
        self.output += o.output;
    }
}

pub struct Usage {
    session: HashMap<String, Tally>,
    days: BTreeMap<String, HashMap<String, Tally>>,
    loaded: bool,
    last_save: Instant,
}

impl Default for Usage {
    fn default() -> Self {
        Self { session: HashMap::new(), days: BTreeMap::new(), loaded: false, last_save: Instant::now() }
    }
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OWNVOICE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir).join("usage.json"));
    }
    app.path().app_config_dir().ok().map(|d| d.join("usage.json"))
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

impl Usage {
    fn ensure_loaded(&mut self, app: &AppHandle) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if let Some(text) = path(app).and_then(|p| std::fs::read_to_string(p).ok()) {
            self.days = serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap_or_default();
        }
    }

    fn record(&mut self, app: &AppHandle, model: &str, t: Tally) {
        self.ensure_loaded(app);
        self.session.entry(model.to_string()).or_default().add(&t);
        self.days.entry(today()).or_default().entry(model.to_string()).or_default().add(&t);
        // Audio arrives every 100 ms; write at most every 10 s (and on stop).
        if self.last_save.elapsed().as_secs() >= 10 {
            self.save(app);
        }
    }

    pub fn save(&mut self, app: &AppHandle) {
        self.last_save = Instant::now();
        let Some(p) = path(app) else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(&self.days) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(tmp, p);
            }
        }
    }
}

pub fn add_audio(app: &AppHandle, model: &str, secs: f64) {
    if let Ok(mut u) = app.state::<Mutex<Usage>>().lock() {
        u.record(app, model, Tally { audio_secs: secs, ..Default::default() });
    }
}

pub fn add_tokens(app: &AppHandle, model: &str, t: Tally) {
    if let Ok(mut u) = app.state::<Mutex<Usage>>().lock() {
        u.record(app, model, Tally { requests: 1, ..t });
    }
}

pub fn flush(app: &AppHandle) {
    if let Ok(mut u) = app.state::<Mutex<Usage>>().lock() {
        if u.loaded {
            u.save(app);
        }
    }
}

#[derive(Serialize)]
pub struct Row {
    pub id: String,
    pub label: String,
    pub provider: String,
    #[serde(flatten)]
    pub tally: Tally,
    pub usd: f64,
}

#[derive(Serialize)]
pub struct Report {
    pub session: Vec<Row>,
    pub today: Vec<Row>,
    pub month: Vec<Row>,
}

fn rows(map: &HashMap<String, Tally>) -> Vec<Row> {
    let mut out: Vec<Row> = map
        .iter()
        .map(|(id, t)| {
            let m = catalog::find(id);
            Row {
                id: id.clone(),
                label: m.map_or(id.as_str(), |m| m.label).to_string(),
                provider: m.map_or("", |m| m.provider).to_string(),
                tally: t.clone(),
                // Cache writes are billed near the input price; count them as input.
                usd: m.map_or(0.0, |m| catalog::cost(m, t.audio_secs, t.input + t.cache_write, t.cached, t.output)),
            }
        })
        .collect();
    out.sort_by(|a, b| b.usd.total_cmp(&a.usd));
    out
}

pub fn report(app: &AppHandle) -> Report {
    let state = app.state::<Mutex<Usage>>();
    let Ok(mut u) = state.lock() else {
        return Report { session: vec![], today: vec![], month: vec![] };
    };
    u.ensure_loaded(app);
    let day = today();
    let month_prefix = &day[..7];
    let mut month: HashMap<String, Tally> = HashMap::new();
    for (d, models) in u.days.range(format!("{month_prefix}-00")..) {
        if !d.starts_with(month_prefix) {
            continue;
        }
        for (id, t) in models {
            month.entry(id.clone()).or_default().add(t);
        }
    }
    Report { session: rows(&u.session), today: u.days.get(&day).map(rows).unwrap_or_default(), month: rows(&month) }
}
