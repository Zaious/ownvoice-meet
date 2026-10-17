// Language models for caption translation and replies, over two wire formats:
//   anthropic      Claude Messages API with streaming. No official Rust SDK, so plain HTTP:
//                  x-api-key + anthropic-version headers; `content_block_delta`/`text_delta`
//                  events carry text, `message_start` / `message_delta` carry usage.
//   openai-compat  Chat Completions with streaming: OpenAI, Gemini's OpenAI-compatible endpoint,
//                  and local servers (vLLM, Ollama). `choices[0].delta.content` carries text; with
//                  stream_options.include_usage the last chunk carries usage.

use std::sync::Mutex;

use futures_util::StreamExt;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::catalog;
use crate::prompts;
use crate::session::{Reply, Session};
use crate::settings::{self, Settings};
use crate::usage::{self, Tally};

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const OPENAI_URL: &str = "https://api.openai.com/v1/chat/completions";
const GEMINI_COMPAT_URL: &str = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions";

pub struct Http(pub reqwest::Client);

/// Where and how to send a request: resolved from settings and the catalog.
pub struct Target {
    pub catalog_id: String,
    pub transport: &'static str,
    pub url: String,
    pub key: Option<String>,
    pub model: String,
    pub effort: String,
}

pub fn target(s: &Settings) -> Result<Target, String> {
    let m = catalog::find(&s.llm_id).ok_or_else(|| format!("unknown model {}", s.llm_id))?;
    if !m.available {
        return Err(format!("{} is not available yet", m.label));
    }
    let effort = if s.llm_effort == "auto" || s.llm_effort.is_empty() { m.min_effort.to_string() } else { s.llm_effort.clone() };
    let (url, model, key) = match m.provider {
        "local" => {
            if s.local_llm_model.trim().is_empty() {
                return Err("本機模型：請在設定填模型名稱".into());
            }
            let base = s.local_llm_url.trim_end_matches('/');
            (format!("{base}/chat/completions"), s.local_llm_model.trim().to_string(), settings::get_key("local"))
        }
        p => {
            let key = settings::get_key(p).ok_or_else(|| format!("沒有設定 {} 的 API key（請到設定填寫）", settings::display_name(p)))?;
            let url = match p {
                "anthropic" => ANTHROPIC_URL,
                "gemini" => GEMINI_COMPAT_URL,
                _ => OPENAI_URL,
            };
            (url.to_string(), m.api_model.to_string(), Some(key))
        }
    };
    Ok(Target { catalog_id: m.id.to_string(), transport: m.transport, url, key, model, effort })
}

