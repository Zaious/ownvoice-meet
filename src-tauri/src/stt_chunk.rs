// Speech-to-text by sentence: the energy segmenter cuts utterances out of the audio and each one is
// posted as a WAV file to an OpenAI-compatible /audio/transcriptions endpoint. One code path serves
// OpenAI (gpt-4o-mini-transcribe) and self-hosted servers (faster-whisper behind the same API).
// A gray "…" line shows while someone talks; the text arrives when the sentence ends.

use std::time::{Duration, Instant};

use reqwest::multipart::{Form, Part};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;

use crate::audio::chunk_len;
use crate::captions;
use crate::llm::Http;
use crate::usage;
use crate::vad::{wav, Segmenter, VadEvent};

pub const RATE: u32 = 16_000;
const SILENCE_AFTER: Duration = Duration::from_millis(250);

pub struct Config {
    pub catalog_id: String,
    /// Base URL ending before /audio/transcriptions, e.g. https://api.openai.com/v1
    pub base_url: String,
    pub key: Option<String>,
    pub model: String,
    /// ISO 639-1, empty = let the server detect it.
    pub language: String,
}

pub async fn run(app: AppHandle, cfg: Config, mut audio: mpsc::Receiver<Vec<i16>>) {
    let (jobs, queue) = mpsc::channel::<(u64, Vec<i16>)>(32);
    // One upload at a time keeps captions in order.
    let worker = tauri::async_runtime::spawn(upload_worker(app.clone(), cfg, queue));
    captions::status(&app, "聆聽中（切句上傳）");

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
                        let _ = jobs.send((id, utt)).await;
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
    let _ = worker.await;
}

async fn upload_worker(app: AppHandle, cfg: Config, mut queue: mpsc::Receiver<(u64, Vec<i16>)>) {
    let client = app.state::<Http>().0.clone();
    let url = format!("{}/audio/transcriptions", cfg.base_url.trim_end_matches('/'));
    while let Some((id, utt)) = queue.recv().await {
        let secs = utt.len() as f64 / RATE as f64;
        match transcribe(&client, &url, &cfg, wav(&utt, RATE)).await {
            Ok(text) => {
                usage::add_audio(&app, &cfg.catalog_id, secs);
                captions::finish(&app, id, &text);
            }
            Err(e) => {
                captions::fail(&app, id, &format!("辨識失敗：{e}"));
                captions::status(&app, format!("辨識失敗：{e}"));
            }
        }
    }
}

pub(crate) async fn transcribe(client: &reqwest::Client, url: &str, cfg: &Config, wav: Vec<u8>) -> Result<String, String> {
    let file = Part::bytes(wav).file_name("speech.wav").mime_str("audio/wav").map_err(|e| e.to_string())?;
    let mut form = Form::new().part("file", file).text("model", cfg.model.clone()).text("response_format", "json");
    if !cfg.language.is_empty() {
        form = form.text("language", cfg.language.clone());
    }
    let mut req = client.post(url).multipart(form).timeout(Duration::from_secs(30));
    if let Some(k) = cfg.key.as_deref().filter(|k| !k.is_empty()) {
        req = req.bearer_auth(k);
    }
    let resp = req.send().await.map_err(|e| format!("network: {e}"))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    if !status.is_success() {
        let msg = v["error"]["message"].as_str().map(str::to_owned).unwrap_or(body);
        return Err(format!("{status}: {}", msg.chars().take(200).collect::<String>()));
    }
    Ok(v["text"].as_str().unwrap_or_default().to_string())
}
