// Speech models that run on this computer (sherpa-onnx): which files each feature needs, where they
// come from, and their SHA-256 digests. They are downloaded the first time a feature is turned on,
// never shipped with the app, and a file is used only after its digest matches.

use std::io::Write as _;
use std::path::PathBuf;

use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::settings;

pub struct ModelFile {
    /// Path under the models folder.
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

/// Speaker labels (speakers.rs): Silero VAD v5 and CAM++ (3D-Speaker, English VoxCeleb). 31 MB.
pub const SPEAKERS: [ModelFile; 2] = [
    ModelFile {
        name: "silero_vad_v5.onnx",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad_v5.onnx",
        sha256: "6b99cbfd39246b6706f98ec13c7c50c6b299181f2474fa05cbc8046acc274396",
        bytes: 2_313_101,
    },
    ModelFile {
        name: "3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx",
        sha256: "98daf9bcf1c8abf3db374fec4c27a5eda7c58705bb3f180d057b085a72a88bb5",
        bytes: 29_596_979,
    },
];

/// Local speech recognition (local_stt.rs): NVIDIA Parakeet TDT 0.6B v3 (English and other European
/// languages, CC BY 4.0) and SenseVoice (Chinese, Japanese, Korean, Cantonese; the 2024-07-17 int8
/// export: the 2025-09-09 one named English "yue" and garbled Japanese and Korean). 910 MB.
pub const LOCAL_STT: [ModelFile; 6] = [
    ModelFile { name: "parakeet-v3/encoder.int8.onnx", url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/encoder.int8.onnx", sha256: "1afbe69c1ffdabee94755f42e4daf1d93549131a3da11356bda84fcef0f2c479", bytes: 652_282_294 },
    ModelFile { name: "parakeet-v3/decoder.int8.onnx", url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/decoder.int8.onnx", sha256: "0222a86147299d4dee90edf95b6d13827edb42ad8b7c9cf0b2582f030937c0b5", bytes: 11_845_274 },
    ModelFile { name: "parakeet-v3/joiner.int8.onnx", url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/joiner.int8.onnx", sha256: "cbd02f3696e9944970111fb439d9c3fd365a80374565f13f2b9c7007d253fc98", bytes: 6_355_277 },
    ModelFile { name: "parakeet-v3/tokens.txt", url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/tokens.txt", sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d", bytes: 93_939 },
    ModelFile { name: "sensevoice/model.int8.onnx", url: "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/model.int8.onnx", sha256: "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51", bytes: 239_233_841 },
    ModelFile { name: "sensevoice/tokens.txt", url: "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/tokens.txt", sha256: "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc", bytes: 315_894 },
];

/// Local translation (local_llm.rs): Qwen3 1.7B (Apache-2.0), Q4_K_M GGUF quantized by Unsloth. 1.1 GB.
pub const LOCAL_LLM: [ModelFile; 1] = [ModelFile {
    name: "qwen3/Qwen3-1.7B-Q4_K_M.gguf",
    url: "https://huggingface.co/unsloth/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q4_K_M.gguf",
    sha256: "b139949c5bd74937ad8ed8c8cf3d9ffb1e99c866c823204dc42c0d91fa181897",
    bytes: 1_107_409_472,
}];

pub fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(settings::config_dir(app)?.join("models"))
}

pub fn path(app: &AppHandle, f: &ModelFile) -> Result<String, String> {
    Ok(dir(app)?.join(f.name).to_string_lossy().into_owned())
}

/// Every file is here, whole.
pub fn ready(app: &AppHandle, files: &[ModelFile]) -> bool {
    let Ok(d) = dir(app) else { return false };
    files.iter().all(|f| std::fs::metadata(d.join(f.name)).is_ok_and(|m| m.len() == f.bytes))
}

#[derive(Clone, Serialize)]
struct Progress {
    done: u64,
    total: u64,
}

/// Downloads what is missing, reporting progress as `event`; a file is kept only when its digest
/// matches.
pub async fn download(app: &AppHandle, client: &reqwest::Client, files: &[ModelFile], event: &str) -> Result<(), String> {
    let d = dir(app)?;
    let total: u64 = files.iter().map(|f| f.bytes).sum();
    let mut done = 0u64;
    for f in files {
        let target = d.join(f.name);
        if std::fs::metadata(&target).is_ok_and(|m| m.len() == f.bytes) {
            done += f.bytes;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let resp = client.get(f.url).send().await.map_err(|e| format!("下載模型失敗：{e}"))?;
        if !resp.status().is_success() {
            return Err(format!("下載模型失敗：{}", resp.status()));
        }
        let part = target.with_extension("part");
        let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
        let mut bytes = resp.bytes_stream();
        let mut last = 0u64;
        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(|e| format!("下載模型失敗：{e}"))?;
            digest.update(&chunk);
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            done += chunk.len() as u64;
            if done - last >= 1 << 20 || done == total {
                last = done;
                let _ = app.emit(event, Progress { done, total });
            }
        }
        drop(file);
        let got: String = digest.finish().as_ref().iter().map(|b| format!("{b:02x}")).collect();
        if got != f.sha256 {
            let _ = std::fs::remove_file(&part);
            return Err(format!("{} 下載後核對不符，已刪除，請再試一次", f.name));
        }
        std::fs::rename(&part, &target).map_err(|e| e.to_string())?;
    }
    let _ = app.emit(event, Progress { done: total, total });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GH: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download";
    const HF_PARAKEET: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main";
    const HF_SENSEVOICE: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main";

    #[test]
    fn every_model_comes_from_its_release_by_its_own_name() {
        for f in SPEAKERS.iter() {
            assert_eq!(f.url, format!("{GH}/{}", if f.name.starts_with("silero") { "asr-models/silero_vad_v5.onnx".to_string() } else { format!("speaker-recongition-models/{}", f.name) }));
        }
        for f in LOCAL_STT.iter() {
            let (base, file) = f.name.split_once('/').unwrap();
            let hf = if base == "parakeet-v3" { HF_PARAKEET } else { HF_SENSEVOICE };
            assert_eq!(f.url, format!("{hf}/{file}"), "{}", f.name);
        }
    }
}
