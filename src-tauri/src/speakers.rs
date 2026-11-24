// Live speaker labels ("講者 1", "講者 2", …), worked out on this computer from voice prints, with
// sherpa-onnx: silero VAD cuts the sound into stretches of speech, CAM++ (3D-Speaker, trained on
// English VoxCeleb) turns each stretch into a voice print, and a stretch joins the closest speaker
// met so far or starts a new one. Nothing is sent anywhere; it works with every speech service that
// hears our own audio (not Windows Live Captions, which hears the sound itself).
//
// Checked on a real 47-minute roundtable recorded through a laptop microphone (2026-11-24): the
// plain approach of sherpa-onnx's example found 61 to 124 "speakers"; with the remedies here
// (threshold 0.4, stretches under 2 s keep the current speaker, each speaker's print the running
// mean of its stretches) it found the six people who spoke, in the programme's order. Short
// interjections are counted with the speaker before them.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::Tap;
use crate::models::{self, SPEAKERS};
use crate::settings;

const RATE: u32 = 16_000;
/// How alike a stretch must be to a known speaker's print (cosine similarity) to join them.
const THRESHOLD: f32 = 0.4;
/// A stretch shorter than this is too short to found a new speaker: it keeps the one talking.
const MIN_NEW: f32 = 2.0;
/// A finished caption line looks for the voice print of its stretch this often, this many times:
/// a stretch is only printed when it ends (a pause, or MAX_STRETCH), which can be well after the
/// line's words are final. In the first check on a real recording, waiting 4.5 s left 9 of 27
/// lines without a speaker.
const LABEL_EVERY: Duration = Duration::from_millis(1500);
const LABEL_TRIES: usize = 10;
/// Longer speech is cut here, so a speaker who never pauses is still printed often.
const MAX_STRETCH: f32 = 10.0;
/// Stretches kept for matching caption lines.
const KEEP: usize = 400;

struct Stretch {
    start: Instant,
    end: Instant,
    who: usize,
}

#[derive(Default)]
struct Inner {
    feed: Option<mpsc::Sender<Vec<i16>>>,
    stretches: VecDeque<Stretch>,
    names: Vec<String>,
    /// Bumped by every begin, so a worker from an earlier session cannot write into this one.
    session: u64,
}

#[derive(Default)]
pub struct Speakers(Mutex<Inner>);

/// Starts labelling for a new listening session when it is on and the models are here; returns the
/// audio tap to use (the recording's, with the voice prints fed alongside).
pub fn begin(app: &AppHandle, rate: u32, record: Option<Tap>) -> Option<Tap> {
    let s = settings::load(app);
    if !s.speaker_labels || !models::ready(app, &SPEAKERS) {
        return record;
    }
    let (tx, rx) = mpsc::channel::<Vec<i16>>();
    let session = {
        let state = app.state::<Speakers>();
        let Ok(mut inner) = state.0.lock() else { return record };
        inner.session += 1;
        inner.feed = Some(tx.clone());
        inner.stretches.clear();
        inner.names.clear();
        inner.session
    };
    let worker_app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = work(&worker_app, rate, rx, session) {
            crate::captions::status(&worker_app, format!("標講者沒有啟動：{e}"));
        }
    });
    Some(Box::new(move |chunk: Vec<i16>| {
        if let Some(r) = &record {
            r(chunk.clone());
        }
        let _ = tx.send(chunk);
    }))
}

/// Listening ended: the worker finishes when its feed closes.
pub fn end(app: &AppHandle) {
    if let Ok(mut inner) = app.state::<Speakers>().0.lock() {
        inner.feed = None;
    }
}

fn to_16k(chunk: &[i16], rate: u32) -> Vec<f32> {
    let f: Vec<f32> = chunk.iter().map(|&s| s as f32 / 32768.0).collect();
    if rate == RATE || f.is_empty() {
        return f;
    }
    let n = (f.len() as u64 * RATE as u64 / rate as u64) as usize;
    (0..n)
        .map(|i| {
            let x = i as f64 * rate as f64 / RATE as f64;
            let j = x as usize;
            let t = (x - j as f64) as f32;
            let a = f[j.min(f.len() - 1)];
            let b = f[(j + 1).min(f.len() - 1)];
            a + (b - a) * t
        })
        .collect()
}

/// Voice prints and the running clusters: which speaker a stretch belongs to.
#[derive(Default)]
struct Clusters {
    means: Vec<Vec<f32>>,
    counts: Vec<f32>,
    prev: Option<usize>,
}

