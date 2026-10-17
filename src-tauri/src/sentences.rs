// Windows Live Captions shows one rolling block of text: the last few lines of speech, revised as
// recognition firms up. This turns that block into caption lines: a gray line for what is being
// said, and a finished (translated) line for each sentence once it has settled.
//
// What the text does (recorded on Windows 11 build 26200, testdata/live-captions-en-trace.json):
// the words being recognized end with a provisional period that moves on as the next word arrives
// ("Pipeline." -> "Pipeline onto the." -> "Pipeline onto the new cluster"); a period in the middle
// can also come and go for a few seconds ("servers. Once every" -> "servers once every"); case and
// commas change; old text is cut off at the front, mid-word. The words themselves rarely change.
// So the place reached is found by words, ignoring case and punctuation, and a sentence end only
// counts once it has stayed put.
//
// Ported from LiveCaptions-Translator by SakiRinn (Apache-2.0, see THIRD_PARTY_NOTICES.md): the
// clean-up of RegexPatterns.cs and TextUtil.ReplaceNewlines, the sentence-end punctuation, the
// short-sentence threshold and the "unchanged for 50 polls" idea of Translator.SyncLoop.
// Reimplemented in Rust, with changes: a point between digits ("3.5") is not a sentence end, an
// acronym is single letters ("U.S", not "OK. So"), a very short sentence joins the next one, and
// sentences are cut by words that have settled rather than by the last punctuation seen.

use std::time::{Duration, Instant};

const EOS: [char; 6] = ['.', '?', '!', '。', '？', '！'];
/// A sentence shorter than this many UTF-8 bytes ("OK.", "Yes.") joins the next one.
const SHORT_BYTES: usize = 10;
/// A line piece at least this long ends a sentence where Live Captions breaks the line.
const MEDIUM_BYTES: usize = 40;
/// A sentence end followed by more words counts once it has stayed this long.
pub const SETTLE: Duration = Duration::from_millis(1500);
/// Nothing new for this long and the last sentence has its end: it is finished.
pub const STABLE: Duration = Duration::from_millis(1500);
/// Nothing new for this long without a sentence end: finished anyway. Live Captions itself can go
/// quiet for over two seconds in the middle of a sentence while it catches up.
pub const QUIET: Duration = Duration::from_secs(3);
/// Finished words remembered to find the place again in the next poll.
const ANCHOR_WORDS: usize = 8;

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// What is being said, so far (the gray line).
    Interim(String),
    /// A finished sentence: the gray line becomes final and is translated.
    Final(String),
    /// The gray line came to nothing.
    Drop,
}

/// A word of the caption text: where it is, its lowercase form, and whether a sentence end
/// follows it. A Chinese or Japanese character counts as a word.
#[derive(Debug)]
struct Word {
    start: usize,
    /// After the word and the sentence-end punctuation right after it.
    end: usize,
    norm: String,
    eos: bool,
}

/// Follows the caption text from one poll to the next.
#[derive(Default)]
pub struct Tracker {
    /// The last finished words (lowercase).
    anchor: Vec<String>,
    /// The unfinished words at the last poll, and since when everything up to each one has stayed
    /// the same (words and sentence ends).
    pending: Vec<(String, bool)>,
    since: Vec<Instant>,
    /// The gray line's text.
    open: String,
}

impl Tracker {
    /// Count everything already in the caption text as heard (Live Captions was running before).
    pub fn skip(&mut self, raw: &str) {
        let text = clean(raw);
        let ws = words(&text);
        self.remember(ws.iter().map(|w| w.norm.clone()));
    }

    /// The gray line's text, if any (to finish it when listening stops).
    pub fn open(&self) -> &str {
        &self.open
    }

