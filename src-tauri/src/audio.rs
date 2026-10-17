// System-audio capture: the default output device recorded in WASAPI loopback mode (cpal turns an
// input stream on an output device into loopback), downmixed and resampled to mono 16-bit PCM at
// the rate a provider asks for (Gemini Live 16 kHz, OpenAI realtime 24 kHz), in 100 ms chunks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as std_mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use tokio::sync::mpsc;

/// Samples in a 100 ms chunk at `rate`.
pub fn chunk_len(rate: u32) -> usize {
    rate as usize / 10
}

pub struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub device: String,
}

impl Capture {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Start capturing. Chunks go to `tx` (dropped when the receiver lags, so audio never piles up);
/// `on_level` gets each chunk's RMS for the level meter.
pub fn start(rate: u32, tx: mpsc::Sender<Vec<i16>>, on_level: impl Fn(f32) + Send + 'static) -> Result<Capture, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = std_mpsc::channel::<Result<String, String>>();
    let stop_t = stop.clone();
    // cpal streams are not Send on every platform: the stream lives and dies on its own thread.
    let thread = std::thread::spawn(move || {
        let stream = match open(rate, tx, on_level) {
            Ok((stream, name)) => {
                let _ = ready_tx.send(Ok(name));
                stream
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        while !stop_t.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(stream);
    });
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(device)) => Ok(Capture { stop, thread: Some(thread), device }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("audio device did not start in time".into()),
    }
}

fn open(rate: u32, tx: mpsc::Sender<Vec<i16>>, on_level: impl Fn(f32) + Send + 'static) -> Result<(cpal::Stream, String), String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no default output device")?;
    let name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "output".into());
    let supported = device.default_output_config().map_err(|e| format!("output config: {e}"))?;
    let format = supported.sample_format();
    let config = supported.config();
    let mut proc = Downmix::new(config.channels as usize, config.sample_rate, rate, tx, on_level);
    let on_err = |e| eprintln!("audio stream error: {e}");
    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| proc.feed(data.iter().copied()),
            on_err,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| proc.feed(data.iter().map(|&s| s as f32 / 32768.0)),
            on_err,
            None,
        ),
        other => return Err(format!("unsupported sample format {other:?}")),
    }
    .map_err(|e| format!("loopback stream: {e}"))?;
    stream.play().map_err(|e| format!("start stream: {e}"))?;
    Ok((stream, name))
}

/// Interleaved frames in, mono i16 chunks at the target rate out. A box average over one input period per
/// output sample keeps aliasing down well enough for speech; linear interpolation does the rest.
struct Downmix<L: Fn(f32)> {
    channels: usize,
    step: f64,
    pos: f64,
    prev: f32,
    window: Vec<f32>,
    win_i: usize,
    win_sum: f32,
    chunk_len: usize,
    chunk: Vec<i16>,
    tx: mpsc::Sender<Vec<i16>>,
    on_level: L,
}

impl<L: Fn(f32)> Downmix<L> {
    fn new(channels: usize, in_rate: u32, out_rate: u32, tx: mpsc::Sender<Vec<i16>>, on_level: L) -> Self {
        let step = in_rate as f64 / out_rate as f64;
        let win = (step.round() as usize).max(1);
        Self {
            channels: channels.max(1),
            step,
            pos: 0.0,
            prev: 0.0,
            window: vec![0.0; win],
            win_i: 0,
            win_sum: 0.0,
            chunk_len: chunk_len(out_rate),
            chunk: Vec::with_capacity(chunk_len(out_rate)),
            tx,
            on_level,
        }
    }

    fn feed(&mut self, samples: impl Iterator<Item = f32>) {
        let mut acc = 0.0;
        let mut n = 0;
        for s in samples {
            acc += s;
            n += 1;
            if n == self.channels {
                self.push(acc / self.channels as f32);
                acc = 0.0;
                n = 0;
            }
        }
    }

    fn push(&mut self, x: f32) {
        // moving average over roughly one output period
        self.win_sum += x - self.window[self.win_i];
        self.window[self.win_i] = x;
        self.win_i = (self.win_i + 1) % self.window.len();
        let x = self.win_sum / self.window.len() as f32;
        while self.pos <= 1.0 {
            let y = self.prev + (x - self.prev) * self.pos as f32;
            self.emit(y);
            self.pos += self.step;
        }
        self.pos -= 1.0;
        self.prev = x;
    }

    fn emit(&mut self, y: f32) {
        self.chunk.push((y.clamp(-1.0, 1.0) * 32767.0) as i16);
        if self.chunk.len() >= self.chunk_len {
            let chunk = std::mem::replace(&mut self.chunk, Vec::with_capacity(self.chunk_len));
            let rms = (chunk.iter().map(|&s| (s as f32 / 32768.0).powi(2)).sum::<f32>() / chunk.len() as f32).sqrt();
            (self.on_level)(rms);
            let _ = self.tx.try_send(chunk);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsamples_48k_stereo_to_16k_chunks() {
        let (tx, mut rx) = mpsc::channel(16);
        let mut d = Downmix::new(2, 48_000, 16_000, tx, |_| {});
        // 0.2 s of a 440 Hz tone, stereo
        let frames: Vec<f32> = (0..9_600).flat_map(|i| {
            let v = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5;
            [v, v]
        }).collect();
        d.feed(frames.into_iter());
        let a = rx.try_recv().expect("first 100 ms chunk");
        let b = rx.try_recv().expect("second 100 ms chunk");
        assert_eq!(a.len(), 1_600);
        assert_eq!(b.len(), 1_600);
        let peak = b.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(peak > 12_000 && peak < 17_000, "tone survives downsampling, peak {peak}");
    }
}

#[cfg(test)]
mod hardware {
    use super::*;

    // Needs speakers/headphones as the default output: cargo test --lib -- --ignored loopback
    #[test]
    #[ignore]
    fn loopback_hears_windows_tts() {
        let (tx, mut rx) = mpsc::channel(256);
        let cap = start(16_000, tx, |_| {}).expect("capture starts");
        println!("device: {}", cap.device);
        let ps = "Add-Type -AssemblyName System.Speech; (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak('Testing the loopback capture for eztrans.')";
        std::process::Command::new("powershell").args(["-NoProfile", "-Command", ps]).status().expect("tts");
        std::thread::sleep(Duration::from_millis(500));
        cap.stop();
        let mut chunks = 0;
        let mut peak = 0.0f32;
        while let Ok(c) = rx.try_recv() {
            chunks += 1;
            let rms = (c.iter().map(|&s| (s as f32 / 32768.0).powi(2)).sum::<f32>() / c.len() as f32).sqrt();
            peak = peak.max(rms);
        }
        println!("chunks: {chunks}, peak rms: {peak:.4}");
        assert!(chunks >= 15, "about 2 s of speech should give 15+ chunks");
        assert!(peak > 0.005, "speech should be audible in the loopback");
    }
}
