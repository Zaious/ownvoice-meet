// Sign in with ChatGPT (OpenAI, preview since 2026-10): the user's ChatGPT plan (Go, Plus, Pro or
// Business) pays for text requests instead of an API key. Only the Responses API is open to it:
// no transcription or audio, so this can stand in for the translation key, not the speech one.
//
// Written from the protocol documentation at developers.openai.com/siwc/token-sharing-open-source
// (OAuth 2 authorization code with PKCE S256, a loopback callback on 127.0.0.1, dynamic client
// registration through client_id=dynamic_agent_client, the issued client_id kept for next time).
// OpenAI's own DevKit is under a noncommercial licence and none of its code is used here.
//
// Settings offers it as the translation option "chatgpt-luna" (catalog.rs); llm.rs sends its
// requests here. The measuring test at the bottom signs in (the browser opens), keeps the tokens in
// the system keychain and compares this route with the API key route on the same model.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
#[cfg(test)]
use futures_util::StreamExt;
use ring::rand::SecureRandom;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const ISSUER: &str = "https://auth.openai.com";
const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPES: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
/// The scope that lets requests draw on the ChatGPT plan; an ID token alone does not.
const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
const CALLBACK: &str = "/auth/callback";
const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
const KEYCHAIN_SERVICE: &str = "ownvoice-meet";
const KEYCHAIN_USER: &str = "chatgpt";

/// A ChatGPT connection, kept in the system keychain (never in a file, never shown).
#[derive(Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub client_id: String,
    pub access: String,
    pub refresh: String,
    /// Unix seconds.
    pub expires_at: u64,
    pub scopes: Vec<String>,
    /// The account's email, to show which account is connected.
    #[serde(default)]
    pub email: String,
}

/// What the settings window shows.
#[derive(Serialize)]
pub struct Status {
    pub signed_in: bool,
    pub plan: bool,
    pub email: String,
}

pub fn status() -> Status {
    match load() {
        Some(t) => Status { signed_in: true, plan: t.plan_allowed(), email: t.email },
        None => Status { signed_in: false, plan: false, email: String::new() },
    }
}

/// The ID token's claims, read for the nonce and the email shown in Settings. The access token is
/// what OpenAI checks on every request, so nothing here is trusted for access.
fn id_claims(id_token: &str) -> Value {
    let payload = id_token.split('.').nth(1).unwrap_or("");
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null)
}

impl Tokens {
    pub fn plan_allowed(&self) -> bool {
        self.scopes.iter().any(|s| s == PLAN_SCOPE)
    }
    fn expired(&self) -> bool {
        now() + 60 >= self.expires_at
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn random() -> String {
    let mut buf = [0u8; 32];
    let _ = ring::rand::SystemRandom::new().fill(&mut buf);
    b64(&buf)
}

/// PKCE S256: base64url, no padding, of the verifier's SHA-256.
fn challenge(verifier: &str) -> String {
    b64(ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes()).as_ref())
}

/// application/x-www-form-urlencoded, through the URL type reqwest already carries.
fn form(pairs: &[(&str, &str)]) -> String {
    let mut u = reqwest::Url::parse("http://form.invalid/").expect("static url");
    u.query_pairs_mut().extend_pairs(pairs);
    u.query().unwrap_or("").to_string()
}

/// Windows Credential Manager holds at most 2560 bytes per entry (1280 characters as keyring
/// stores them), and the access token alone is longer: the connection is kept in numbered pieces.
const PIECE: usize = 1000;

fn piece(i: usize) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, &format!("{KEYCHAIN_USER}.{i}")).map_err(|e| e.to_string())
}

pub fn load() -> Option<Tokens> {
    let count: usize = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).ok()?.get_password().ok()?.parse().ok()?;
    let mut text = String::new();
    for i in 0..count {
        text.push_str(&piece(i).ok()?.get_password().ok()?);
    }
    serde_json::from_str(&text).ok()
}

