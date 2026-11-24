// Speech recognition on this computer, free and offline, with sherpa-onnx: NVIDIA Parakeet TDT 0.6B
// v3 for English and other European languages, SenseVoice for Chinese, Japanese, Korean and Cantonese. The energy segmenter (vad.rs,
// as for chunked cloud recognition) cuts sentences; a gray "…" shows while someone talks and the
// words arrive when the sentence ends (about 0.5 to 1 s later on a desktop CPU).
//
// Checked against Gemini Live on the user's 47-minute roundtable (2026-11-24): Parakeet read about as
// well, sometimes catching words Gemini dropped; English-only models turned a Mandarin turn into
// noise, hence SenseVoice for Chinese. With the language on automatic, SenseVoice names each
// sentence's language first: Chinese keeps its text, anything else goes to Parakeet.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::AppHandle;
use tokio::sync::mpsc;

use crate::audio::chunk_len;
use crate::captions;
use crate::models::{self, LOCAL_STT};
use crate::usage;
use crate::vad::{Segmenter, VadEvent};

pub const RATE: u32 = 16_000;
pub const CATALOG_ID: &str = "sherpa-local";
const SILENCE_AFTER: Duration = Duration::from_millis(250);

/// Languages Parakeet v3 was trained on (ISO 639-1).
const PARAKEET: [&str; 25] = ["bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv", "lt", "mt", "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk"];

/// Which model hears a sentence.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Route {
    Parakeet,
    SenseVoice,
    /// SenseVoice names the language, then Chinese stays with it and the rest goes to Parakeet.
    Auto,
}

fn route(language: &str) -> Route {
    match captions::iso639(language).as_str() {
        "" => Route::Auto,
        l if PARAKEET.contains(&l) => Route::Parakeet,
        _ => Route::SenseVoice,
    }
}

struct Engines {
    route: Route,
    parakeet: Option<sherpa_onnx::OfflineRecognizer>,
    sense: Option<sherpa_onnx::OfflineRecognizer>,
}

/// Loaded once and kept while listening, so a pause for quiet does not reload a gigabyte.
static LOADED: Mutex<Option<Arc<Engines>>> = Mutex::new(None);

fn load(app: &AppHandle, route: Route) -> Result<Arc<Engines>, String> {
    if let Some(e) = LOADED.lock().ok().and_then(|l| l.clone()).filter(|e| e.route == route) {
        return Ok(e);
    }
    let path = |i: usize| models::path(app, &LOCAL_STT[i]);
    let parakeet = if route != Route::SenseVoice {
        let mut c = sherpa_onnx::OfflineRecognizerConfig::default();
        c.model_config.transducer = sherpa_onnx::OfflineTransducerModelConfig { encoder: Some(path(0)?), decoder: Some(path(1)?), joiner: Some(path(2)?) };
        c.model_config.tokens = Some(path(3)?);
        c.model_config.model_type = Some("nemo_transducer".into());
        // One thread: with more, onnxruntime's idle threads spin and burn a core for nothing.
        c.model_config.num_threads = 1;
        Some(sherpa_onnx::OfflineRecognizer::create(&c).ok_or("Parakeet 模型載入失敗")?)
    } else {
        None
    };
    let sense = if route != Route::Parakeet {
        let mut c = sherpa_onnx::OfflineRecognizerConfig::default();
        c.model_config.sense_voice = sherpa_onnx::OfflineSenseVoiceModelConfig { model: Some(path(4)?), language: Some("auto".into()), use_itn: true };
        c.model_config.tokens = Some(path(5)?);
        c.model_config.num_threads = 1;
        Some(sherpa_onnx::OfflineRecognizer::create(&c).ok_or("SenseVoice 模型載入失敗")?)
    } else {
        None
    };
    let e = Arc::new(Engines { route, parakeet, sense });
    if let Ok(mut l) = LOADED.lock() {
        *l = Some(e.clone());
    }
    Ok(e)
}

/// Listening ended: give the memory back.
pub fn unload() {
    if let Ok(mut l) = LOADED.lock() {
        *l = None;
    }
}

