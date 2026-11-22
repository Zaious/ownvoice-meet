// Audio capture. Two sources: what the computer plays (the default output device recorded in WASAPI
// loopback mode; cpal turns an input stream on an output device into loopback), for online meetings,
// or a microphone, for a talk in the room. Either way the sound is downmixed and resampled to mono
// 16-bit PCM at the rate a provider asks for (Gemini Live 16 kHz, OpenAI realtime 24 kHz), in 100 ms
// chunks. The same chunks can also go to the recorder (recorder.rs), which never drops any.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use tokio::sync::mpsc;

/// Where every chunk goes for the recording (recorder.rs).
pub type Tap = Box<dyn Fn(Vec<i16>) + Send>;

/// Samples in a 100 ms chunk at `rate`.
pub fn chunk_len(rate: u32) -> usize {
    rate as usize / 10
}

/// Where the sound comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// What the computer plays (an online meeting).
    Loopback,
    /// A microphone, by the name Windows lists (empty = the default one), with a software gain.
    Mic { device: String, gain: f32 },
}

impl Source {
    pub fn from_settings(s: &crate::settings::Settings) -> Self {
        if s.audio_source == "mic" {
            Source::Mic { device: s.mic_device.clone(), gain: s.mic_gain.clamp(1.0, 16.0) }
        } else {
            Source::Loopback
        }
    }

