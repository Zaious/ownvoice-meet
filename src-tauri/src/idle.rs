// Idle listening costs money: Gemini Live bills every second it is sent, silence included, and a
// window left listening runs all night. After a quiet stretch (no new caption line; with record-only,
// nothing loud) the speech service is let go, so nothing is sent or billed; as soon as someone speaks
// it is reconnected, with the last two seconds sent first so the first words are not lost. The
// recording does not pause. After a longer quiet stretch listening ends by itself, as if ■ 停止 was
// pressed. Both times are in Settings (minutes, 0 = never).

use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;

use crate::captions;

/// Audio kept while paused, sent first when the speech service is reconnected.
const PREROLL_CHUNKS: usize = 20; // 2 s of 100 ms chunks
/// Chunks in a row above the wake level before reconnecting.
const WAKE_CHUNKS: u32 = 3;
/// The quietest wake level; above the room's own noise in any case.
const WAKE_MIN: f32 = 0.008;
/// How loud a chunk must be to count as someone speaking, with no captions to go by (record-only).
const SOUND: f32 = 0.01;

/// When something last happened: a caption line opened or grew, or (record-only) a loud chunk.
pub struct Activity {
    base: Instant,
    last_ms: AtomicU64,
}

impl Default for Activity {
    fn default() -> Self {
        Activity { base: Instant::now(), last_ms: AtomicU64::new(0) }
    }
}

pub fn touch(app: &AppHandle) {
    let a = app.state::<Activity>();
    a.last_ms.store(a.base.elapsed().as_millis() as u64, Ordering::Relaxed);
}

fn quiet_for(app: &AppHandle) -> Duration {
    let a = app.state::<Activity>();
    Duration::from_millis((a.base.elapsed().as_millis() as u64).saturating_sub(a.last_ms.load(Ordering::Relaxed)))
}

/// Minutes from Settings: 0 (or less) = never.
pub fn minutes(m: f32) -> Option<Duration> {
    (m > 0.0).then(|| Duration::from_secs_f32(m * 60.0))
}

#[derive(Debug, PartialEq)]
pub enum Step {
    /// Send this chunk to the speech service.
    Forward(Vec<i16>),
    /// Paused: keep it for the preroll only.
    Hold,
    /// Let the speech service go now.
    Pause,
    /// Reconnect, sending these chunks first (the preroll, ending with the one that woke it).
    Resume(Vec<Vec<i16>>),
    /// Quiet for too long: stop listening.
    End,
}

/// The decisions, apart from the plumbing so they can be tested.
pub struct Gate {
    pause_after: Option<Duration>,
    end_after: Option<Duration>,
    /// The quiet time at which to pause: `pause_after`, or later right after a reconnect (a full
    /// pause time of grace, since the first caption takes a moment). Back to `pause_after` as soon as
    /// something happens.
    pause_at: Option<Duration>,
    last_quiet: Duration,
    paused: bool,
    preroll: VecDeque<Vec<i16>>,
    loud: u32,
    floor: Option<f32>,
}

impl Gate {
    pub fn new(pause_after: Option<Duration>, end_after: Option<Duration>) -> Self {
        Gate { pause_after, end_after, pause_at: pause_after, last_quiet: Duration::ZERO, paused: false, preroll: VecDeque::new(), loud: 0, floor: None }
    }

    #[cfg(test)]
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// A chunk of audio arrived; `quiet` is how long nothing has happened.
    pub fn on_chunk(&mut self, chunk: Vec<i16>, rms: f32, quiet: Duration) -> Step {
        match self.on_tick(quiet) {
            Step::End => return Step::End,
            Step::Pause => {
                self.keep(chunk);
                return Step::Pause;
            }
            _ => {}
        }
        if !self.paused {
            self.learn(rms);
            return Step::Forward(chunk);
        }
        self.keep(chunk);
        // Wake on speech clearly above the room's own level, sustained for a moment.
        let wake = self.floor.map_or(WAKE_MIN, |f| (f * 2.5).max(WAKE_MIN));
        if rms > wake {
            self.loud += 1;
        } else {
            self.loud = 0;
            self.learn(rms);
        }
        if self.loud >= WAKE_CHUNKS {
            self.paused = false;
            self.loud = 0;
            self.pause_at = self.pause_after.map(|p| quiet + p);
            return Step::Resume(self.preroll.drain(..).collect());
        }
        Step::Hold
    }

    /// Time passed with or without audio (the computer's own audio sends nothing while nothing plays).
    pub fn on_tick(&mut self, quiet: Duration) -> Step {
        if quiet < self.last_quiet {
            self.pause_at = self.pause_after; // something happened: the grace is over
        }
        self.last_quiet = quiet;
        // The end time counts from the last caption, never from a reconnect: noise that keeps waking
        // it in a loud hall cannot keep the session alive.
        if self.end_after.is_some_and(|e| quiet >= e) {
            return Step::End;
        }
        if !self.paused && self.pause_at.is_some_and(|p| quiet >= p) {
            self.paused = true;
            self.preroll.clear();
            self.loud = 0;
            return Step::Pause;
        }
        Step::Hold
    }