pub fn save(t: &Tokens) -> Result<(), String> {
    let text = serde_json::to_string(t).map_err(|e| e.to_string())?;
    let chars: Vec<char> = text.chars().collect();
    let pieces: Vec<String> = chars.chunks(PIECE).map(|c| c.iter().collect()).collect();
    for (i, p) in pieces.iter().enumerate() {
        piece(i)?.set_password(p).map_err(|e| e.to_string())?;
    }
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
        .and_then(|e| e.set_password(&pieces.len().to_string()))
        .map_err(|e| e.to_string())
}

pub fn forget() {
    if let Ok(e) = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER) {
        let count: usize = e.get_password().ok().and_then(|c| c.parse().ok()).unwrap_or(0);
        let _ = e.delete_credential();
        for i in 0..count.max(8) {
            if let Ok(p) = piece(i) {
                let _ = p.delete_credential();
            }
        }
    }
}

async fn endpoints(client: &reqwest::Client) -> Result<(String, String), String> {
    let v: Value = client
        .get(format!("{ISSUER}/.well-known/openid-configuration"))
        .send()
        .await
        .map_err(|e| format!("network: {e}"))?
        .json()
        .await
        .map_err(|e| format!("discovery: {e}"))?;
    let auth = v["authorization_endpoint"].as_str().ok_or("discovery: no authorization_endpoint")?;
    let token = v["token_endpoint"].as_str().ok_or("discovery: no token_endpoint")?;
    for u in [auth, token] {
        if !u.starts_with(ISSUER) {
            return Err(format!("discovery: unexpected endpoint {u}"));
        }
    }
    Ok((auth.to_string(), token.to_string()))
}

fn tokens_from(v: &Value, client_id: &str, previous: Option<&Tokens>) -> Result<Tokens, String> {
    let access = v["access_token"].as_str().ok_or("no access_token")?.to_string();
    let refresh = v["refresh_token"].as_str().map(str::to_owned).or_else(|| previous.map(|p| p.refresh.clone())).unwrap_or_default();
    let scopes = match v["scope"].as_str() {
        Some(s) => s.split_whitespace().map(str::to_owned).collect(),
        None => previous.map(|p| p.scopes.clone()).unwrap_or_default(),
    };
    let expires_at = now() + v["expires_in"].as_u64().unwrap_or(3600);
    let email = v["id_token"]
        .as_str()
        .and_then(|t| id_claims(t)["email"].as_str().map(str::to_owned))
        .or_else(|| previous.map(|p| p.email.clone()))
        .unwrap_or_default();
    Ok(Tokens { client_id: client_id.to_string(), access, refresh, expires_at, scopes, email })
}

async fn token_request(client: &reqwest::Client, url: &str, body: String) -> Result<Value, String> {
    let r = client
        .post(url)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("network: {e}"))?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let msg = v["error_description"].as_str().or(v["error"]["message"].as_str()).or(v["error"].as_str()).unwrap_or("");
        return Err(format!("{status}: {msg}"));
    }
    Ok(v)
}

/// Waits for the browser to come back to the loopback address; returns the code and the issued
/// client id after checking the state.
fn wait_for_callback(listener: TcpListener, state: String, give_up: Duration) -> Result<(String, Option<String>), String> {
    // Polled, so the wait ends by itself: a blocked accept would keep the app's thread forever.
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + give_up;
    loop {
        let mut stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err("等了 5 分鐘沒有完成登入".into());
                }
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).map_err(|e| e.to_string())?;
        let path = line.split_whitespace().nth(1).unwrap_or("");
        let url = reqwest::Url::parse(&format!("http://127.0.0.1{path}")).map_err(|e| e.to_string())?;
        if url.path() != CALLBACK {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            continue;
        }
        let get = |k: &str| url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned());
        let page = |text: &str| format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<!doctype html><meta charset=utf-8><title>OwnVoice Meet</title><p style=\"font:18px system-ui;margin:3em\">{text}</p>");
        if get("state").as_deref() != Some(state.as_str()) {
            let _ = stream.write_all(page("登入失敗：回應不符，請回 OwnVoice Meet 再試一次。").as_bytes());
            return Err("state mismatch".into());
        }
        if let Some(err) = get("error") {
            let _ = stream.write_all(page("沒有完成登入，可以關掉這個分頁。").as_bytes());
            return Err(format!("sign-in: {err} {}", get("error_description").unwrap_or_default()));
        }
        let code = get("code").ok_or("sign-in: no code")?;
        let _ = stream.write_all(page("已連上 ChatGPT，可以關掉這個分頁，回到 OwnVoice Meet。").as_bytes());
        return Ok((code, get("client_id")));
    }
}

