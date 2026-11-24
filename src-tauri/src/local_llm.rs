// Translation on this computer, free and offline: Qwen3 1.7B (Apache-2.0, Q4_K_M GGUF) run by
// llama.cpp. One thread owns the model and its context; requests queue up and are answered one at a
// time, streamed, and an early translation that is no longer wanted can stop mid-way.
//
// Measured with the user's roundtable sentences (2026-11-24, CPU only, 4 threads of an i5-12400):
// about 1.9 s per caption line, the meaning mostly right with an occasional slip; Opus-MT, a
// dedicated translation model, looped on spoken fragments ("呃,呃,呃…") and was dropped. Qwen3
// thinks before it answers unless told not to: the prompt opens an empty think block itself.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex, OnceLock};

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use tokio::sync::mpsc;

use crate::usage::Tally;

const N_CTX: u32 = 4096;
/// Room kept for the answer.
const MAX_NEW: usize = 400;

/// The model file, set at start from the models folder (models.rs).
static MODEL_PATH: OnceLock<PathBuf> = OnceLock::new();
static BACKEND: OnceLock<Result<LlamaBackend, String>> = OnceLock::new();
/// The worker's queue while the model is loaded.
static WORKER: Mutex<Option<std_mpsc::Sender<Job>>> = Mutex::new(None);

pub fn set_model_path(path: PathBuf) {
    let _ = MODEL_PATH.set(path);
}

struct Job {
    /// The system blocks in order; the later ones (the script) are left out when the prompt is too long.
    system: Vec<String>,
    user: String,
    out: mpsc::UnboundedSender<Result<String, String>>,
    stop: Arc<AtomicBool>,
    usage: Arc<Mutex<Tally>>,
}

/// Qwen3's chat format, with the thinking turned off by an empty think block.
fn prompt(system: &str, user: &str) -> String {
    format!("<|im_start|>system\n{system}<|im_end|>\n<|im_start|>user\n{user}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n")
}

fn threads() -> i32 {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    (n / 2).clamp(2, 4) as i32
}

fn worker(rx: std_mpsc::Receiver<Job>, ready: std_mpsc::Sender<Result<(), String>>) {
    let backend = match BACKEND.get_or_init(|| LlamaBackend::init().map_err(|e| e.to_string())) {
        Ok(b) => b,
        Err(e) => {
            let _ = ready.send(Err(e.clone()));
            return;
        }
    };
    let Some(path) = MODEL_PATH.get() else {
        let _ = ready.send(Err("本機翻譯的模型位置還沒設定".into()));
        return;
    };
    let model = match LlamaModel::load_from_file(backend, path, &LlamaModelParams::default()) {
        Ok(m) => m,
        Err(e) => {
            let _ = ready.send(Err(format!("本機翻譯模型載入失敗：{e}")));
            return;
        }
    };
    let params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(N_CTX))
        .with_n_batch(N_CTX)
        .with_n_threads(threads())
        .with_n_threads_batch(threads());
    let mut ctx = match model.new_context(backend, params) {
        Ok(c) => c,
        Err(e) => {
            let _ = ready.send(Err(format!("本機翻譯模型載入失敗：{e}")));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let vocab = model.vocab();
    // The prompt the cache holds: the system prompt (and the script) is the same from one caption
    // line to the next, so only what differs is computed again.
    let mut cached: Vec<llama_cpp_2::token::LlamaToken> = Vec::new();
    for job in rx {
        // Leave out later system blocks until the prompt fits with room for the answer.
        let mut keep = job.system.len();
        let tokens = loop {
            let text = prompt(&job.system[..keep].join("\n\n"), &job.user);
            let t = vocab.tokenize(text.as_bytes(), false, true);
            if t.len() + MAX_NEW <= N_CTX as usize || keep <= 1 {
                break t;
            }
            keep -= 1;
        };
        if tokens.len() + MAX_NEW > N_CTX as usize {
            let _ = job.out.send(Err("這段內容太長，本機翻譯放不下".into()));
            continue;
        }
        // Keep the part of the cache that matches this prompt (at least one token is computed, for
        // its logits), drop the rest, the previous answer included.
        let common = cached.iter().zip(&tokens).take_while(|(a, b)| a == b).count().min(tokens.len() - 1);
        if ctx.clear_kv_cache_seq(Some(0), Some(common as u32), None).is_err() {
            ctx.clear_kv_cache();
        }
        let mut batch = LlamaBatch::new(N_CTX as usize, 1);
        let last = tokens.len() as i32 - 1;
        for (i, t) in tokens.iter().enumerate().skip(common) {
            let _ = batch.add(*t, i as i32, &[0], i as i32 == last);
        }
        cached = tokens.clone();
        if let Err(e) = ctx.decode(&mut batch) {
            ctx.clear_kv_cache();
            cached.clear();
            let _ = job.out.send(Err(format!("本機翻譯失敗：{e}")));
            continue;
        }
        let mut sampler = LlamaSampler::greedy();
        let mut pos = tokens.len() as i32;
        let mut bytes: Vec<u8> = Vec::new();
        let mut produced = 0u64;
        for _ in 0..MAX_NEW {
            if job.stop.load(Ordering::Relaxed) || job.out.is_closed() {
                break;
            }
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);
            if vocab.is_eog(token) {
                break;
            }
            produced += 1;
            bytes.extend(vocab.token_to_piece(token, false, None));
            // A Chinese character can come in two tokens: send only whole characters.
            let valid = match std::str::from_utf8(&bytes) {
                Ok(s) => s.len(),
                Err(e) => e.valid_up_to(),
            };
            if valid > 0 {
                let piece = String::from_utf8_lossy(&bytes[..valid]).into_owned();
                bytes.drain(..valid);
                let _ = job.out.send(Ok(piece));
            }
            batch.clear();
            let _ = batch.add(token, pos, &[0], true);
            pos += 1;
            if let Err(e) = ctx.decode(&mut batch) {
                let _ = job.out.send(Err(format!("本機翻譯失敗：{e}")));
                break;
            }
        }
        if let Ok(mut u) = job.usage.lock() {
            u.input = tokens.len() as u64;
            u.output = produced;
        }
    }
}