    /// The room's own level: pulled down quickly by the gaps between words, raised slowly, so it
    /// settles on the noise rather than the speech.
    fn learn(&mut self, rms: f32) {
        self.floor = Some(match self.floor {
            None => rms,
            Some(f) if rms < f => f * 0.5 + rms * 0.5,
            Some(f) => f + (rms - f) * 0.002,
        });
    }

    fn keep(&mut self, chunk: Vec<i16>) {
        if self.preroll.len() == PREROLL_CHUNKS {
            self.preroll.pop_front();
        }
        self.preroll.push_back(chunk);
    }
}

/// "3", or "0.5" for a time under a whole minute.
pub fn minutes_label(m: f32) -> String {
    if (m - m.round()).abs() < 0.01 { format!("{:.0}", m.round()) } else { format!("{m:.1}") }
}

fn rms(chunk: &[i16]) -> f32 {
    if chunk.is_empty() {
        return 0.0;
    }
    (chunk.iter().map(|&s| (s as f32 / 32768.0).powi(2)).sum::<f32>() / chunk.len() as f32).sqrt()
}

/// How a listening session ended.
pub enum Ended {
    /// The capture stopped (■ 停止, or the app closing).
    Stopped,
    /// Quiet for longer than the end time.
    Idle,
    /// The speech service gave up on its own (a rejected key, say) and nothing is being recorded.
    Failed,
}