/// Sign in through the browser (`open` opens the address). Reuses the client id issued last time.
pub async fn sign_in(client: &reqwest::Client, previous_client_id: Option<&str>, open: impl Fn(&str)) -> Result<Tokens, String> {
    let (auth, token) = endpoints(client).await?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("loopback: {e}"))?;
    let redirect = format!("http://127.0.0.1:{}{CALLBACK}", listener.local_addr().map_err(|e| e.to_string())?.port());
    let (state, nonce, verifier) = (random(), random(), random());
    let mut url = reqwest::Url::parse(&auth).map_err(|e| e.to_string())?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", previous_client_id.unwrap_or("dynamic_agent_client"))
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect)
            .append_pair("scope", SCOPES)
            .append_pair("resource", RESOURCE)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &challenge(&verifier));
        if previous_client_id.is_none() {
            q.append_pair("agent_name_hint", "OwnVoice Meet");
        }
    }
    open(url.as_str());
    let (code, issued) = tokio::task::spawn_blocking(move || wait_for_callback(listener, state, Duration::from_secs(300)))
        .await
        .map_err(|e| e.to_string())??;
    let client_id = issued.or(previous_client_id.map(str::to_owned)).ok_or("sign-in: no client_id")?;
    let body = form(&[
        ("grant_type", "authorization_code"),
        ("client_id", &client_id),
        ("code", &code),
        ("code_verifier", &verifier),
        ("redirect_uri", &redirect),
        ("resource", RESOURCE),
    ]);
    let v = token_request(client, &token, body).await?;
    // The nonce sent with this sign-in must come back in the ID token: the answer belongs to it.
    let id = v["id_token"].as_str().ok_or("ChatGPT 沒有回傳身分資料，請再登入一次")?;
    if id_claims(id)["nonce"].as_str() != Some(nonce.as_str()) {
        return Err("ChatGPT 的登入回應不符，請再登入一次".into());
    }
    tokens_from(&v, &client_id, None)
}

/// Only one refresh at a time: a refresh token works once, and translations run side by side.
static REFRESHING: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();

/// A usable access token for a request: the saved one, refreshed (and saved) when it is about to
/// expire.
pub async fn access(client: &reqwest::Client) -> Result<String, String> {
    let _one = REFRESHING.get_or_init(|| tokio::sync::Mutex::new(())).lock().await;
    let t = load().ok_or("還沒連上 ChatGPT：請到設定的「翻」按 Continue with ChatGPT")?;
    if !t.plan_allowed() {
        return Err("這次登入沒有同意使用 ChatGPT 方案，請到設定重新登入".into());
    }
    if !t.expired() {
        return Ok(t.access);
    }
    let fresh = refreshed(client, &t).await.map_err(|e| format!("ChatGPT 登入已失效，請到設定重新登入（{e}）"))?;
    save(&fresh)?;
    Ok(fresh.access)
}

/// A fresh access token when the saved one is about to expire; the caller saves the result.
pub async fn refreshed(client: &reqwest::Client, t: &Tokens) -> Result<Tokens, String> {
    if !t.expired() {
        return Ok(t.clone());
    }
    let (_, token) = endpoints(client).await?;
    let body = form(&[("grant_type", "refresh_token"), ("client_id", &t.client_id), ("refresh_token", &t.refresh), ("resource", RESOURCE)]);
    let v = token_request(client, &token, body).await?;
    tokens_from(&v, &t.client_id, Some(t))
}

