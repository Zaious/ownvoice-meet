// Live speech-to-text with OpenAI gpt-live-transcribe over the Realtime WebSocket. Per
// developers.openai.com/api/docs/guides/realtime-transcription:
//   session.update { session: { type: "transcription", audio.input: { format audio/pcm 24 kHz,
//                    transcription { model, languages, delay }, turn_detection: null } } }
//   the model has no server-side turn detection: the client segments and sends
//   input_audio_buffer.append (base64 PCM16) then input_audio_buffer.commit per utterance
//   conversation.item.input_audio_transcription.delta      new text for item_id (gray)
//   conversation.item.input_audio_transcription.completed  final transcript for item_id (white)
// Only speech is sent (the segmenter's preroll + utterance), so silence is never billed.
// The guide shows no transcription-only URL; both forms below are tried, in order.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::AppHandle;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest, Message};

use crate::audio::chunk_len;
use crate::captions::{self, Failure};
use crate::usage;
use crate::vad::{Segmenter, VadEvent};

pub const RATE: u32 = 24_000;
const URLS: [&str; 2] = [
    "wss://api.openai.com/v1/realtime?intent=transcription",
    "wss://api.openai.com/v1/realtime?model=gpt-live-transcribe",
];
const SILENCE_AFTER: Duration = Duration::from_millis(250);

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub async fn run(app: AppHandle, catalog_id: String, key: String, model: String, language: String, mut audio: mpsc::Receiver<Vec<i16>>) {
    let mut backoff = 1u64;
    loop {
        match session(&app, &catalog_id, &key, &model, &language, &mut audio).await {
            Ok(()) => break,
            Err(Failure::Fatal(msg)) => {
                captions::status(&app, format!("OpenAI 拒絕連線：{msg}（請到設定檢查 key）"));
                break;
            }
            Err(Failure::Retry(msg)) => {
                captions::status(&app, format!("OpenAI 連線中斷：{msg}；{backoff} 秒後重試"));
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(30);
            }
        }
    }
}

fn session_update(model: &str, language: &str) -> String {
    let mut transcription = json!({ "model": model, "delay": "low" });
    if !language.is_empty() {
        transcription["languages"] = json!([language]);
    }
    json!({
        "type": "session.update",
        "session": {
            "type": "transcription",
            "audio": { "input": {
                "format": { "type": "audio/pcm", "rate": RATE },
                "transcription": transcription,
                "turn_detection": null
            } }
        }
    })
    .to_string()
}

/// Connect (trying each URL form), configure the session and wait for it to be accepted.
async fn connect(key: &str, model: &str, language: &str) -> Result<Ws, Failure> {
    let mut last = String::new();
    for url in URLS {
        let mut req = url.into_client_request().map_err(|e| Failure::Fatal(e.to_string()))?;
        let auth = format!("Bearer {key}").parse().map_err(|_| Failure::Fatal("key has invalid characters".into()))?;
        req.headers_mut().insert("Authorization", auth);
        let mut ws = match tokio_tungstenite::connect_async(req).await {
            Ok((ws, _)) => ws,
            Err(tungstenite::Error::Http(resp)) => {
                let code = resp.status().as_u16();
                let body = resp.body().as_deref().map(String::from_utf8_lossy).unwrap_or_default().chars().take(200).collect::<String>();
                last = format!("HTTP {code} {body}");
                if code == 401 || code == 403 {
                    return Err(Failure::Fatal(last));
                }
                continue; // this URL form was refused; try the next
            }
            Err(e) => return Err(Failure::Retry(e.to_string())),
        };
        ws.send(Message::Text(session_update(model, language).into())).await.map_err(|e| Failure::Retry(e.to_string()))?;
        let accepted = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(msg) = ws.next().await {
                let Some(v) = json_of(msg.map_err(|e| Failure::Retry(e.to_string()))?) else { continue };
                match v["type"].as_str() {
                    Some("session.updated") => return Ok(()),
                    Some("error") => return Err(Failure::Fatal(v["error"]["message"].as_str().unwrap_or("session refused").to_string())),
                    _ => {}
                }
            }
            Err(Failure::Retry("closed during setup".into()))
        })
        .await
        .map_err(|_| Failure::Retry("setup timed out".into()))?;
        accepted?;
        return Ok(ws);
    }
    Err(Failure::Fatal(format!("no realtime URL accepted the session: {last}")))
}

