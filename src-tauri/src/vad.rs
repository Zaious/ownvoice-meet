// Energy-based speech segmentation, ported from the PyQt prototype (audio.py), where it was tuned
// on meeting audio played through the speakers: 30 ms frames, speech above an RMS threshold, 300 ms
// kept from before the onset, an utterance ends after 700 ms of silence or 12 s of speech.
//
// Events let two kinds of provider share it: upload-per-sentence providers take `End.audio`;
// streaming providers send `Start.preroll` and `Audio` as they come and commit on `End`, so silence
// between sentences is never sent (and never billed).

use std::collections::VecDeque;

pub const RMS_THRESHOLD: f32 = 0.008;
const FRAME_MS: usize = 30;
const PREROLL_MS: usize = 300;
const END_SILENCE_MS: usize = 700;
const MAX_UTTERANCE_MS: usize = 12_000;
const MIN_VOICED_MS: usize = 300;

#[derive(Debug, PartialEq)]
pub enum VadEvent {
    /// Speech began; the audio just before it, so the first word is not clipped.
    Start(Vec<i16>),
    /// More audio of the current utterance.
    Audio(Vec<i16>),
    /// The utterance ended; all of its audio (preroll included).
    End(Vec<i16>),
    /// What looked like speech was too short to be speech (a click, a cough).
    Discard,
}

pub struct Segmenter {
    frame: usize,
    pending: Vec<i16>,
    preroll: VecDeque<Vec<i16>>,
    preroll_frames: usize,
    utterance: Vec<i16>,
    in_speech: bool,
    silent_frames: usize,
    voiced_frames: usize,
    total_frames: usize,
}

impl Segmenter {
    pub fn new(rate: u32) -> Self {
        Self {
            frame: rate as usize * FRAME_MS / 1000,
            pending: Vec::new(),
            preroll: VecDeque::new(),
            preroll_frames: PREROLL_MS / FRAME_MS,
            utterance: Vec::new(),
            in_speech: false,
            silent_frames: 0,
            voiced_frames: 0,
            total_frames: 0,
        }
    }

    pub fn in_speech(&self) -> bool {
        self.in_speech
    }

    pub fn push(&mut self, chunk: &[i16]) -> Vec<VadEvent> {
        self.pending.extend_from_slice(chunk);
        let mut events = Vec::new();
        let mut audio = Vec::new();
        while self.pending.len() >= self.frame {
            let frame: Vec<i16> = self.pending.drain(..self.frame).collect();
            let rms = (frame.iter().map(|&s| (s as f32 / 32768.0).powi(2)).sum::<f32>() / frame.len() as f32).sqrt();
            let voiced = rms > RMS_THRESHOLD;
            if !self.in_speech {
                if voiced {
                    self.in_speech = true;
                    let pre: Vec<i16> = self.preroll.drain(..).flatten().collect();
                    self.utterance = pre.clone();
                    self.utterance.extend_from_slice(&frame);
                    self.silent_frames = 0;
                    self.voiced_frames = 1;
                    self.total_frames = 1;
                    let mut start = pre;
                    start.extend_from_slice(&frame);
                    events.push(VadEvent::Start(start));
                } else {
                    self.preroll.push_back(frame);
                    if self.preroll.len() > self.preroll_frames {
                        self.preroll.pop_front();
                    }
                }
                continue;
            }
            self.utterance.extend_from_slice(&frame);
            audio.extend_from_slice(&frame);
            self.total_frames += 1;
            if voiced {
                self.voiced_frames += 1;
                self.silent_frames = 0;
            } else {
                self.silent_frames += 1;
            }
            let ended = self.silent_frames * FRAME_MS >= END_SILENCE_MS || self.total_frames * FRAME_MS >= MAX_UTTERANCE_MS;
            if ended {
                if !audio.is_empty() {
                    events.push(VadEvent::Audio(std::mem::take(&mut audio)));
                }
                self.in_speech = false;
                let utt = std::mem::take(&mut self.utterance);
                events.push(if self.voiced_frames * FRAME_MS >= MIN_VOICED_MS { VadEvent::End(utt) } else { VadEvent::Discard });
            }
        }
        if !audio.is_empty() {
            events.push(VadEvent::Audio(audio));
        }
        events
    }
}

/// 16-bit mono PCM as a WAV file, for upload-per-sentence providers.
pub fn wav(samples: &[i16], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM header size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;

    fn tone(ms: usize) -> Vec<i16> {
        (0..RATE as usize * ms / 1000).map(|i| ((i as f32 * 0.3).sin() * 8000.0) as i16).collect()
    }
    fn silence(ms: usize) -> Vec<i16> {
        vec![0; RATE as usize * ms / 1000]
    }

    fn run(seg: &mut Segmenter, audio: &[i16]) -> Vec<VadEvent> {
        audio.chunks(1600).flat_map(|c| seg.push(c)).collect()
    }

    #[test]
    fn one_sentence_gives_start_audio_end_with_preroll() {
        let mut seg = Segmenter::new(RATE);
        let mut audio = silence(600);
        audio.extend(tone(1500));
        audio.extend(silence(900));
        let ev = run(&mut seg, &audio);
        assert!(matches!(ev.first(), Some(VadEvent::Start(_))));
        let end: Vec<_> = ev.iter().filter_map(|e| if let VadEvent::End(a) = e { Some(a.len()) } else { None }).collect();
        assert_eq!(end.len(), 1, "exactly one utterance: {ev:?}");
        // 300 ms preroll + 1500 ms speech + 700 ms closing silence, to the frame
        let expect = RATE as usize * 2500 / 1000;
        assert!((end[0] as i64 - expect as i64).abs() <= 480, "utterance length {} vs {expect}", end[0]);
        assert!(!seg.in_speech());
    }

    #[test]
    fn a_click_is_discarded() {
        let mut seg = Segmenter::new(RATE);
        let mut audio = tone(120);
        audio.extend(silence(900));
        let ev = run(&mut seg, &audio);
        assert!(ev.contains(&VadEvent::Discard));
        assert!(!ev.iter().any(|e| matches!(e, VadEvent::End(_))));
    }

    #[test]
    fn long_speech_is_cut_at_twelve_seconds() {
        let mut seg = Segmenter::new(RATE);
        let ev = run(&mut seg, &tone(13_000));
        assert_eq!(ev.iter().filter(|e| matches!(e, VadEvent::End(_))).count(), 1);
    }

    #[test]
    fn wav_header_is_well_formed() {
        let w = wav(&[0, 1, -1], RATE);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(w.len(), 44 + 6);
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), RATE);
    }
}