    fn gain(&self) -> f32 {
        match self {
            Source::Loopback => 1.0,
            Source::Mic { gain, .. } => *gain,
        }
    }
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

/// Start capturing. Chunks go to `tx` (dropped when the receiver lags, so audio never piles up for
/// speech-to-text) and, every one of them, to `record` when given; `on_level` gets each chunk's RMS
/// for the level meter. The computer and screen stay awake while it runs.
pub fn start(
    rate: u32,
    source: Source,
    tx: mpsc::Sender<Vec<i16>>,
    record: Option<Tap>,
    on_level: impl Fn(f32) + Send + 'static,
) -> Result<Capture, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = std_mpsc::channel::<Result<String, String>>();
    let stop_t = stop.clone();
    // cpal streams are not Send on every platform: the stream lives and dies on its own thread.
    let thread = std::thread::spawn(move || {
        let stream = match open(rate, &source, tx, record, on_level) {
            Ok((stream, name)) => {
                let _ = ready_tx.send(Ok(name));
                stream
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        let _awake = KeepAwake::new();
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

/// The microphones the system offers, by the name shown in Settings.
pub fn input_devices() -> Vec<String> {
    labeled_inputs(&cpal::default_host()).into_iter().map(|(name, _)| name).collect()
}

/// Every microphone with a name that tells it apart: Windows' full name ("麥克風 (USB Audio
/// Device)"), and "#2", "#3" on names that are still the same. Settings stores this name, and
/// opening a microphone looks it up the same way.
fn labeled_inputs(host: &cpal::Host) -> Vec<(String, cpal::Device)> {
    let mut out: Vec<(String, cpal::Device)> = Vec::new();
    for d in host.input_devices().into_iter().flatten() {
        let base = device_name(&d);
        if base.is_empty() {
            continue;
        }
        let same = out.iter().filter(|(n, _)| *n == base || n.starts_with(&format!("{base} #"))).count();
        let name = if same == 0 { base } else { format!("{base} #{}", same + 1) };
        out.push((name, d));
    }
    out
}

/// Listen to `source` for `secs` and report the loudest and the average chunk level (RMS, after the
/// gain), for the microphone check in Settings. `on_level` sees every chunk as it comes.
pub fn measure(source: Source, secs: f32, on_level: impl Fn(f32) + Send + 'static) -> Result<(String, f32, f32), String> {
    let levels = Arc::new(Mutex::new(Vec::<f32>::new()));
    let sink = levels.clone();
    let (tx, _rx) = mpsc::channel(4);
    let cap = start(16_000, source, tx, None, move |rms| {
        on_level(rms);
        if let Ok(mut l) = sink.lock() {
            l.push(rms);
        }
    })?;
    let device = cap.device.clone();
    std::thread::sleep(Duration::from_secs_f32(secs));
    cap.stop();
    let l = levels.lock().map_err(|_| "level lock is poisoned")?;
    if l.is_empty() {
        return Err(format!("「{device}」沒有送出任何聲音"));
    }
    let peak = l.iter().copied().fold(0.0, f32::max);
    let avg = l.iter().sum::<f32>() / l.len() as f32;
    Ok((device, peak, avg))
}

/// The device's full name: on Windows the friendly name ("麥克風 (USB Audio Device)") is the first
/// extended line, while the plain name is only the kind of device ("麥克風").
fn device_name(d: &cpal::Device) -> String {
    let Ok(desc) = d.description() else { return String::new() };
    if let Some(full) = desc.extended().first().filter(|l| !l.trim().is_empty()) {
        return full.trim().to_string();
    }
    match desc.driver() {
        Some(driver) if !driver.is_empty() && driver != desc.name() => format!("{} ({driver})", desc.name()),
        _ => desc.name().to_string(),
    }
}

fn open(
    rate: u32,
    source: &Source,
    tx: mpsc::Sender<Vec<i16>>,
    record: Option<Tap>,
    on_level: impl Fn(f32) + Send + 'static,
) -> Result<(cpal::Stream, String), String> {
    let host = cpal::default_host();
    let (device, supported) = match source {
        Source::Loopback => {
            if cfg!(target_os = "macos") {
                return Err("Mac 不能直接錄電腦播放的聲音：請到設定 → 收音與存檔，改用麥克風".into());
            }
            let device = host.default_output_device().ok_or("no default output device")?;
            let supported = device.default_output_config().map_err(|e| format!("output config: {e}"))?;
            (device, supported)
        }
        Source::Mic { device: wanted, .. } => {
            let device = if wanted.is_empty() {
                host.default_input_device().ok_or("找不到麥克風：Windows 沒有預設的錄音裝置")?
            } else {
                labeled_inputs(&host)
                    .into_iter()
                    .find(|(name, _)| name == wanted)
                    .map(|(_, d)| d)
                    .ok_or_else(|| format!("找不到麥克風「{wanted}」：請到設定重新選擇"))?
            };
            let supported = device.default_input_config().map_err(|e| format!("麥克風設定：{e}"))?;
            (device, supported)
        }
    };
    let name = match device_name(&device) {
        n if n.is_empty() => "audio".to_string(),
        n => n,
    };
    let format = supported.sample_format();
    let config = supported.config();
    let mut proc = Downmix::new(config.channels as usize, config.sample_rate, rate, source.gain(), tx, record, on_level);
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
        SampleFormat::I32 => device.build_input_stream(
            &config,
            move |data: &[i32], _: &cpal::InputCallbackInfo| proc.feed(data.iter().map(|&s| s as f32 / 2_147_483_648.0)),
            on_err,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| proc.feed(data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0)),
            on_err,
            None,
        ),
        other => return Err(format!("unsupported sample format {other:?}")),
    };
    let blocked = matches!(source, Source::Mic { .. });
    let hint = |e: String| {
        if blocked {
            format!("麥克風無法開啟：{e}（若是權限問題：Windows 設定 → 隱私權與安全性 → 麥克風 → 允許桌面應用程式存取麥克風）")
        } else {
            e
        }
    };
    let stream = stream.map_err(|e| hint(format!("capture stream: {e}")))?;
    stream.play().map_err(|e| hint(format!("start stream: {e}")))?;
    Ok((stream, name))
}

/// Keeps the computer and screen awake while capturing: a laptop at a talk would otherwise go to
/// sleep mid-session. Windows: a request that belongs to the capture thread and ends with it. macOS:
/// the system's `caffeinate`, run for as long as the capture lasts.
struct KeepAwake {
    #[cfg(target_os = "macos")]
    caffeinate: Option<std::process::Child>,
}

impl KeepAwake {
    fn new() -> Self {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED};
            SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED);
        }
        KeepAwake {
            // -d: the display, -i: idle sleep.
            #[cfg(target_os = "macos")]
            caffeinate: std::process::Command::new("/usr/bin/caffeinate").arg("-di").spawn().ok(),
        }
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS};
            SetThreadExecutionState(ES_CONTINUOUS);
        }
        #[cfg(target_os = "macos")]
        if let Some(mut c) = self.caffeinate.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
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
    gain: f32,
    chunk_len: usize,
    chunk: Vec<i16>,
    tx: mpsc::Sender<Vec<i16>>,
    record: Option<Tap>,
    on_level: L,
}

