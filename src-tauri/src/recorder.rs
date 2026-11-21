// The recording: every audio chunk written to a 16-bit mono WAV file on its own thread, so speech
// recognition (and a venue's Wi-Fi) can fail without losing the sound. One file per segment: a new
// segment closes the file and opens the next with no gap in the samples. Audio and splits travel in
// one channel, so a split lands exactly between the chunks before and after it. The header's sizes
// are rewritten every few seconds, so a file cut short by a crash or a flat battery still plays.
// The computer's own audio (loopback) delivers nothing while nothing plays; those stretches are
// written as silence, so the recording keeps real time and matches the transcript's timestamps.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::audio::Tap;

const HEADER_EVERY: Duration = Duration::from_secs(5);
/// Fill with silence only when the file is behind real time by more than this.
const GAP_SLACK: Duration = Duration::from_millis(300);

enum Msg {
    Audio(Vec<i16>),
    /// Close the current file (renaming it to `closing_as` if that differs) and start `next`.
    Split { closing_as: PathBuf, next: PathBuf },
    /// Close the current file, renaming it to `closing_as` if that differs.
    Stop { closing_as: PathBuf },
}

pub struct Recorder {
    tx: mpsc::Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

impl Recorder {
    /// Start writing to `path` at `rate`. Returns the recorder and the tap that audio.rs feeds.
    pub fn start(path: PathBuf, rate: u32) -> Result<(Recorder, Tap), String> {
        let first = Wav::create(&path, rate).map_err(|e| format!("無法建立錄音檔 {}：{e}", path.display()))?;
        let (tx, rx) = mpsc::channel::<Msg>();
        let thread = std::thread::spawn(move || run(first, rate, rx));
        let audio = tx.clone();
        let tap: Tap = Box::new(move |chunk| {
            let _ = audio.send(Msg::Audio(chunk));
        });
        Ok((Recorder { tx, thread: Some(thread) }, tap))
    }

    /// Close the current file as `closing_as` and continue in `next`.
    pub fn split(&self, closing_as: PathBuf, next: PathBuf) {
        let _ = self.tx.send(Msg::Split { closing_as, next });
    }

    /// Finish the last file as `closing_as` and wait for the writer. Stop the capture first, so no
    /// chunk arrives after this.
    pub fn stop(mut self, closing_as: PathBuf) {
        let _ = self.tx.send(Msg::Stop { closing_as });
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(mut wav: Wav, rate: u32, rx: mpsc::Receiver<Msg>) {
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Audio(chunk) => {
                let _ = wav.fill_gap(chunk.len());
                if let Err(e) = wav.write(&chunk) {
                    eprintln!("recorder: write failed: {e}");
                }
            }
            Msg::Split { closing_as, next } => {
                let _ = wav.fill_gap(0);
                let _ = wav.finish(&closing_as);
                match Wav::create(&next, rate) {
                    Ok(w) => wav = w,
                    Err(e) => {
                        eprintln!("recorder: cannot open {}: {e}", next.display());
                        return;
                    }
                }
            }
            Msg::Stop { closing_as } => {
                let _ = wav.fill_gap(0);
                let _ = wav.finish(&closing_as);
                return;
            }
        }
    }
    // Every sender dropped without a Stop (the app is going away): close the file as it is.
    let path = wav.path.clone();
    let _ = wav.finish(&path);
}

/// A WAV file being written: the 44-byte header holds the sizes, rewritten as the data grows.
struct Wav {
    out: BufWriter<File>,
    path: PathBuf,
    rate: u32,
    opened: Instant,
    data_bytes: u32,
    last_header: Instant,
}

impl Wav {
    fn create(path: &Path, rate: u32) -> std::io::Result<Wav> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(&header(rate, 0))?;
        Ok(Wav { out, path: path.to_path_buf(), rate, opened: Instant::now(), data_bytes: 0, last_header: Instant::now() })
    }

    /// Write silence for the stretch where no audio came, leaving room for `incoming` samples that
    /// are about to be written: the file then covers the time since it was opened.
    fn fill_gap(&mut self, incoming: usize) -> std::io::Result<()> {
        let due = (self.opened.elapsed().as_secs_f64() * self.rate as f64) as u64;
        let have = (self.data_bytes / 2) as u64 + incoming as u64;
        let slack = (GAP_SLACK.as_secs_f64() * self.rate as f64) as u64;
        if due > have + slack {
            let missing = (due - have) as usize;
            for _ in 0..missing / 1600 {
                self.write(&[0i16; 1600])?;
            }
            self.write(&vec![0i16; missing % 1600])?;
        }
        Ok(())
    }