/// The worker's queue, loading the model the first time (a second or two).
fn queue() -> Result<std_mpsc::Sender<Job>, String> {
    let mut w = WORKER.lock().map_err(|_| "本機翻譯狀態異常")?;
    if let Some(tx) = w.as_ref() {
        return Ok(tx.clone());
    }
    let (tx, rx) = std_mpsc::channel::<Job>();
    let (ready_tx, ready_rx) = std_mpsc::channel();
    std::thread::spawn(move || worker(rx, ready_tx));
    ready_rx.recv().map_err(|_| "本機翻譯沒有啟動".to_string())??;
    *w = Some(tx.clone());
    Ok(tx)
}

/// Lets the model go (listening stopped): the worker ends after the requests already queued.
pub fn unload() {
    if let Ok(mut w) = WORKER.lock() {
        *w = None;
    }
}

/// One answer, streamed into `on_delta`; `stop` ends it early (an early translation overtaken).
pub async fn stream(system: Vec<String>, user: String, mut on_delta: impl FnMut(&str), stop: impl Fn() -> bool) -> Result<Tally, String> {
    let tx = tokio::task::spawn_blocking(queue).await.map_err(|e| e.to_string())??;
    let (out, mut rx) = mpsc::unbounded_channel();
    let halt = Arc::new(AtomicBool::new(false));
    let usage = Arc::new(Mutex::new(Tally::default()));
    tx.send(Job { system, user, out, stop: halt.clone(), usage: usage.clone() }).map_err(|_| "本機翻譯已停止".to_string())?;
    while let Some(piece) = rx.recv().await {
        on_delta(&piece?);
        if stop() {
            halt.store(true, Ordering::Relaxed);
            break;
        }
    }
    let u = usage.lock().map(|u| u.clone()).unwrap_or_default();
    Ok(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_turns_thinking_off() {
        let p = prompt("S", "U");
        assert!(p.starts_with("<|im_start|>system\nS<|im_end|>"));
        assert!(p.ends_with("<|im_start|>assistant\n<think>\n\n</think>\n\n"));
    }

    // The model through this module, with the app's caption prompt (model file in the models
    // folder given by OWNVOICE_LLM_MODEL):
    //   OWNVOICE_LLM_MODEL=<path to Qwen3-1.7B-Q4_K_M.gguf> cargo test --lib -- --ignored --nocapture local_llm_live
    #[tokio::test]
    #[ignore]
    async fn local_llm_live() {
        let Some(path) = std::env::var_os("OWNVOICE_LLM_MODEL") else {
            println!("OWNVOICE_LLM_MODEL not set; skipped");
            return;
        };
        set_model_path(PathBuf::from(path));
        for en in ["Students were saying that in essence we do not want our teachers to become cyborgs.", "Uh previous my main point. Uh then uh so", "Revenue grew eleven percent, mostly from the new enterprise contracts.", "Can we set up a follow-up meeting on Thursday afternoon?"] {
            let t0 = std::time::Instant::now();
            let mut zh = String::new();
            let mut first = None;
            let u = stream(vec![crate::prompts::LISTEN_SYSTEM.to_string()], crate::prompts::listen_user(&[], en, false), |d| {
                first.get_or_insert(t0.elapsed());
                zh.push_str(d);
            }, || false)
            .await
            .expect("local translation");
            println!("{:>5}ms first {:>4}ms in {} out {} | {en} → {zh}", t0.elapsed().as_millis(), first.map_or(0, |d| d.as_millis()), u.input, u.output);
            assert!(zh.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)), "Chinese expected: {zh}");
            assert!(!zh.contains("<think>"), "thinking left on");
        }
    }
}