impl Clusters {
    fn assign(&mut self, print: &[f32], secs: f32) -> usize {
        let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
        let e: Vec<f32> = {
            let n = norm(print);
            print.iter().map(|x| x / n).collect()
        };
        let best = self
            .means
            .iter()
            .enumerate()
            .map(|(k, m)| (k, m.iter().zip(&e).map(|(a, b)| a * b).sum::<f32>() / norm(m)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let who = match (best, self.prev) {
            (Some((k, sim)), _) if sim >= THRESHOLD => k,
            (_, Some(p)) if secs < MIN_NEW => p,
            _ => {
                self.means.push(vec![0.0; e.len()]);
                self.counts.push(0.0);
                self.means.len() - 1
            }
        };
        let c = self.counts[who];
        for (m, x) in self.means[who].iter_mut().zip(&e) {
            *m = (*m * c + x) / (c + 1.0);
        }
        self.counts[who] = c + 1.0;
        self.prev = Some(who);
        who
    }
}

fn work(app: &AppHandle, rate: u32, rx: mpsc::Receiver<Vec<i16>>, session: u64) -> Result<(), String> {
    let mut vc = sherpa_onnx::VadModelConfig::default();
    vc.silero_vad.model = Some(models::path(app, &SPEAKERS[0])?);
    vc.silero_vad.threshold = 0.5;
    vc.silero_vad.min_silence_duration = 0.4;
    vc.silero_vad.min_speech_duration = 0.25;
    vc.silero_vad.max_speech_duration = MAX_STRETCH;
    vc.silero_vad.window_size = 512;
    vc.sample_rate = RATE as i32;
    vc.num_threads = 1;
    let vad = sherpa_onnx::VoiceActivityDetector::create(&vc, 60.0).ok_or("VAD 模型載入失敗")?;
    let ec = sherpa_onnx::SpeakerEmbeddingExtractorConfig {
        model: Some(models::path(app, &SPEAKERS[1])?),
        num_threads: 1,
        ..Default::default()
    };
    let extractor = sherpa_onnx::SpeakerEmbeddingExtractor::create(&ec).ok_or("聲紋模型載入失敗")?;
    let mut clusters = Clusters::default();
    let mut pending: Vec<f32> = Vec::new();
    let mut base: Option<Instant> = None;
    for chunk in rx {
        let samples = to_16k(&chunk, rate);
        // Sample n of this session was heard at base + n / 16 kHz.
        let base = *base.get_or_insert_with(|| Instant::now() - Duration::from_secs_f64(samples.len() as f64 / RATE as f64));
        pending.extend_from_slice(&samples);
        let whole = pending.len() / 512 * 512;
        for w in pending[..whole].chunks(512) {
            vad.accept_waveform(w);
        }
        pending.drain(..whole);
        while let Some(seg) = vad.front() {
            let (start, n) = (seg.start().max(0) as f64, seg.n().max(0) as f64);
            let print = extractor.create_stream().and_then(|s| {
                s.accept_waveform(RATE as i32, seg.samples());
                s.input_finished();
                extractor.compute(&s)
            });
            vad.pop();
            let Some(print) = print else { continue };
            let who = clusters.assign(&print, (n / RATE as f64) as f32);
            let at = |s: f64| base + Duration::from_secs_f64(s / RATE as f64);
            let state = app.state::<Speakers>();
            let Ok(mut inner) = state.0.lock() else { continue };
            if inner.session != session {
                return Ok(());
            }
            while inner.names.len() <= who {
                let n = inner.names.len() + 1;
                inner.names.push(format!("講者 {n}"));
            }
            inner.stretches.push_back(Stretch { start: at(start), end: at(start + n), who });
            if inner.stretches.len() > KEEP {
                inner.stretches.pop_front();
            }
        }
    }
    Ok(())
}

/// The speaker of the stretch that overlaps `from..to` the most.
fn lookup(app: &AppHandle, from: Instant, to: Instant) -> Option<(usize, String)> {
    let state = app.state::<Speakers>();
    let inner = state.0.lock().ok()?;
    let overlap = |s: &Stretch| {
        let a = s.start.max(from);
        let b = s.end.min(to);
        b.saturating_duration_since(a)
    };
    let best = inner.stretches.iter().filter(|s| overlap(s) > Duration::ZERO).max_by_key(|s| overlap(s))?;
    Some((best.who, inner.names.get(best.who).cloned().unwrap_or_else(|| format!("講者 {}", best.who + 1))))
}

/// Labels caption line `id`, heard from `from` to `to`, once its voice print is in.
pub fn label_later(app: &AppHandle, id: u64, from: Instant, to: Instant) {
    let active = app.state::<Speakers>().0.lock().is_ok_and(|i| i.feed.is_some());
    if !active {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        // The words appear a moment after the speaker starts: look a little earlier.
        let from = from.checked_sub(Duration::from_millis(1500)).unwrap_or(from);
        for _ in 0..LABEL_TRIES {
            std::thread::sleep(LABEL_EVERY);
            if let Some((who, name)) = lookup(&app, from, to) {
                crate::captions::set_speaker(&app, id, who, &name);
                return;
            }
        }
    });
}

/// Names the user gave speakers, by number (the captions window shows the change at once).
#[derive(Clone, Serialize)]
pub struct Renamed {
    pub who: usize,
    pub name: String,
}

pub fn rename(app: &AppHandle, who: usize, name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("名字不能是空的".into());
    }
    let old = {
        let state = app.state::<Speakers>();
        let mut inner = state.0.lock().map_err(|_| "speaker state is poisoned")?;
        while inner.names.len() <= who {
            let n = inner.names.len() + 1;
            inner.names.push(format!("講者 {n}"));
        }
        std::mem::replace(&mut inner.names[who], name.to_string())
    };
    crate::captions::rename_speaker(app, who, &old, name);
    let _ = app.emit("speaker-renamed", Renamed { who, name: name.to_string() });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stretches_join_the_closest_speaker_or_start_one() {
        let mut c = Clusters::default();
        let a = [1.0, 0.0, 0.0];
        let b = [0.0, 1.0, 0.0];
        assert_eq!(c.assign(&a, 5.0), 0);
        assert_eq!(c.assign(&[0.9, 0.1, 0.0], 5.0), 0, "close to the first speaker");
        assert_eq!(c.assign(&b, 5.0), 1, "a different voice starts speaker 2");
        assert_eq!(c.assign(&a, 5.0), 0);
        // A short stretch that matches no one keeps the speaker talking instead of founding one.
        assert_eq!(c.assign(&[0.0, 0.0, 1.0], 1.0), 0);
        assert_eq!(c.means.len(), 2);
        assert_eq!(c.assign(&[0.0, 0.0, 1.0], 3.0), 2, "a long one does found a speaker");
    }

    #[test]
    fn audio_is_brought_to_16_khz() {
        assert_eq!(to_16k(&[0; 2400], 24_000).len(), 1600);
        assert_eq!(to_16k(&[16384; 1600], 16_000)[0], 0.5);
    }
}
