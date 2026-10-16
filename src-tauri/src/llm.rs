// Claude over the Messages API with streaming (Server-Sent Events). There is no official Rust SDK,
// so this is plain HTTP: x-api-key + anthropic-version headers, `content_block_delta` events with
// `text_delta` carry the text.

use std::sync::Mutex;

use futures_util::StreamExt;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::prompts;
use crate::session::{Reply, Session};
use crate::settings;

const API_URL: &str = "https://api.anthropic.com/v1/messages";

pub struct Http(pub reqwest::Client);

/// Stream one completion; `on_delta` gets each piece of text as it arrives.
pub async fn stream(
    client: &reqwest::Client,
    key: &str,
    model: &str,
    effort: &str,
    system: Vec<Value>,
    user: String,
    mut on_delta: impl FnMut(&str),
) -> Result<(), String> {
    let body = json!({
        "model": model,
        "max_tokens": 4000,
        "stream": true,
        "system": system,
        "output_config": { "effort": effort },
        "messages": [{ "role": "user", "content": user }],
    });
    let resp = client
        .post(API_URL)
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("network: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
            .unwrap_or(text);
        return Err(format!("{status}: {}", msg.chars().take(300).collect::<String>()));
    }
    let mut bytes = resp.bytes_stream();
    // Bytes, not text: a Chinese character can be split across two network chunks.
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = bytes.next().await {
        buf.extend_from_slice(&chunk.map_err(|e| format!("stream: {e}"))?);
        while let Some(end) = buf.iter().position(|&b| b == b'\n') {
            let line = String::from_utf8_lossy(&buf[..end]).trim_end_matches('\r').to_string();
            buf.drain(..=end);
            let Some(data) = line.strip_prefix("data: ") else { continue };
            let Ok(ev) = serde_json::from_str::<Value>(data) else { continue };
            match ev["type"].as_str() {
                Some("content_block_delta") if ev["delta"]["type"] == "text_delta" => {
                    if let Some(t) = ev["delta"]["text"].as_str() {
                        on_delta(t);
                    }
                }
                Some("message_delta") if ev["delta"]["stop_reason"] == "refusal" => on_delta(" [declined]"),
                Some("error") => return Err(ev["error"]["message"].as_str().unwrap_or("stream error").to_string()),
                _ => {}
            }
        }
    }
    Ok(())
}

fn gather(app: &AppHandle, before: Option<u64>) -> (settings::Settings, Vec<String>, String) {
    let s = settings::load(app);
    let (ctx, script) = app
        .state::<Mutex<Session>>()
        .lock()
        .map(|sess| (sess.context(s.context_lines, before), sess.script.clone()))
        .unwrap_or_default();
    (s, ctx, script)
}

/// Translate a finished caption; the Chinese streams into the caption as it arrives.
pub fn spawn_translate(app: AppHandle, id: u64, en: String) {
    tauri::async_runtime::spawn(async move {
        let set = |zh: &str| {
            if let Ok(mut sess) = app.state::<Mutex<Session>>().lock() {
                sess.set_zh(&app, id, zh);
            }
        };
        let Some(key) = settings::get_key("anthropic") else {
            set("〔沒有設定 Anthropic key，無法翻譯〕");
            return;
        };
        let (s, ctx, script) = gather(&app, Some(id));
        let system = prompts::system_blocks(prompts::LISTEN_SYSTEM, &prompts::parse_glossary(&s.glossary), &script);
        let client = app.state::<Http>().0.clone();
        let mut zh = String::new();
        let res = stream(&client, &key, &s.llm_model, &s.llm_effort, system, prompts::listen_user(&ctx, &en), |d| {
            zh.push_str(d);
            set(&zh);
        })
        .await;
        if let Err(e) = res {
            set(&format!("〔翻譯失敗：{e}〕"));
        }
    });
}

/// Turn typed Chinese into English to read aloud; the reply window follows the `reply` events.
pub fn spawn_reply(app: AppHandle, id: u64, zh: String) {
    tauri::async_runtime::spawn(async move {
        let emit = |en: &str, done: bool| {
            let r = Reply { id, zh: zh.clone(), en: en.to_string(), done };
            if let Ok(mut sess) = app.state::<Mutex<Session>>().lock() {
                if sess.reply.as_ref().is_some_and(|cur| cur.id != id) {
                    return; // a newer reply replaced this one
                }
                sess.reply = Some(r.clone());
            }
            let _ = app.emit("reply", r);
        };
        let Some(key) = settings::get_key("anthropic") else {
            emit("[No Anthropic key. Add one in Settings.]", true);
            return;
        };
        let (s, ctx, script) = gather(&app, None);
        let system = prompts::system_blocks(prompts::REPLY_SYSTEM, &prompts::parse_glossary(&s.glossary), &script);
        let client = app.state::<Http>().0.clone();
        let mut en = String::new();
        let res = stream(&client, &key, &s.llm_model, &s.llm_effort, system, prompts::reply_user(&ctx, &zh), |d| {
            en.push_str(d);
            emit(&en, false);
        })
        .await;
        match res {
            Ok(()) => {
                emit(&en, true);
                if let Ok(mut sess) = app.state::<Mutex<Session>>().lock() {
                    let after = sess.segments.last().map_or(0, |x| x.id);
                    sess.my_lines.push((after, en.clone()));
                }
            }
            Err(e) => emit(&format!("[Failed: {e}]"), true),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real API call: ANTHROPIC_API_KEY must be set. cargo test --lib -- --ignored claude
    #[tokio::test]
    #[ignore]
    async fn claude_translates_a_caption_and_writes_a_reply() {
        let Ok(key) = std::env::var("ANTHROPIC_API_KEY") else {
            println!("ANTHROPIC_API_KEY not set; skipped");
            return;
        };
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::new();
        let script = "# Q&A\nQ: How do I contribute?\nThere are no outside contributors yet.\n> 目前還沒有外部貢獻者。";

        let mut zh = String::new();
        let sys = prompts::system_blocks(prompts::LISTEN_SYSTEM, &[], script);
        let t0 = std::time::Instant::now();
        stream(&client, &key, "claude-haiku-5-5", "low", sys, prompts::listen_user(&[], "Could you share your screen and walk us through the timeline?"), |d| zh.push_str(d))
            .await
            .expect("translation");
        println!("translate {:.2}s: {zh}", t0.elapsed().as_secs_f32());
        assert!(zh.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)), "Chinese expected");
        assert!(!zh.to_lowercase().contains("recogni"), "no commentary in a subtitle");

        let mut en = String::new();
        let sys = prompts::system_blocks(prompts::REPLY_SYSTEM, &[], script);
        let t0 = std::time::Instant::now();
        stream(&client, &key, "claude-haiku-5-5", "low", sys, prompts::reply_user(&[], "目前還沒有外部貢獻者，但歡迎送 pull request"), |d| en.push_str(d))
            .await
            .expect("reply");
        println!("reply {:.2}s: {en}", t0.elapsed().as_secs_f32());
        assert!(en.to_lowercase().contains("contributor"), "reuses the script's wording");
    }

    #[tokio::test]
    #[ignore]
    async fn claude_bad_key_gives_a_readable_error() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let err = stream(&reqwest::Client::new(), "sk-ant-invalid", "claude-haiku-5-5", "low", vec![], "hi".into(), |_| {})
            .await
            .expect_err("must fail");
        println!("error: {err}");
        assert!(err.starts_with("401"), "status first: {err}");
    }
}