/// Stream one completion; `on_delta` gets each piece of text as it arrives. Returns token usage.
pub async fn stream(client: &reqwest::Client, t: &Target, system: Vec<Value>, user: String, mut on_delta: impl FnMut(&str)) -> Result<Tally, String> {
    let anthropic = t.transport == "anthropic";
    let req = if anthropic {
        let mut body = json!({
            "model": t.model,
            "max_tokens": 4000,
            "stream": true,
            "system": system,
            "messages": [{ "role": "user", "content": user }],
        });
        if !t.effort.is_empty() {
            body["output_config"] = json!({ "effort": t.effort });
        }
        client
            .post(&t.url)
            .header("x-api-key", t.key.as_deref().unwrap_or_default())
            .header("anthropic-version", "2023-06-01")
            .json(&body)
    } else {
        // Chat Completions has no cache_control: the system blocks become one system message.
        // OpenAI and Gemini cache long identical prefixes on their own.
        let system_text = system.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n\n");
        let mut body = json!({
            "model": t.model,
            "stream": true,
            "stream_options": { "include_usage": true },
            "messages": [
                { "role": "system", "content": system_text },
                { "role": "user", "content": user }
            ],
        });
        if !t.effort.is_empty() {
            body["reasoning_effort"] = json!(t.effort);
        }
        let mut r = client.post(&t.url).json(&body);
        if let Some(k) = t.key.as_deref().filter(|k| !k.is_empty()) {
            r = r.bearer_auth(k);
        }
        r
    };
    let resp = req.send().await.map_err(|e| format!("network: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("{status}: {}", error_message(&text).chars().take(300).collect::<String>()));
    }
    let mut usage = Tally::default();
    let mut bytes = resp.bytes_stream();
    // Bytes, not text: a Chinese character can be split across two network chunks.
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = bytes.next().await {
        buf.extend_from_slice(&chunk.map_err(|e| format!("stream: {e}"))?);
        while let Some(end) = buf.iter().position(|&b| b == b'\n') {
            let line = String::from_utf8_lossy(&buf[..end]).trim_end_matches('\r').to_string();
            buf.drain(..=end);
            let Some(data) = line.strip_prefix("data:").map(str::trim_start) else { continue };
            if data == "[DONE]" {
                continue;
            }
            let Ok(ev) = serde_json::from_str::<Value>(data) else { continue };
            if anthropic {
                anthropic_event(&ev, &mut usage, &mut on_delta)?;
            } else {
                openai_event(&ev, &mut usage, &mut on_delta)?;
            }
        }
    }
    Ok(usage)
}

fn anthropic_event(ev: &Value, usage: &mut Tally, on_delta: &mut impl FnMut(&str)) -> Result<(), String> {
    match ev["type"].as_str() {
        Some("message_start") => {
            let u = &ev["message"]["usage"];
            usage.input = u["input_tokens"].as_u64().unwrap_or(0);
            usage.cached = u["cache_read_input_tokens"].as_u64().unwrap_or(0);
            usage.cache_write = u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
        }
        Some("content_block_delta") if ev["delta"]["type"] == "text_delta" => {
            if let Some(t) = ev["delta"]["text"].as_str() {
                on_delta(t);
            }
        }
        Some("message_delta") => {
            if let Some(o) = ev["usage"]["output_tokens"].as_u64() {
                usage.output = o;
            }
            if ev["delta"]["stop_reason"] == "refusal" {
                on_delta(" [declined]");
            }
        }
        Some("error") => return Err(ev["error"]["message"].as_str().unwrap_or("stream error").to_string()),
        _ => {}
    }
    Ok(())
}

fn openai_event(ev: &Value, usage: &mut Tally, on_delta: &mut impl FnMut(&str)) -> Result<(), String> {
    if let Some(err) = ev.get("error") {
        return Err(err["message"].as_str().unwrap_or("stream error").to_string());
    }
    if let Some(t) = ev["choices"][0]["delta"]["content"].as_str() {
        on_delta(t);
    }
    let u = &ev["usage"];
    if u.is_object() {
        let prompt = u["prompt_tokens"].as_u64().unwrap_or(0);
        let cached = u["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0);
        usage.input = prompt.saturating_sub(cached);
        usage.cached = cached;
        usage.output = u["completion_tokens"].as_u64().unwrap_or(0);
    }
    Ok(())
}

/// The readable part of an error body: {"error":{"message"}} (OpenAI, Anthropic) or a list of
/// those (Gemini), else the raw text.
fn error_message(text: &str) -> String {
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let e = if v.is_array() { &v[0]["error"] } else { &v["error"] };
    e["message"].as_str().map(str::to_owned).unwrap_or_else(|| text.to_string())
}

fn gather(app: &AppHandle, before: Option<u64>) -> (Settings, Vec<String>, String) {
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
        let (s, ctx, script) = gather(&app, Some(id));
        let t = match target(&s) {
            Ok(t) => t,
            Err(e) => return set(&format!("〔{e}〕")),
        };
        let system = prompts::system_blocks(prompts::LISTEN_SYSTEM, &prompts::parse_glossary(&s.glossary), &script);
        let client = app.state::<Http>().0.clone();
        let mut zh = String::new();
        let res = stream(&client, &t, system, prompts::listen_user(&ctx, &en), |d| {
            zh.push_str(d);
            set(&zh);
        })
        .await;
        match res {
            Ok(u) => usage::add_tokens(&app, &t.catalog_id, u),
            Err(e) => set(&format!("〔翻譯失敗：{e}〕")),
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
        let (s, ctx, script) = gather(&app, None);
        let t = match target(&s) {
            Ok(t) => t,
            Err(e) => return emit(&format!("[{e}]"), true),
        };
        let system = prompts::system_blocks(prompts::REPLY_SYSTEM, &prompts::parse_glossary(&s.glossary), &script);
        let client = app.state::<Http>().0.clone();
        let mut en = String::new();
        let res = stream(&client, &t, system, prompts::reply_user(&ctx, &zh), |d| {
            en.push_str(d);
            emit(&en, false);
        })
        .await;
        match res {
            Ok(u) => {
                usage::add_tokens(&app, &t.catalog_id, u);
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

    fn target_for(id: &str, key: &str) -> Target {
        let s = Settings { llm_id: id.into(), ..Settings::default() };
        let m = catalog::find(id).unwrap();
        let url = match m.provider {
            "anthropic" => ANTHROPIC_URL,
            "gemini" => GEMINI_COMPAT_URL,
            _ => OPENAI_URL,
        };
        Target { catalog_id: id.into(), transport: m.transport, url: url.into(), key: Some(key.into()), model: m.api_model.into(), effort: if s.llm_effort == "auto" { m.min_effort.into() } else { s.llm_effort } }
    }

    #[test]
    fn parses_openai_stream_usage_with_cache() {
        let mut u = Tally::default();
        let mut text = String::new();
        openai_event(&json!({"choices":[{"delta":{"content":"你好"}}]}), &mut u, &mut |d: &str| text.push_str(d)).unwrap();
        openai_event(&json!({"choices":[],"usage":{"prompt_tokens":1200,"completion_tokens":30,"prompt_tokens_details":{"cached_tokens":1000}}}), &mut u, &mut |_: &str| {}).unwrap();
        assert_eq!(text, "你好");
        assert_eq!((u.input, u.cached, u.output), (200, 1000, 30));
    }

    #[test]
    fn reads_error_bodies_from_each_provider() {
        assert_eq!(error_message(r#"{"error":{"message":"bad key"}}"#), "bad key");
        assert_eq!(error_message(r#"[{"error":{"message":"API key not valid"}}]"#), "API key not valid");
        assert_eq!(error_message("plain"), "plain");
    }

    // Real API calls, one per provider whose key is in the environment (or .env.local):
    //   cargo test --lib -- --ignored --nocapture live_models
    #[tokio::test]
    #[ignore]
    async fn live_models_translate_and_reply() {
        let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../.env.local"));
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::new();
        let script = "# Q&A\nQ: How do I contribute?\nThere are no outside contributors yet.\n> 目前還沒有外部貢獻者。";
        let mut ran = 0;
        for m in catalog::MODELS.iter().filter(|m| m.kind == catalog::Kind::Llm && m.available && m.provider != "local") {
            let var = match m.provider { "anthropic" => "ANTHROPIC_API_KEY", "openai" => "OPENAI_API_KEY", _ => "GEMINI_API_KEY" };
            let Ok(key) = std::env::var(var) else { println!("{:<22} skipped ({var} not set)", m.id); continue };
            if key.is_empty() { println!("{:<22} skipped ({var} empty)", m.id); continue }
            let t = target_for(m.id, &key);
            let mut zh = String::new();
            let t0 = std::time::Instant::now();
            let mut first = None;
            let r = stream(&client, &t, prompts::system_blocks(prompts::LISTEN_SYSTEM, &[], script),
                prompts::listen_user(&[], "Could you share your screen and walk us through the timeline?"),
                |d| { first.get_or_insert(t0.elapsed()); zh.push_str(d); }).await;
            match r {
                Ok(u) => {
                    ran += 1;
                    let cost = catalog::cost(m, 0.0, u.input + u.cache_write, u.cached, u.output);
                    println!("{:<22} first {:>4}ms total {:>4}ms in {} cached {} out {} ${cost:.6} | {zh}",
                        m.id, first.map_or(0, |d| d.as_millis()), t0.elapsed().as_millis(), u.input, u.cached, u.output);
                    assert!(zh.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)), "{}: Chinese expected", m.id);
                }
                Err(e) => panic!("{}: {e}", m.id),
            }
        }
        println!("ran {ran} models");
    }

    #[tokio::test]
    #[ignore]
    async fn claude_bad_key_gives_a_readable_error() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let t = target_for("claude-haiku-5-5", "sk-ant-invalid");
        let err = stream(&reqwest::Client::new(), &t, vec![], "hi".into(), |_| {}).await.expect_err("must fail");
        println!("error: {err}");
        assert!(err.starts_with("401"), "status first: {err}");
    }
}

#[cfg(test)]
mod compat_tests {
    use super::*;

    // cargo test --lib -- --ignored --nocapture compat_bad
    #[tokio::test]
    #[ignore]
    async fn compat_bad_keys_are_readable() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::new();
        for (id, url) in [("gpt-6-luna", OPENAI_URL), ("gemini-3.5-flash-lite", GEMINI_COMPAT_URL)] {
            let m = catalog::find(id).unwrap();
            let t = Target { catalog_id: id.into(), transport: m.transport, url: url.into(), key: Some("invalid-key".into()), model: m.api_model.into(), effort: m.min_effort.into() };
            let err = stream(&client, &t, vec![], "hi".into(), |_| {}).await.expect_err("must fail");
            println!("{id}: {err}");
            assert!(err.starts_with("401") || err.starts_with("400") || err.starts_with("403"), "{id}: status first: {err}");
            assert!(!err.contains("<html"), "{id}: readable message");
        }
    }
}