/// Sit between the capture and the speech service: forward audio, let the service go when it is
/// quiet, bring it back when someone speaks, and end when it has been quiet too long. `spawn` starts
/// the speech service on a channel (None for record-only); `recording` says whether sound is being
/// saved, in which case a service that fails on its own does not end the session.
pub async fn supervise<F, Fut>(
    app: AppHandle,
    mut capture: mpsc::Receiver<Vec<i16>>,
    pause_after: Option<Duration>,
    end_after: Option<Duration>,
    spawn: Option<F>,
    recording: impl Fn() -> bool,
) -> Ended
where
    F: Fn(mpsc::Receiver<Vec<i16>>) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    touch(&app);
    let mut record_only = spawn.is_none();
    let mut gate = Gate::new(if record_only { None } else { pause_after }, end_after);
    let start = |chunks: Vec<Vec<i16>>| {
        let (tx, rx) = mpsc::channel::<Vec<i16>>(64);
        for c in chunks {
            let _ = tx.try_send(c);
        }
        let task = spawn.as_ref().map(|f| tauri::async_runtime::spawn(f(rx)));
        (tx, task)
    };
    let (mut tx, mut task) = if record_only { (None, None) } else {
        let (tx, task) = start(Vec::new());
        (Some(tx), task)
    };
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        let step = tokio::select! {
            chunk = capture.recv() => match chunk {
                None => {
                    drop(tx.take());
                    if let Some(t) = task.take() {
                        let _ = t.await;
                    }
                    return Ended::Stopped;
                }
                Some(c) => {
                    let level = rms(&c);
                    if record_only && level > SOUND {
                        touch(&app);
                    }
                    gate.on_chunk(c, level, quiet_for(&app))
                }
            },
            _ = tick.tick() => gate.on_tick(quiet_for(&app)),
            // The speech service ended on its own while it was supposed to run.
            _ = async { task.as_mut().expect("guarded").await }, if task.is_some() => {
                task = None;
                tx = None;
                if !recording() {
                    return Ended::Failed;
                }
                // Keep watching the recording so a forgotten window still ends.
                record_only = true;
                gate = Gate::new(None, end_after);
                continue;
            }
        };
        match step {
            Step::Forward(c) => {
                if let Some(t) = &tx {
                    let _ = t.try_send(c);
                }
            }
            Step::Hold => {}
            Step::Pause => {
                drop(tx.take());
                if let Some(t) = task.take() {
                    let _ = t.await;
                }
                let mins = pause_after.map_or_else(String::new, |d| minutes_label(d.as_secs_f32() / 60.0));
                let rec = if recording() { "；錄音繼續" } else { "" };
                captions::status(&app, format!("已暫停辨識：安靜超過 {mins} 分鐘，不再送出聲音、不計費；有人說話會自動接上{rec}"));
            }
            Step::Resume(chunks) => {
                captions::status(&app, "有聲音，重新連線中…");
                let (t, h) = start(chunks);
                tx = Some(t);
                task = h;
            }
            Step::End => {
                drop(tx.take());
                if let Some(t) = task.take() {
                    let _ = t.await;
                }
                return Ended::Idle;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: Duration = Duration::from_secs(60);

    fn chunk(v: i16) -> Vec<i16> {
        vec![v; 1600]
    }

    #[test]
    fn forwards_while_people_talk_then_pauses_after_the_quiet_time() {
        let mut g = Gate::new(Some(3 * M), Some(20 * M));
        assert_eq!(g.on_chunk(chunk(5), 0.0002, M), Step::Forward(chunk(5)));
        assert_eq!(g.on_chunk(chunk(5), 0.0002, 3 * M), Step::Pause);
        assert!(g.paused());
        assert_eq!(g.on_chunk(chunk(5), 0.0002, 3 * M + Duration::from_secs(1)), Step::Hold, "nothing is sent while paused");
    }

    #[test]
    fn speech_wakes_it_with_the_last_two_seconds_first() {
        let mut g = Gate::new(Some(3 * M), Some(20 * M));
        assert_eq!(g.on_tick(4 * M), Step::Pause, "the computer's own audio may send nothing at all: time alone pauses");
        for _ in 0..30 {
            assert_eq!(g.on_chunk(chunk(10), 0.001, 4 * M), Step::Hold, "room noise does not wake it");
        }
        assert_eq!(g.on_chunk(chunk(3000), 0.09, 4 * M), Step::Hold);
        assert_eq!(g.on_chunk(chunk(3000), 0.09, 4 * M), Step::Hold);
        match g.on_chunk(chunk(3001), 0.09, 4 * M) {
            Step::Resume(pre) => {
                assert_eq!(pre.len(), PREROLL_CHUNKS, "2 s of audio go first");
                assert_eq!(pre.last(), Some(&chunk(3001)), "ending with the chunk that woke it");
                assert_eq!(pre[pre.len() - 3], chunk(3000), "the start of the speech is in it");
            }
            other => panic!("expected Resume, got {other:?}"),
        }
        assert!(!g.paused());
        assert_eq!(g.on_chunk(chunk(7), 0.09, 4 * M), Step::Forward(chunk(7)), "no instant re-pause before the first caption");
        assert_eq!(g.on_tick(6 * M), Step::Hold, "a full pause time of grace after reconnecting");
        assert_eq!(g.on_tick(7 * M), Step::Pause, "and paused again if still nothing was said");
    }

    #[test]
    fn a_caption_after_a_reconnect_brings_the_normal_pause_time_back() {
        let mut g = Gate::new(Some(3 * M), Some(20 * M));
        g.on_chunk(chunk(5), 0.002, M); // a quiet room
        g.on_tick(3 * M);
        for _ in 0..3 {
            g.on_chunk(chunk(3000), 0.2, 3 * M);
        }
        assert!(!g.paused());
        assert_eq!(g.on_tick(Duration::from_secs(5)), Step::Hold, "a caption arrived: quiet went back to zero");
        assert_eq!(g.on_tick(3 * M), Step::Pause, "three quiet minutes after it, not six");
    }

    #[test]
    fn a_noisy_room_needs_speech_clearly_above_its_own_level() {
        let mut g = Gate::new(Some(3 * M), None);
        for _ in 0..50 {
            g.on_chunk(chunk(1), 0.03, M); // a loud hall, heard while listening normally
        }
        g.on_tick(3 * M);
        for _ in 0..50 {
            assert_eq!(g.on_chunk(chunk(1), 0.03, 3 * M), Step::Hold, "the hall itself does not wake it");
        }
        for _ in 0..5 {
            assert_eq!(g.on_chunk(chunk(1), 0.05, 3 * M), Step::Hold, "a little louder than the hall is not speech");
        }
        let woke = (0..3).map(|_| g.on_chunk(chunk(1), 0.2, 3 * M)).any(|s| matches!(s, Step::Resume(_)));
        assert!(woke);
    }

    #[test]
    fn speech_in_the_first_paused_moment_still_wakes_it() {
        let mut g = Gate::new(Some(3 * M), None);
        g.on_tick(3 * M); // nothing was heard before: no idea of the room's level yet
        let woke = (0..3).map(|_| g.on_chunk(chunk(3000), 0.2, 3 * M)).any(|s| matches!(s, Step::Resume(_)));
        assert!(woke, "speech is not mistaken for the room's own level");
    }

    #[test]
    fn ends_after_the_end_time_paused_or_not() {
        let mut g = Gate::new(Some(3 * M), Some(20 * M));
        assert_eq!(g.on_tick(20 * M), Step::End);
        let mut g = Gate::new(Some(3 * M), Some(20 * M));
        g.on_tick(3 * M);
        assert_eq!(g.on_chunk(chunk(1), 0.0, 20 * M), Step::End);
    }

    #[test]
    fn minutes_read_naturally() {
        assert_eq!(minutes_label(3.0), "3");
        assert_eq!(minutes_label(0.1), "0.1");
        assert_eq!(minutes_label(20.0), "20");
    }

    #[test]
    fn zero_minutes_means_never() {
        assert_eq!(minutes(0.0), None);
        assert_eq!(minutes(3.0), Some(3 * M));
        let mut g = Gate::new(minutes(0.0), minutes(0.0));
        assert_eq!(g.on_tick(10_000 * M), Step::Hold);
        assert_eq!(g.on_chunk(chunk(1), 0.0, 10_000 * M), Step::Forward(chunk(1)));
    }
}