    fn write(&mut self, samples: &[i16]) -> std::io::Result<()> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        self.out.write_all(&bytes)?;
        self.data_bytes = self.data_bytes.saturating_add(bytes.len() as u32);
        if self.last_header.elapsed() >= HEADER_EVERY {
            self.sync_header()?;
        }
        Ok(())
    }

    fn sync_header(&mut self) -> std::io::Result<()> {
        self.last_header = Instant::now();
        let riff = 36u32.saturating_add(self.data_bytes);
        self.out.seek(SeekFrom::Start(4))?;
        self.out.write_all(&riff.to_le_bytes())?;
        self.out.seek(SeekFrom::Start(40))?;
        self.out.write_all(&self.data_bytes.to_le_bytes())?;
        self.out.seek(SeekFrom::End(0))?;
        self.out.flush()
    }

    /// Final sizes, flushed and closed; then renamed if the segment's name changed meanwhile.
    fn finish(mut self, closing_as: &Path) -> std::io::Result<()> {
        self.sync_header()?;
        let path = self.path.clone();
        drop(self.out);
        if closing_as != path {
            std::fs::rename(&path, closing_as)?;
        }
        Ok(())
    }
}

/// The canonical 44-byte header for 16-bit mono PCM.
fn header(rate: u32, data_bytes: u32) -> [u8; 44] {
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_bytes).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&(rate * 2).to_le_bytes()); // byte rate
    h[32..34].copy_from_slice(&2u16.to_le_bytes()); // block align
    h[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits per sample
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_u32(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ownvoice-rec-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn writes_a_playable_wav_and_splits_exactly_between_chunks() {
        let dir = temp_dir("split");
        let (rec, tap) = Recorder::start(dir.join("01-part.wav"), 16_000).unwrap();
        for _ in 0..10 {
            tap(vec![1000i16; 1600]); // 1 s
        }
        rec.split(dir.join("01-第一段.wav"), dir.join("02-part.wav"));
        for _ in 0..5 {
            tap(vec![-1000i16; 1600]); // 0.5 s
        }
        drop(tap);
        rec.stop(dir.join("02-part.wav"));

        let first = std::fs::read(dir.join("01-第一段.wav")).expect("first file renamed to its segment name");
        assert!(!dir.join("01-part.wav").exists());
        assert_eq!(&first[0..4], b"RIFF");
        assert_eq!(&first[8..16], b"WAVEfmt ");
        assert_eq!(read_u32(&first, 24), 16_000);
        assert_eq!(read_u32(&first, 40), 32_000, "1 s of 16-bit mono at 16 kHz, nothing from after the split");
        assert_eq!(read_u32(&first, 4), 36 + 32_000);
        assert_eq!(first.len(), 44 + 32_000);
        assert!(first[44..].chunks(2).all(|b| i16::from_le_bytes([b[0], b[1]]) == 1000));

        let second = std::fs::read(dir.join("02-part.wav")).unwrap();
        assert_eq!(read_u32(&second, 40), 16_000, "0.5 s in the second file");
        assert!(second[44..].chunks(2).all(|b| i16::from_le_bytes([b[0], b[1]]) == -1000));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stretches_without_audio_become_silence() {
        let dir = temp_dir("gap");
        let (rec, tap) = Recorder::start(dir.join("gap.wav"), 16_000).unwrap();
        tap(vec![500i16; 1600]); // 0.1 s right away
        std::thread::sleep(Duration::from_millis(1_000)); // nothing plays for a second
        tap(vec![500i16; 1600]);
        drop(tap);
        rec.stop(dir.join("gap.wav"));
        let b = std::fs::read(dir.join("gap.wav")).unwrap();
        let secs = read_u32(&b, 40) as f64 / 2.0 / 16_000.0;
        assert!((1.0..1.4).contains(&secs), "about 1.1 s of real time, got {secs:.2} s");
        let last = i16::from_le_bytes([b[b.len() - 2], b[b.len() - 1]]);
        assert_eq!(last, 500, "the audio after the gap comes after the silence");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn header_sizes_are_kept_up_to_date_while_recording() {
        let dir = temp_dir("live");
        let path = dir.join("live.wav");
        let mut wav = Wav::create(&path, 16_000).unwrap();
        wav.write(&vec![0i16; 16_000]).unwrap();
        wav.sync_header().unwrap(); // what happens every few seconds
        let mid = std::fs::read(&path).unwrap();
        assert_eq!(read_u32(&mid, 40), 32_000, "a file read mid-recording already has its size");
        wav.finish(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