fn decode(r: &sherpa_onnx::OfflineRecognizer, samples: &[f32]) -> (String, String) {
    let s = r.create_stream();
    s.accept_waveform(RATE as i32, samples);
    r.decode(&s);
    s.get_result().map(|res| (res.text.trim().to_string(), res.lang.trim_matches(|c| c == '<' || c == '>' || c == '|').to_string())).unwrap_or_default()
}

/// The words of one sentence.
fn recognize(e: &Engines, utt: &[i16]) -> String {
    let samples: Vec<f32> = utt.iter().map(|&s| s as f32 / 32768.0).collect();
    let chinese = |t: &str| crate::zh::to_taiwan(t);
    match (e.route, &e.parakeet, &e.sense) {
        (Route::Parakeet, Some(p), _) => decode(p, &samples).0,
        (Route::SenseVoice, _, Some(s)) => chinese(&decode(s, &samples).0),
        (Route::Auto, Some(p), Some(s)) => {
            let (text, lang) = decode(s, &samples);
            if matches!(lang.as_str(), "zh" | "yue") { chinese(&text) } else { decode(p, &samples).0 }
        }
        _ => String::new(),
    }
}

pub async fn run(app: AppHandle, language: String, mut audio: mpsc::Receiver<Vec<i16>>) {
    let route = route(&language);
    captions::status(&app, "載入本機辨識模型…");
    let load_app = app.clone();
    let loaded = tauri::async_runtime::spawn_blocking(move || load(&load_app, route)).await.map_err(|e| e.to_string()).and_then(|r| r);
    let engines = match loaded {
        Ok(e) => e,
        Err(e) => {
            captions::status(&app, format!("本機辨識沒有啟動：{e}"));
            return;
        }
    };
    captions::status(&app, "聆聽中（本機辨識，不上傳）");

    // One sentence at a time, in order, on its own thread (a sentence takes ~0.1-1 s of CPU).
    let (jobs, queue) = std::sync::mpsc::channel::<(u64, Vec<i16>)>();
    let worker_app = app.clone();
    let worker = std::thread::spawn(move || {
        for (id, utt) in queue {
            let secs = utt.len() as f64 / RATE as f64;
            let text = recognize(&engines, &utt);
            usage::add_audio(&worker_app, CATALOG_ID, secs);
            captions::finish(&worker_app, id, &text);
        }
    });

    let mut seg = Segmenter::new(RATE);
    let mut current: Option<u64> = None;
    let mut last_audio = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let silence = vec![0i16; chunk_len(RATE)];
    loop {
        let events = tokio::select! {
            chunk = audio.recv() => match chunk {
                Some(c) => { last_audio = Instant::now(); seg.push(&c) }
                None => break,
            },
            // Loopback delivers nothing while nothing plays: feed silence so a sentence can end.
            _ = tick.tick() => {
                if last_audio.elapsed() > SILENCE_AFTER && seg.in_speech() { seg.push(&silence) } else { continue }
            }
        };
        for ev in events {
            match ev {
                VadEvent::Start(_) => current = Some(captions::open(&app)),
                VadEvent::Audio(_) => {}
                VadEvent::End(utt) => {
                    if let Some(id) = current.take() {
                        let _ = jobs.send((id, utt));
                    }
                }
                VadEvent::Discard => {
                    if let Some(id) = current.take() {
                        captions::drop_line(&app, id);
                    }
                }
            }
        }
    }
    if let Some(id) = current.take() {
        captions::drop_line(&app, id);
    }
    drop(jobs);
    let _ = tauri::async_runtime::spawn_blocking(move || worker.join()).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_language_goes_to_the_model_that_knows_it() {
        assert_eq!(route("en-US"), Route::Parakeet);
        assert_eq!(route("de-DE"), Route::Parakeet);
        assert_eq!(route("zh-TW"), Route::SenseVoice);
        assert_eq!(route("ja-JP"), Route::SenseVoice);
        assert_eq!(route(""), Route::Auto);
    }
}