/// The models this account may use (the measuring test lists them).
#[cfg(test)]
pub async fn models(client: &reqwest::Client, t: &Tokens) -> Result<Vec<String>, String> {
    let v: Value = client
        .get(format!("{RESOURCE}/models"))
        .bearer_auth(&t.access)
        .send()
        .await
        .map_err(|e| format!("network: {e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let list = v["models"].as_array().or(v["data"].as_array()).cloned().unwrap_or_default();
    Ok(list
        .iter()
        .filter(|m| m["visibility"].as_str().is_none_or(|x| x == "list"))
        .filter_map(|m| m["slug"].as_str().or(m["id"].as_str()).map(str::to_owned))
        .collect())
}

/// The Responses request for the ChatGPT plan: store false and stream true are required;
/// temperature and output limits are not accepted.
pub fn request_body(model: &str, effort: &str, instructions: &str, user: &str) -> Value {
    let mut body = json!({
        "model": model,
        "instructions": instructions,
        "input": [{ "role": "user", "content": user }],
        "store": false,
        "stream": true,
    });
    if !effort.is_empty() {
        body["reasoning"] = json!({ "effort": effort });
    }
    body
}

pub const URL: &str = RESPONSES_URL;

/// A refused request, in words the user can act on (errors-and-recovery in OpenAI's docs).
pub fn friendly_error(status: u16, body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let code = v["error"]["code"].as_str().unwrap_or("");
    match (status, code) {
        (_, "subscription_sharing_user_not_eligible") => "這個 ChatGPT 帳號不能用方案額度（需要 Go、Plus、Pro 或 Business；公司帳號要管理員開放），請改用 API key".into(),
        (_, "subscription_sharing_usage_limit_exceeded") => "ChatGPT 方案的用量到上限了：到 chatgpt.com/settings/usage 查看，或先改用 API key".into(),
        (401, _) => "ChatGPT 登入已失效，請到設定重新登入".into(),
        _ => {
            let msg = v["error"]["message"].as_str().unwrap_or(body);
            format!("{status} {code}: {}", msg.chars().take(300).collect::<String>())
        }
    }
}

/// One server-sent event of a Responses stream. Ok(true) once the response is complete.
pub fn event(ev: &Value, usage: &mut crate::usage::Tally, on_delta: &mut impl FnMut(&str)) -> Result<bool, String> {
    match ev["type"].as_str() {
        Some("response.output_text.delta") => {
            if let Some(d) = ev["delta"].as_str() {
                on_delta(d);
            }
        }
        Some("response.completed") => {
            let u = &ev["response"]["usage"];
            let cached = u["input_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0);
            usage.input = u["input_tokens"].as_u64().unwrap_or(0).saturating_sub(cached);
            usage.cached = cached;
            usage.output = u["output_tokens"].as_u64().unwrap_or(0);
            return Ok(true);
        }
        Some("response.failed") | Some("error") => {
            let e = if ev["type"] == "error" { ev } else { &ev["response"]["error"] };
            let code = e["code"].as_str().unwrap_or("failed");
            return Err(friendly_error(0, &json!({ "error": { "code": code, "message": e["message"] } }).to_string()));
        }
        Some("response.incomplete") => return Err("ChatGPT 沒有回完這一句".into()),
        _ => {}
    }
    Ok(false)
}

/// One streamed request outside the app's pipeline (the measuring test).
#[cfg(test)]
pub async fn stream(client: &reqwest::Client, t: &Tokens, model: &str, effort: &str, instructions: &str, user: &str, mut on_delta: impl FnMut(&str)) -> Result<(), String> {
    let resp = client
        .post(RESPONSES_URL)
        .bearer_auth(&t.access)
        .header("accept", "text/event-stream")
        .json(&request_body(model, effort, instructions, user))
        .send()
        .await
        .map_err(|e| format!("network: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(friendly_error(status.as_u16(), &resp.text().await.unwrap_or_default()));
    }
    let mut usage = crate::usage::Tally::default();
    let mut bytes = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = bytes.next().await {
        buf.extend_from_slice(&chunk.map_err(|e| format!("stream: {e}"))?);
        while let Some(end) = buf.iter().position(|&b| b == b'\n') {
            let line = String::from_utf8_lossy(&buf[..end]).trim_end_matches('\r').to_string();
            buf.drain(..=end);
            let Some(data) = line.strip_prefix("data:").map(str::trim_start) else { continue };
            let Ok(ev) = serde_json::from_str::<Value>(data) else { continue };
            if event(&ev, &mut usage, &mut on_delta)? {
                return Ok(());
            }
        }
    }
    Err("the stream ended before completion".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_errors_say_what_to_do() {
        let e = friendly_error(403, r#"{"error":{"code":"subscription_sharing_user_not_eligible","message":"x"}}"#);
        assert!(e.contains("不能用方案額度"), "{e}");
        assert!(friendly_error(429, r#"{"error":{"code":"subscription_sharing_usage_limit_exceeded"}}"#).contains("用量到上限"));
        assert!(friendly_error(401, "").contains("重新登入"));
        let body = request_body("gpt-6-luna", "none", "I", "U");
        assert_eq!((body["store"].as_bool(), body["stream"].as_bool()), (Some(false), Some(true)));
        assert!(body.get("temperature").is_none() && body.get("max_output_tokens").is_none());
    }

    #[test]
    fn a_responses_stream_gives_text_and_usage() {
        let mut usage = crate::usage::Tally::default();
        let mut text = String::new();
        let mut put = |d: &str| text.push_str(d);
        assert!(!event(&json!({"type":"response.output_text.delta","delta":"你好"}), &mut usage, &mut put).unwrap());
        let done = json!({"type":"response.completed","response":{"usage":{"input_tokens":120,"input_tokens_details":{"cached_tokens":20},"output_tokens":9}}});
        assert!(event(&done, &mut usage, &mut put).unwrap());
        assert_eq!(text, "你好");
        assert_eq!((usage.input, usage.cached, usage.output), (100, 20, 9));
        let failed = json!({"type":"response.failed","response":{"error":{"code":"subscription_sharing_usage_limit_exceeded","message":"limit"}}});
        assert!(event(&failed, &mut usage, &mut |_: &str| {}).unwrap_err().contains("用量到上限"));
    }

    #[test]
    fn the_id_token_gives_the_email_and_nonce() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"email":"a@b.c","nonce":"n1"}"#);
        let claims = id_claims(&format!("h.{payload}.s"));
        assert_eq!((claims["email"].as_str(), claims["nonce"].as_str()), (Some("a@b.c"), Some("n1")));
        assert!(id_claims("garbage").is_null());
    }

    #[test]
    fn pkce_challenge_is_base64url_sha256() {
        // Expected value from Python: base64.urlsafe_b64encode(hashlib.sha256(v).digest()).rstrip(b"=").
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wWlW-Mo2ytwI"), "grOeZkNXjCoCTieTozzAM2kHdOWP77HUm-pp0OJRf7s");
        assert_eq!(random().len(), 43);
        assert!(form(&[("a", "b c"), ("r", "http://127.0.0.1:1/x")]).contains("r=http%3A%2F%2F127.0.0.1%3A1%2Fx"));
    }

    const LINES: [&str; 12] = [
        "Thanks everyone for joining, let's get started with the quarterly review.",
        "Revenue grew eleven percent, mostly from the new enterprise contracts.",
        "Could you share your screen and walk us through the timeline?",
        "We had to push the migration back by two weeks because of the vendor delay.",
        "I think the bigger risk is the data retention policy in Europe.",
        "Does anyone have questions before we move on to hiring?",
        "We're planning to open three positions in the platform team next month.",
        "The budget for the conference trip has been approved.",
        "Let's make sure the documentation is ready before the launch.",
        "Can we set up a follow-up meeting on Thursday afternoon?",
        "One open question is who owns the support rotation after launch.",
        "Great, thanks everyone, I'll send the notes after the call.",
    ];

    fn median(mut v: Vec<u128>) -> u128 {
        v.sort_unstable();
        v.get(v.len() / 2).copied().unwrap_or(0)
    }

    // The prototype: sign in with ChatGPT (browser, once), then translate the same caption lines
    // through the ChatGPT plan and through the API key, on gpt-6-luna, and compare.
    //   cargo test --lib -- --ignored --nocapture chatgpt_probe
    #[tokio::test]
    #[ignore]
    async fn chatgpt_probe() {
        let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../.env.local"));
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::new();
        let mut t = match load() {
            Some(t) => t,
            None => {
                println!("Opening the browser to sign in with ChatGPT...");
                let t = sign_in(&client, None, |u| {
                    // No secret in it (state, nonce and the PKCE challenge only).
                    println!("If the browser did not come up, open this address:\n{u}\n");
                    #[cfg(windows)]
                    let _ = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", u]).spawn();
                    #[cfg(not(windows))]
                    let _ = std::process::Command::new("open").arg(u).spawn();
                })
                .await
                .expect("sign in");
                if let Err(e) = save(&t) { println!("(not kept in the keychain: {e})"); }
                t
            }
        };
        t = refreshed(&client, &t).await.expect("refresh");
        if let Err(e) = save(&t) { println!("(not kept in the keychain: {e})"); }
        println!("signed in; plan usage allowed: {}; scopes: {}", t.plan_allowed(), t.scopes.join(" "));
        assert!(t.plan_allowed(), "the sign-in did not grant {PLAN_SCOPE}");
        let list = models(&client, &t).await.unwrap_or_else(|e| vec![format!("(models failed: {e})")]);
        println!("models for this account: {}", list.join(", "));
        let model = list.iter().find(|m| m.contains("luna")).cloned().unwrap_or_else(|| "gpt-6-luna".into());
        println!("using {model}\n");

        let key = std::env::var("OPENAI_API_KEY").ok().filter(|k| !k.is_empty());
        let api = key.as_ref().map(|k| crate::llm::tests_target("gpt-6-luna", k));
        let (mut plan_first, mut plan_total, mut api_first, mut api_total) = (vec![], vec![], vec![], vec![]);
        let mut failures = 0;
        for line in LINES {
            let user = crate::prompts::listen_user(&[], line, false);
            let t0 = std::time::Instant::now();
            let mut first = None;
            let mut zh = String::new();
            match stream(&client, &t, &model, "none", crate::prompts::LISTEN_SYSTEM, &user, |d| {
                first.get_or_insert(t0.elapsed().as_millis());
                zh.push_str(d);
            })
            .await
            {
                Ok(()) => {
                    plan_first.push(first.unwrap_or(0));
                    plan_total.push(t0.elapsed().as_millis());
                }
                Err(e) => {
                    failures += 1;
                    println!("plan ERROR {e}");
                }
            }
            let plan_line = format!("plan {:>5}ms {:>5}ms {zh}", first.unwrap_or(0), t0.elapsed().as_millis());
            let mut api_line = String::from("api   (no OPENAI_API_KEY)");
            if let Some(a) = &api {
                let t1 = std::time::Instant::now();
                let mut f2 = None;
                let mut zh2 = String::new();
                let system = crate::prompts::system_blocks(crate::prompts::LISTEN_SYSTEM, &[], "");
                if crate::llm::stream(&client, a, system, user.clone(), |d| {
                    f2.get_or_insert(t1.elapsed().as_millis());
                    zh2.push_str(d);
                })
                .await
                .is_ok()
                {
                    api_first.push(f2.unwrap_or(0));
                    api_total.push(t1.elapsed().as_millis());
                }
                api_line = format!("api  {:>5}ms {:>5}ms {zh2}", f2.unwrap_or(0), t1.elapsed().as_millis());
            }
            println!("{line}\n  {plan_line}\n  {api_line}");
        }
        println!(
            "\nmedian first text / whole line: ChatGPT plan {}ms / {}ms ({} ok, {failures} failed); API key {}ms / {}ms ({} ok)",
            median(plan_first.clone()),
            median(plan_total.clone()),
            plan_total.len(),
            median(api_first),
            median(api_total.clone()),
            api_total.len()
        );
    }
}