async fn session(app: &AppHandle, catalog_id: &str, key: &str, model: &str, language: &str, audio: &mut mpsc::Receiver<Vec<i16>>) -> Result<(), Failure> {
    let ws = connect(key, model, language).await?;
    let (mut tx, mut rx) = ws.split();
    captions::status(app, "聆聽中（OpenAI Live）");
    let retry = |e: tungstenite::Error| Failure::Retry(e.to_string());

    let mut seg = Segmenter::new(RATE);
    let mut lines: HashMap<String, (u64, String)> = HashMap::new(); // item_id -> (caption id, text)
    let mut last_audio = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let silence = vec![0i16; chunk_len(RATE)];
    loop {
        let events = tokio::select! {
            chunk = audio.recv() => match chunk {
                Some(c) => { last_audio = Instant::now(); seg.push(&c) }
                None => {
                    let _ = tx.close().await;
                    return Ok(());
                }
            },
            _ = tick.tick() => {
                if last_audio.elapsed() > SILENCE_AFTER && seg.in_speech() { seg.push(&silence) } else { continue }
            },
            msg = rx.next() => {
                match msg {
                    Some(Ok(m)) => if let Some(v) = json_of(m) { handle(app, &v, &mut lines) },
                    Some(Err(e)) => return Err(retry(e)),
                    None => return Err(Failure::Retry("connection ended".into())),
                }
                continue;
            }
        };
        for ev in events {
            let send = match ev {
                VadEvent::Start(pcm) | VadEvent::Audio(pcm) => {
                    usage::add_audio(app, catalog_id, pcm.len() as f64 / RATE as f64);
                    append(&pcm)
                }
                VadEvent::End(_) => json!({ "type": "input_audio_buffer.commit" }).to_string(),
                VadEvent::Discard => json!({ "type": "input_audio_buffer.clear" }).to_string(),
            };
            tx.send(Message::Text(send.into())).await.map_err(retry)?;
        }
    }
}

fn append(pcm: &[i16]) -> String {
    let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
    json!({ "type": "input_audio_buffer.append", "audio": base64::engine::general_purpose::STANDARD.encode(bytes) }).to_string()
}

fn json_of(msg: Message) -> Option<Value> {
    let text = match msg {
        Message::Text(t) => t.to_string(),
        Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
        _ => return None,
    };
    serde_json::from_str(&text).ok()
}

fn handle(app: &AppHandle, v: &Value, lines: &mut HashMap<String, (u64, String)>) {
    let item = v["item_id"].as_str().unwrap_or_default().to_string();
    match v["type"].as_str() {
        Some("conversation.item.input_audio_transcription.delta") => {
            let entry = lines.entry(item).or_insert_with(|| (captions::open(app), String::new()));
            entry.1.push_str(v["delta"].as_str().unwrap_or_default());
            captions::interim(app, entry.0, &entry.1);
        }
        // Completions of different turns may arrive out of order: matched by item_id.
        Some("conversation.item.input_audio_transcription.completed") => {
            let text = v["transcript"].as_str().unwrap_or_default();
            match lines.remove(&item) {
                Some((id, _)) => captions::finish(app, id, text),
                None if !text.trim().is_empty() => {
                    let id = captions::open(app);
                    captions::finish(app, id, text);
                }
                None => {}
            }
        }
        Some("error") => captions::status(app, format!("OpenAI：{}", v["error"]["message"].as_str().unwrap_or("error"))),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_update_matches_the_documented_shape() {
        let v: Value = serde_json::from_str(&session_update("gpt-live-transcribe", "en")).unwrap();
        let input = &v["session"]["audio"]["input"];
        assert_eq!(v["session"]["type"], "transcription");
        assert_eq!(input["format"]["rate"], 24_000);
        assert_eq!(input["transcription"]["languages"][0], "en");
        assert!(input["turn_detection"].is_null());
        let auto: Value = serde_json::from_str(&session_update("gpt-live-transcribe", "")).unwrap();
        assert!(auto["session"]["audio"]["input"]["transcription"].get("languages").is_none());
    }

    // cargo test --lib -- --ignored --nocapture openai_rt
    #[tokio::test]
    #[ignore]
    async fn openai_rt_bad_key_is_fatal() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        match connect("sk-invalid", "gpt-live-transcribe", "en").await {
            Err(Failure::Fatal(m)) => println!("fatal as expected: {m}"),
            Err(Failure::Retry(m)) => panic!("must not retry a bad key: {m}"),
            Ok(_) => panic!("must not connect"),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn openai_rt_real_key_session_accepted() {
        let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../.env.local"));
        let Some(key) = std::env::var("OPENAI_API_KEY").ok().filter(|k| !k.is_empty()) else {
            println!("OPENAI_API_KEY not set; skipped");
            return;
        };
        let _ = rustls::crypto::ring::default_provider().install_default();
        assert!(connect(&key, "gpt-live-transcribe", "en").await.is_ok(), "session refused");
    }
}