impl<L: Fn(f32)> Downmix<L> {
    fn new(
        channels: usize,
        in_rate: u32,
        out_rate: u32,
        gain: f32,
        tx: mpsc::Sender<Vec<i16>>,
        record: Option<Tap>,
        on_level: L,
    ) -> Self {
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
            gain,
            chunk_len: chunk_len(out_rate),
            chunk: Vec::with_capacity(chunk_len(out_rate)),
            tx,
            record,
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
        self.chunk.push(((y * self.gain).clamp(-1.0, 1.0) * 32767.0) as i16);
        if self.chunk.len() >= self.chunk_len {
            let chunk = std::mem::replace(&mut self.chunk, Vec::with_capacity(self.chunk_len));
            let rms = (chunk.iter().map(|&s| (s as f32 / 32768.0).powi(2)).sum::<f32>() / chunk.len() as f32).sqrt();
            (self.on_level)(rms);
            if let Some(r) = &self.record {
                r(chunk.clone());
            }
            let _ = self.tx.try_send(chunk);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, amp: f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * amp;
                [v, v]
            })
            .collect()
    }

    #[test]
    fn downsamples_48k_stereo_to_16k_chunks() {
        let (tx, mut rx) = mpsc::channel(16);
        let mut d = Downmix::new(2, 48_000, 16_000, 1.0, tx, None, |_| {});
        // 0.2 s of a 440 Hz tone, stereo
        d.feed(tone(9_600, 0.5).into_iter());
        let a = rx.try_recv().expect("first 100 ms chunk");
        let b = rx.try_recv().expect("second 100 ms chunk");
        assert_eq!(a.len(), 1_600);
        assert_eq!(b.len(), 1_600);
        let peak = b.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(peak > 12_000 && peak < 17_000, "tone survives downsampling, peak {peak}");
    }

    #[test]
    fn gain_lifts_a_quiet_microphone_and_the_recorder_gets_every_chunk() {
        let (tx, _rx) = mpsc::channel(1); // speech-to-text lags: chunks beyond the first are dropped there
        let (rec_tx, rec_rx) = std_mpsc::channel();
        let tap: Tap = Box::new(move |c| {
            let _ = rec_tx.send(c);
        });
        let mut d = Downmix::new(2, 48_000, 16_000, 4.0, tx, Some(tap), |_| {});
        d.feed(tone(48_000, 0.05).into_iter()); // 1 s of a quiet tone
        let chunks: Vec<Vec<i16>> = rec_rx.try_iter().collect();
        assert_eq!(chunks.len(), 10, "one second = ten 100 ms chunks, none dropped");
        let peak = chunks[5].iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(peak > 5_500 && peak < 7_000, "0.05 x 4 = 0.2 of full scale, peak {peak}");
    }

    #[test]
    fn gain_clips_instead_of_wrapping() {
        let (tx, mut rx) = mpsc::channel(4);
        let mut d = Downmix::new(2, 48_000, 16_000, 16.0, tx, None, |_| {});
        d.feed(tone(4_800, 0.5).into_iter());
        let c = rx.try_recv().unwrap();
        assert!(c.iter().all(|&s| s >= -32_767), "no wrap-around past full scale");
        assert!(c.iter().any(|&s| s == 32_767), "loud input is clipped at full scale");
    }

    #[test]
    fn settings_choose_the_source() {
        let mut s = crate::settings::Settings::default();
        let default = if cfg!(target_os = "macos") { Source::Mic { device: String::new(), gain: 1.0 } } else { Source::Loopback };
        assert_eq!(Source::from_settings(&s), default, "the Mac starts on the microphone");
        s.audio_source = "loopback".into();
        assert_eq!(Source::from_settings(&s), Source::Loopback);
        s.audio_source = "mic".into();
        s.mic_device = "USB Mic".into();
        s.mic_gain = 40.0;
        assert_eq!(Source::from_settings(&s), Source::Mic { device: "USB Mic".into(), gain: 16.0 });
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
        let cap = start(16_000, Source::Loopback, tx, None, |_| {}).expect("capture starts");
        println!("device: {}", cap.device);
        let ps = "Add-Type -AssemblyName System.Speech; (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak('Testing the loopback capture for OwnVoice Meet.')";
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

    // Needs a microphone: cargo test --lib -- --ignored --nocapture microphone
    #[test]
    #[ignore]
    fn microphone_lists_and_measures() {
        let names = input_devices();
        println!("microphones: {names:?}");
        assert!(!names.is_empty(), "at least one microphone");
        let (device, peak, avg) = measure(Source::Mic { device: String::new(), gain: 1.0 }, 2.0, |_| {}).expect("default microphone opens");
        println!("default microphone: {device}, peak {peak:.4}, average {avg:.4}");
        for name in &names {
            match measure(Source::Mic { device: name.clone(), gain: 1.0 }, 2.0, |_| {}) {
                Ok((d, peak, avg)) => println!("  {d}: peak {peak:.4}, average {avg:.4}"),
                Err(e) => println!("  {name}: {e}"),
            }
        }
    }
}
