// Checks for self-hosted servers, behind the "測試連線" buttons in Settings: does the transcription
// server answer a real request, and which models does the chat server offer? Both speak the OpenAI
// API (docs/LOCAL.md), so the same checks work for faster-whisper servers, vLLM, Ollama, LM Studio.

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::stt_chunk::{self, Config};
use crate::vad;

/// Post half a second of silence to `<base>/audio/transcriptions`. Silence should come back as an
/// empty (or near-empty) transcript; what matters is that the server answers in the expected shape.
pub async fn check_stt(client: &reqwest::Client, base: &str, model: &str, key: Option<String>) -> Result<String, String> {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("請先填伺服器網址".into());
    }
    let cfg = Config { catalog_id: "local-whisper".into(), base_url: base.into(), key, model: model.trim().into(), language: "en".into() };
    let wav = vad::wav(&vec![0i16; stt_chunk::RATE as usize / 2], stt_chunk::RATE);
    let t0 = Instant::now();
    stt_chunk::transcribe(client, &format!("{base}/audio/transcriptions"), &cfg, wav).await?;
    Ok(format!("連線成功：伺服器在 {} ms 內回應了轉錄請求", t0.elapsed().as_millis()))
}

/// The model ids `<base>/models` lists (OpenAI's shape: {"data": [{"id": ...}]}).
pub async fn list_models(client: &reqwest::Client, base: &str, key: Option<String>) -> Result<Vec<String>, String> {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("請先填伺服器網址".into());
    }
    let mut req = client.get(format!("{base}/models")).timeout(Duration::from_secs(10));
    if let Some(k) = key.as_deref().filter(|k| !k.is_empty()) {
        req = req.bearer_auth(k);
    }
    let resp = req.send().await.map_err(|e| format!("連不上 {base}：{e}"))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{status}：{}", body.chars().take(200).collect::<String>()));
    }
    let v: Value = serde_json::from_str(&body).map_err(|_| "回應不是 OpenAI 相容的模型清單".to_string())?;
    let mut ids: Vec<String> = v["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().map(str::to_owned)).collect();
    ids.sort();
    if ids.is_empty() {
        return Err("伺服器沒有列出任何模型".into());
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Needs scripts/local_whisper_server.py running on port 8790:
    //   cargo test --lib -- --ignored --nocapture local_whisper_check
    #[tokio::test]
    #[ignore]
    async fn local_whisper_check() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::new();
        let ok = check_stt(&client, "http://127.0.0.1:8790/v1", "large-v3", None).await;
        println!("{ok:?}");
        assert!(ok.is_ok());
        let bad = check_stt(&client, "http://127.0.0.1:1/v1", "x", None).await;
        println!("{bad:?}");
        assert!(bad.is_err(), "a closed port is reported");
    }

    #[tokio::test]
    async fn empty_address_is_reported_without_a_request() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::new();
        assert!(check_stt(&client, "  ", "m", None).await.unwrap_err().contains("網址"));
        assert!(list_models(&client, "", None).await.unwrap_err().contains("網址"));
    }
}
