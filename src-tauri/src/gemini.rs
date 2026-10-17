// Speech-to-text over the Gemini Live API (WebSocket, BidiGenerateContent) with a transcription
// model. Protocol per ai.google.dev/gemini-api/docs/live-api/live-transcribe:
//   setup { model, generationConfig.responseModalities, inputAudioTranscription.languageCodes }
//   realtimeInput.audio { data: base64 16-bit 16 kHz mono PCM, mimeType "audio/pcm;rate=16000" }
//   serverContent.interimInputTranscription.text  live hypothesis while someone talks (gray)
//   serverContent.inputTranscription.text         finalized text at a pause (white, translated)
// A session streams for up to 10 minutes, so it is replaced before that, at a pause when possible.
// EZTRANS_DEBUG=1 appends every raw server message to gemini-debug.log in the config folder.

use std::io::Write as _;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{self, Message};

use crate::audio::chunk_len;
use crate::llm;
use crate::session::{Segment, Session};
use crate::usage;

/// Gemini Live takes 16 kHz PCM.
pub const RATE: u32 = 16_000;
const ENDPOINT: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";
const ROTATE_AT_PAUSE: Duration = Duration::from_secs(9 * 60);
const ROTATE_ANYWAY: Duration = Duration::from_secs(9 * 60 + 50);
/// Loopback delivers nothing while nothing plays; silence is sent so the server hears the pause.
const SILENCE_AFTER: Duration = Duration::from_millis(250);

enum End {
    AudioClosed,
    Rotate,
}

struct Fatal(String);

/// Run until the audio channel closes (listening stopped), reconnecting as needed.
pub async fn run(app: AppHandle, key: String, model: String, language: String, mut audio: mpsc::Receiver<Vec<i16>>) {
    let mut live = Live::default();
    let mut backoff = 1u64;
    loop {
        match session(&app, &key, &model, &language, &mut audio, &mut live).await {
            Ok(End::AudioClosed) => break,
            Ok(End::Rotate) => backoff = 1,
            Err(Err2::Fatal(Fatal(msg))) => {
                status(&app, format!("Gemini 拒絕連線：{msg}（請到設定檢查 key 與模型）"));
                break;
            }
            Err(Err2::Retry(msg)) => {
                status(&app, format!("Gemini 連線中斷：{msg}；{backoff} 秒後重試"));
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(30);
            }
        }
    }
}

enum Err2 {
    Fatal(Fatal),
    Retry(String),
}

fn retry(e: impl std::fmt::Display) -> Err2 {
    Err2::Retry(e.to_string())
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Open a socket, send the setup and wait for setupComplete.
async fn connect(key: &str, model: &str, language: &str, log: impl Fn(&str)) -> Result<Ws, Err2> {
    let url = format!("{ENDPOINT}?key={key}");
    let (ws, _) = match tokio_tungstenite::connect_async(url.as_str()).await {
        Ok(ok) => ok,
        Err(tungstenite::Error::Http(resp)) => {
            let code = resp.status();
            let body = resp.body().as_deref().map(String::from_utf8_lossy).unwrap_or_default().chars().take(200).collect::<String>();
            let msg = format!("HTTP {code} {body}");
            return Err(if code.as_u16() == 401 || code.as_u16() == 403 || code.as_u16() == 404 {
                Err2::Fatal(Fatal(msg))
            } else {
                Err2::Retry(msg)
            });
        }
        Err(e) => return Err(retry(e)),
    };
    let (mut tx, mut rx) = ws.split();

    let languages: Vec<&str> = if language.trim().is_empty() { vec![] } else { vec![language.trim()] };
    let setup = json!({
        "setup": {
            "model": format!("models/{model}"),
            "generationConfig": { "responseModalities": ["TEXT"] },
            "inputAudioTranscription": { "languageCodes": languages }
        }
    });
    tx.send(Message::Text(setup.to_string().into())).await.map_err(retry)?;

    // Wait for setupComplete before sending audio. A bad key or model closes the socket here,
    // with the reason in the close frame.
    let wait = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(msg) = rx.next().await {
            match parse(msg.map_err(retry)?, &log) {
                Parsed::Json(v) if v.get("setupComplete").is_some() => return Ok(()),
                Parsed::Closed(reason) => return Err(Err2::Fatal(Fatal(format!("closed during setup: {reason}")))),
                _ => {}
            }
        }
        Err(retry("closed during setup"))
    });
    wait.await.map_err(|_| retry("setup timed out"))??;
    tx.reunite(rx).map_err(|_| retry("socket halves did not match"))
}