    /// Read one poll of the caption text.
    pub fn update(&mut self, raw: &str, now: Instant) -> Vec<Event> {
        let text = clean(raw);
        let ws = words(&text);
        let mut out = Vec::new();
        if ws.is_empty() {
            return out;
        }
        let start = self.resume_at(&ws);
        let cur: Vec<(String, bool)> = ws[start..].iter().map(|w| (w.norm.clone(), w.eos)).collect();
        let same = self.pending.iter().zip(&cur).take_while(|(a, b)| a == b).count();
        self.since.truncate(same);
        self.since.resize(cur.len(), now);
        self.pending = cur;

        // How many of the unfinished words are finished now.
        let n = self.pending.len();
        let quiet_for = self.since.last().map_or(Duration::ZERO, |t| now.duration_since(*t));
        let ends = self.pending.last().is_some_and(|w| w.1);
        let mut done = if n > 0 && ((ends && quiet_for >= STABLE) || quiet_for >= QUIET) {
            n
        } else {
            // The last settled sentence end with more words after it.
            (0..n.saturating_sub(1)).rev().find(|&j| self.pending[j].1 && now.duration_since(self.since[j]) >= SETTLE).map_or(0, |j| j + 1)
        };

        // Cut the finished words into sentences; a short one joins the next.
        let mut sentences: Vec<(usize, usize)> = Vec::new(); // word index ranges, relative to start
        let mut from = 0;
        for j in 0..done {
            if !self.pending[j].1 && j + 1 < done {
                continue;
            }
            let piece = &text[ws[start + from].start..ws[start + j].end];
            if piece.trim().len() < SHORT_BYTES && j + 1 < done {
                continue;
            }
            sentences.push((from, j));
            from = j + 1;
        }
        if let Some(&(a, b)) = sentences.last() {
            if text[ws[start + a].start..ws[start + b].end].trim().len() < SHORT_BYTES && done < n {
                // A short last sentence waits for the next one ("OK." before "So we...").
                sentences.pop();
                done = a;
            } else if sentences.len() > 1 && text[ws[start + a].start..ws[start + b].end].trim().len() < SHORT_BYTES {
                // Finished with a short one ("...today. OK."): it joins the one before.
                sentences.pop();
                if let Some(last) = sentences.last_mut() {
                    last.1 = b;
                }
            }
        }
        for &(a, b) in &sentences {
            out.push(Event::Final(text[ws[start + a].start..ws[start + b].end].trim().to_string()));
        }
        if done > 0 {
            self.remember(self.pending[..done].iter().map(|w| w.0.clone()).collect::<Vec<_>>());
            self.pending.drain(..done);
            self.since.drain(..done);
        }

        let finished_now = !out.is_empty();
        let open = if self.pending.is_empty() { "" } else { text[ws[start + done].start..].trim() };
        if open.is_empty() {
            if !self.open.is_empty() && !finished_now {
                out.push(Event::Drop);
            }
            self.open.clear();
        } else if open != self.open || finished_now {
            self.open = open.to_string();
            out.push(Event::Interim(open.to_string()));
        }
        out
    }

    fn remember(&mut self, words: impl IntoIterator<Item = String>) {
        self.anchor.extend(words);
        let extra = self.anchor.len().saturating_sub(ANCHOR_WORDS);
        self.anchor.drain(..extra);
    }

    /// Index of the first unfinished word in this poll.
    fn resume_at(&mut self, ws: &[Word]) -> usize {
        if self.anchor.is_empty() {
            return 0;
        }
        // The end of the finished words, looked for from the end; fewer of them if the text was revised.
        for k in [self.anchor.len(), 5, 3] {
            if k > self.anchor.len() || k == 0 {
                continue;
            }
            let tail = &self.anchor[self.anchor.len() - k..];
            if let Some(i) = (0..=ws.len().saturating_sub(k)).rev().find(|&i| ws.len() >= k && ws[i..i + k].iter().zip(tail).all(|(w, a)| &w.norm == a)) {
                return i + k;
            }
        }
        // Lost the place (Live Captions started over, or rewrote the words): go on from the start of
        // its last sentence, and remember the words before it as finished.
        let last_end = (0..ws.len().saturating_sub(1)).rev().find(|&j| ws[j].eos).map_or(0, |j| j + 1);
        self.anchor.clear();
        self.remember(ws[..last_end].iter().map(|w| w.norm.clone()).collect::<Vec<_>>());
        self.pending.clear();
        self.since.clear();
        last_end
    }
}