async fn session(
    app: &AppHandle,
    key: &str,
    model: &str,
    language: &str,
    audio: &mut mpsc::Receiver<Vec<i16>>,
    live: &mut Live,
) -> Result<End, Err2> {
    let log = |t: &str| debug_log(app, t);
    let ws = connect(key, model, language, log).await?;
    let (mut tx, mut rx) = ws.split();
    status(app, "聆聽中（Gemini Live）");

    let started = Instant::now();
    let mut last_audio = Instant::now();
    let mut going_away = false;
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let silence = vec![0i16; chunk_len(RATE)];
    loop {
        tokio::select! {
            chunk = audio.recv() => match chunk {
                Some(chunk) => {
                    last_audio = Instant::now();
                    tx.send(audio_message(&chunk)).await.map_err(retry)?;
                    // Gemini bills every second streamed, silence included.
                    usage::add_audio(app, "gemini-live", chunk.len() as f64 / RATE as f64);
                }
                None => {
                    // Listening stopped: end the stream and give the last words a moment to arrive.
                    let _ = tx.send(Message::Text(json!({"realtimeInput": {"audioStreamEnd": true}}).to_string().into())).await;
                    let _ = tokio::time::timeout(Duration::from_millis(1500), async {
                        while let Some(Ok(msg)) = rx.next().await {
                            if let Parsed::Json(v) = parse(msg, &log) { live.handle(app, &v); }
                        }
                    }).await;
                    let _ = tx.close().await;
                    return Ok(End::AudioClosed);
                }
            },
            msg = rx.next() => match msg {
                Some(Ok(msg)) => match parse(msg, &log) {
                    Parsed::Json(v) => {
                        if v.get("goAway").is_some() { going_away = true; }
                        live.handle(app, &v);
                    }
                    Parsed::Closed(reason) => {
                        return if started.elapsed() > ROTATE_AT_PAUSE { Ok(End::Rotate) } else { Err(retry(format!("closed: {reason}"))) };
                    }
                    Parsed::Other => {}
                },
                Some(Err(e)) => return Err(retry(e)),
                None => return Err(retry("connection ended")),
            },
            _ = tick.tick() => {
                if last_audio.elapsed() > SILENCE_AFTER {
                    tx.send(audio_message(&silence)).await.map_err(retry)?;
                    usage::add_audio(app, "gemini-live", silence.len() as f64 / RATE as f64);
                }
                let quiet = live.current.is_none();
                if (going_away && quiet) || (started.elapsed() > ROTATE_AT_PAUSE && quiet) || started.elapsed() > ROTATE_ANYWAY {
                    let _ = tx.close().await;
                    return Ok(End::Rotate);
                }
            }
        }
    }
}

fn audio_message(chunk: &[i16]) -> Message {
    let bytes: Vec<u8> = chunk.iter().flat_map(|s| s.to_le_bytes()).collect();
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    Message::Text(json!({"realtimeInput": {"audio": {"data": data, "mimeType": "audio/pcm;rate=16000"}}}).to_string().into())
}

enum Parsed {
    Json(Value),
    Closed(String),
    Other,
}

fn parse(msg: Message, log: &impl Fn(&str)) -> Parsed {
    let text = match msg {
        Message::Text(t) => t.to_string(),
        // The Live API also sends JSON in binary frames.
        Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
        Message::Close(frame) => return Parsed::Closed(frame.map(|f| format!("{} {}", f.code, f.reason)).unwrap_or_default()),
        _ => return Parsed::Other,
    };
    log(&text);
    serde_json::from_str(&text).map(Parsed::Json).unwrap_or(Parsed::Other)
}

fn debug_log(app: &AppHandle, text: &str) {
    if std::env::var_os("EZTRANS_DEBUG").is_none_or(|v| v != "1") {
        return;
    }
    let Ok(dir) = app.path().app_config_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("gemini-debug.log")) {
        let _ = writeln!(f, "{text}");
    }
}

fn status(app: &AppHandle, msg: impl Into<String>) {
    if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
        s.set_status(app, msg);
    }
}

/// Which caption the live hypothesis belongs to. It persists across reconnects.
#[derive(Default)]
struct Live {
    current: Option<u64>,
}

impl Live {
    fn handle(&mut self, app: &AppHandle, v: &Value) {
        let sc = &v["serverContent"];
        if let Some(t) = sc["interimInputTranscription"]["text"].as_str() {
            self.interim(app, t.trim());
        }
        if let Some(t) = sc["inputTranscription"]["text"].as_str() {
            self.finish(app, t.trim());
        }
    }

    fn interim(&mut self, app: &AppHandle, text: &str) {
        if text.is_empty() {
            return;
        }
        let state = app.state::<Mutex<Session>>();
        let Ok(mut s) = state.lock() else { return };
        let id = match self.current {
            Some(id) => id,
            None => *self.current.insert(s.next_segment_id()),
        };
        s.upsert_segment(app, Segment { id, en: text.to_string(), zh: String::new(), is_final: false });
    }

    fn finish(&mut self, app: &AppHandle, text: &str) {
        let state = app.state::<Mutex<Session>>();
        let Ok(mut s) = state.lock() else { return };
        if text.is_empty() {
            // Nothing final came of the hypothesis: drop the gray line.
            if let Some(id) = self.current.take() {
                s.remove_segment(app, id);
            }
            return;
        }
        let id = self.current.take().unwrap_or_else(|| s.next_segment_id());
        s.upsert_segment(app, Segment { id, en: text.to_string(), zh: String::new(), is_final: true });
        drop(s);
        llm::spawn_translate(app.clone(), id, text.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Network checks against the real endpoint: cargo test --lib -- --ignored gemini
    #[tokio::test]
    #[ignore]
    async fn gemini_bad_key_is_reported_not_retried() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        match connect("not-a-real-key", "gemini-3.5-transcribe-live", "en-US", |_| {}).await {
            Err(Err2::Fatal(Fatal(msg))) => println!("fatal as expected: {msg}"),
            Err(Err2::Retry(msg)) => panic!("a bad key must not be retried forever: {msg}"),
            Ok(_) => panic!("a bad key must not connect"),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn gemini_real_key_reaches_setup_complete() {
        let Ok(key) = std::env::var("GEMINI_API_KEY") else {
            println!("GEMINI_API_KEY not set; skipped");
            return;
        };
        let _ = rustls::crypto::ring::default_provider().install_default();
        let ws = connect(&key, "gemini-3.5-transcribe-live", "en-US", |t| println!("server: {t}")).await;
        assert!(ws.is_ok(), "setup failed");
    }
}