/// The words of a cleaned caption text.
fn words(text: &str) -> Vec<Word> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let at = |i: usize| chars.get(i).map(|c| c.1);
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (start, c) = chars[i];
        if !c.is_alphanumeric() {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if !is_cj(c) {
            // Letters and digits, an apostrophe between letters ("don't"), a point or comma between
            // digits ("3.5", "1,200").
            while let Some(d) = at(j) {
                let joins = matches!(d, '\'' | '’') && at(j + 1).is_some_and(char::is_alphabetic) && at(j - 1).is_some_and(char::is_alphabetic)
                    || matches!(d, '.' | ',') && at(j + 1).is_some_and(|x| x.is_ascii_digit()) && at(j - 1).is_some_and(|x| x.is_ascii_digit());
                if (d.is_alphanumeric() && !is_cj(d)) || joins {
                    j += 1;
                } else {
                    break;
                }
            }
        }
        let word_end = chars.get(j).map_or(text.len(), |c| c.0);
        let norm = text[start..word_end].to_lowercase();
        let mut end = word_end;
        let mut eos = false;
        while let Some(d) = at(j) {
            if !EOS.contains(&d) {
                break;
            }
            eos = true;
            j += 1;
            end = chars.get(j).map_or(text.len(), |c| c.0);
        }
        out.push(Word { start, end, norm, eos });
        i = j;
    }
    out
}

/// SyncLoop's clean-up: join acronyms, one space after Latin punctuation and none around CJK
/// punctuation, and turn Live Captions' line breaks into sentence ends or dashes.
pub fn clean(raw: &str) -> String {
    let t = acronyms(raw, false);
    let t = acronyms(&t, true);
    let t = punctuation_spaces(&t);
    replace_newlines(&t, MEDIUM_BYTES).trim().to_string()
}

/// RegexPatterns.Acronym `([A-Z])\s*\.\s*([A-Z])(?![A-Za-z]+)` -> "$1$2", and with
/// `before_word`, AcronymWithWords `...(?=[A-Za-z]+)` -> "$1 $2"; only after a single letter, so a
/// sentence ending in "OK." or "AWS." keeps its period.
fn acronyms(s: &str, before_word: bool) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let skip_space = |mut i: usize| {
        while i < c.len() && c[i].is_whitespace() {
            i += 1;
        }
        i
    };
    let mut i = 0;
    while i < c.len() {
        if c[i].is_ascii_uppercase() && (i == 0 || !c[i - 1].is_alphabetic()) {
            let dot = skip_space(i + 1);
            if c.get(dot) == Some(&'.') {
                let k = skip_space(dot + 1);
                if c.get(k).is_some_and(char::is_ascii_uppercase) && c.get(k + 1).is_some_and(char::is_ascii_alphabetic) == before_word {
                    out.push(c[i]);
                    if before_word {
                        out.push(' ');
                    }
                    out.push(c[k]);
                    i = k + 1;
                    continue;
                }
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// RegexPatterns.PunctuationSpace `\s*([.!?,])\s*` -> "$1 " and CJPunctuationSpace
/// `\s*([。！？，、])\s*` -> "$1"; a point or comma between digits ("3.5", "1,000") is left alone.
fn punctuation_spaces(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 8);
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        let latin = matches!(ch, '.' | '!' | '?' | ',');
        let cjk = matches!(ch, '。' | '！' | '？' | '，' | '、');
        let in_number = matches!(ch, '.' | ',') && i > 0 && c[i - 1].is_ascii_digit() && c.get(i + 1).is_some_and(char::is_ascii_digit);
        if (latin || cjk) && !in_number {
            while out.ends_with(char::is_whitespace) {
                out.pop();
            }
            out.push(ch);
            if latin {
                out.push(' ');
            }
            i += 1;
            while i < c.len() && c[i].is_whitespace() {
                i += 1;
            }
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// TextUtil.ReplaceNewlines: a long piece before a line break ends a sentence, a short one goes on
/// with a dash.
fn replace_newlines(text: &str, threshold: usize) -> String {
    let parts: Vec<&str> = text.split('\n').map(str::trim).collect();
    let mut out = String::with_capacity(text.len() + 8);
    for (i, p) in parts.iter().enumerate() {
        out.push_str(p);
        if i + 1 == parts.len() {
            break;
        }
        let Some(last) = p.chars().last() else { continue };
        out.push_str(match (p.len() >= threshold, is_cj(last)) {
            (true, true) => "。",
            (true, false) => ". ",
            (false, true) => "——",
            (false, false) => "—",
        });
    }
    out
}

/// TextUtil.isCJChar
fn is_cj(ch: char) -> bool {
    matches!(ch, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{3000}'..='\u{303F}' | '\u{3040}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{3200}'..='\u{32FF}' | '\u{3300}'..='\u{33FF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finals(ev: &[Event]) -> Vec<String> {
        ev.iter().filter_map(|e| if let Event::Final(t) = e { Some(t.clone()) } else { None }).collect()
    }

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    fn norm_words(s: &str) -> Vec<String> {
        words(&clean(s)).into_iter().map(|w| w.norm).collect()
    }

    #[test]
    fn cleans_like_sync_loop() {
        assert_eq!(clean("the U.S economy"), "the US economy");
        assert_eq!(clean("Dr. J.Smith said"), "Dr. J Smith said");
        assert_eq!(clean("We moved to AWS. Then OK. So"), "We moved to AWS. Then OK. So");
        assert_eq!(clean("Hello , world .How are you ?"), "Hello, world. How are you?");
        assert_eq!(clean("你好 。 我們開始 ， 好嗎"), "你好。我們開始，好嗎");
        assert_eq!(clean("Revenue grew 3.5 percent to 1,200 units."), "Revenue grew 3.5 percent to 1,200 units.");
        assert_eq!(clean("this first line is long enough to end a sentence\nnext"), "this first line is long enough to end a sentence. next");
        assert_eq!(clean("Hi\nthere"), "Hi—there");
        assert_eq!(clean("会議を始めます\n次"), "会議を始めます——次");
    }

    #[test]
    fn words_ignore_case_and_commas_and_keep_numbers_whole() {
        let ws = words(&clean("Revenue grew 3.5 percent, don't you think? Yes. 好。"));
        let got: Vec<(&str, bool)> = ws.iter().map(|w| (w.norm.as_str(), w.eos)).collect();
        assert_eq!(got, [("revenue", false), ("grew", false), ("3.5", false), ("percent", false), ("don't", false), ("you", false), ("think", true), ("yes", true), ("好", true)]);
    }

    /// The recorded Live Captions text (testdata), replayed at its own timing, then silence.
    #[test]
    fn a_real_recording_becomes_whole_sentences_without_repeats() {
        let trace: Vec<(u64, String)> = serde_json::from_str(include_str!("../testdata/live-captions-en-trace.json")).unwrap();
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let mut ev = Vec::new();
        // The app polls every 25 ms: replay each text until the next one arrived, and 4 s after the last.
        for (i, (ms, text)) in trace.iter().enumerate() {
            let until = trace.get(i + 1).map_or(ms + 4000, |n| n.0);
            let mut now = *ms;
            while now < until {
                ev.extend(t.update(text, at(t0, now)));
                now += 25;
            }
        }
        let fs = finals(&ev);
        println!("{fs:#?}");
        // No fragments: every finished line is a real sentence (the provisional periods did not count).
        assert!(fs.iter().all(|f| f.len() >= SHORT_BYTES), "{fs:?}");
        assert!(fs.len() <= 5, "{fs:?}");
        // No word sequence finished twice.
        let all: Vec<String> = fs.iter().flat_map(|f| norm_words(f)).collect();
        for w in all.windows(4) {
            assert_eq!(all.windows(4).filter(|x| x == &w).count(), 1, "repeated {w:?} in {fs:?}");
        }
        // Everything said came out once, in order.
        let said = norm_words("Pipeline onto the new cluster and then retire the old build servers once every single team has signed off on the migration checklist that we shared last week. Thanks everyone for joining the call today. Could you share your screen and walk us through the deployment");
        assert_eq!(all, said, "{fs:?}");
    }

    /// Polls 25 ms apart with the same text, from `from` for `ms` milliseconds.
    fn hold(t: &mut Tracker, text: &str, t0: Instant, from: u64, ms: u64) -> Vec<Event> {
        (0..=ms / 25).flat_map(|k| t.update(text, at(t0, from + 25 * k))).collect()
    }

    #[test]
    fn a_provisional_period_at_the_end_does_not_finish_a_sentence() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let mut ev = t.update("Pipeline.", t0);
        ev.extend(t.update("Pipeline onto the.", at(t0, 300)));
        ev.extend(t.update("Pipeline onto the new cluster", at(t0, 900)));
        assert!(finals(&ev).is_empty());
        assert_eq!(t.open(), "Pipeline onto the new cluster");
    }

    #[test]
    fn a_settled_sentence_end_with_words_after_it_finishes_the_sentence() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let text = "Thanks everyone for joining the call today. Could you";
        let ev = hold(&mut t, text, t0, 0, 1400);
        assert!(finals(&ev).is_empty(), "not settled yet");
        let ev = t.update("Thanks everyone for joining the call today. Could you share", at(t0, 1500));
        assert_eq!(finals(&ev), ["Thanks everyone for joining the call today."]);
        assert_eq!(ev.last(), Some(&Event::Interim("Could you share".into())));
    }

    #[test]
    fn a_middle_period_that_comes_and_goes_is_not_a_repeat() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let mut ev = hold(&mut t, "retire the old build servers. Once every single team", t0, 0, 1600);
        assert_eq!(finals(&ev), ["retire the old build servers."]);
        // Live Captions takes the period back and goes on: the finished words are found again.
        ev = t.update("retire the old build servers once every single team has signed off", at(t0, 1700));
        assert!(finals(&ev).is_empty());
        assert_eq!(t.open(), "once every single team has signed off");
    }

    #[test]
    fn silence_finishes_the_last_sentence() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let ev = hold(&mut t, "Could you share your screen?", t0, 0, 1500);
        assert_eq!(finals(&ev), ["Could you share your screen?"]);
        // Without a sentence end it waits longer: Live Captions may still be catching up.
        let mut t = Tracker::default();
        assert!(finals(&hold(&mut t, "and that we shared last", t0, 0, 2900)).is_empty());
        assert_eq!(finals(&t.update("and that we shared last", at(t0, 3000))), ["and that we shared last"]);
    }

    #[test]
    fn short_sentences_join_the_next() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let ev = hold(&mut t, "OK. So let us begin with the plan. Next", t0, 0, 1600);
        assert_eq!(finals(&ev), ["OK. So let us begin with the plan."]);
        let mut t = Tracker::default();
        assert_eq!(finals(&hold(&mut t, "Yes.", t0, 0, 1600)), ["Yes."]);
    }

    #[test]
    fn text_cut_off_at_the_front_mid_word_is_still_followed() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let ev = hold(&mut t, "Pipeline onto the new cluster and then retire the old build servers. Once every", t0, 0, 1600);
        assert_eq!(finals(&ev), ["Pipeline onto the new cluster and then retire the old build servers."]);
        let ev = t.update("eline onto the new cluster and then retire the old build servers. Once every single team", at(t0, 1700));
        assert!(finals(&ev).is_empty());
        assert_eq!(t.open(), "Once every single team");
    }

    #[test]
    fn text_from_before_listening_is_skipped() {
        let mut t = Tracker::default();
        t.skip("This was said before we started listening.");
        let ev = t.update("This was said before we started listening. New words", Instant::now());
        assert_eq!(ev, [Event::Interim("New words".into())]);
    }

    #[test]
    fn chinese_is_followed_character_by_character() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let ev = hold(&mut t, "謝謝大家今天來參加這個會議。可以請你", t0, 0, 1600);
        assert_eq!(finals(&ev), ["謝謝大家今天來參加這個會議。"]);
        assert_eq!(t.open(), "可以請你");
    }
}
