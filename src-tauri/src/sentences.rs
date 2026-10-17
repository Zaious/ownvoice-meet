// Windows Live Captions shows one rolling block of text: the last few lines of speech, revised as
// recognition firms up, with punctuation added as sentences end. This turns that block into caption
// lines: a gray line for the sentence being spoken, and a finished (translated) line for each
// sentence that ends.
//
// Ported from LiveCaptions-Translator by SakiRinn (Apache-2.0, see THIRD_PARTY_NOTICES.md): the
// clean-up of RegexPatterns.cs and TextUtil.ReplaceNewlines, the last-sentence rules and thresholds
// of Translator.SyncLoop, and TextUtil.Similarity. Reimplemented in Rust, with four changes: a point
// between digits ("3.5") is not a sentence end, an acronym is single letters ("U.S", not "OK. So"),
// a very short sentence joins the next one instead of the one before, and the finished text is found
// again by position so each sentence becomes exactly one caption line.

use std::time::{Duration, Instant};

const EOS: [char; 6] = ['.', '?', '!', '。', '？', '！'];
/// A sentence shorter than this many UTF-8 bytes ("OK.", "Yes.") joins the next one.
const SHORT_BYTES: usize = 10;
/// A line piece at least this long ends a sentence where Live Captions breaks the line.
const MEDIUM_BYTES: usize = 40;
/// The sentence being spoken counts as finished once it stays unchanged this long
/// (SyncLoop: 50 polls of 25 ms).
pub const STABLE: Duration = Duration::from_millis(1250);
/// How much finished text is remembered to find the place again in the next poll.
const ANCHOR_CHARS: usize = 60;
/// SyncLoop's threshold for "the same sentence, revised".
const SIMILAR: f64 = 0.6;

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// The sentence being spoken, so far (the gray line).
    Interim(String),
    /// A sentence that ended: the gray line becomes final and is translated.
    Final(String),
    /// The gray line came to nothing.
    Drop,
}

/// Follows the caption text from one poll to the next.
#[derive(Default)]
pub struct Tracker {
    /// The end of the text already finished, as it reads in the caption text.
    anchor: String,
    /// The sentence being spoken and when it last changed.
    open: String,
    changed: Option<Instant>,
}

impl Tracker {
    /// Count everything already in the caption text as heard (Live Captions was running before).
    pub fn skip(&mut self, raw: &str) {
        let text = clean(raw);
        self.anchor = tail(&text, text.len());
    }

    /// The sentence being spoken, if any (to finish it when listening stops).
    pub fn open(&self) -> &str {
        &self.open
    }

    /// Read one poll of the caption text.
    pub fn update(&mut self, raw: &str, now: Instant) -> Vec<Event> {
        let text = clean(raw);
        let mut out = Vec::new();
        if text.is_empty() {
            return out;
        }
        let start = self.resume_at(&text);
        // Every ended sentence after the finished text becomes a line; a short one waits for the next.
        let mut from = start;
        for (_, end) in sentences(&text[start..]).into_iter().map(|(s, e)| (s + start, e + start)) {
            if !ends_sentence(&text[..end]) {
                break;
            }
            let line = body(&text[from..end]);
            if line.len() < SHORT_BYTES {
                continue;
            }
            out.push(Event::Final(line.to_string()));
            self.anchor = tail(&text, end);
            from = end;
        }
        let finished_now = !out.is_empty();
        let open = body(&text[from..]);
        if open.is_empty() {
            if !self.open.is_empty() && !finished_now {
                out.push(Event::Drop);
            }
            self.open.clear();
            self.changed = None;
        } else if open != self.open || finished_now {
            self.open = open.to_string();
            self.changed = Some(now);
            out.push(Event::Interim(open.to_string()));
        } else if self.changed.is_some_and(|t| now.duration_since(t) >= STABLE) && (open.len() >= SHORT_BYTES || ends_sentence(open)) {
            // Unchanged for a while: the speaker paused without Live Captions ending the sentence.
            out.push(Event::Final(open.to_string()));
            self.anchor = tail(&text, text.len());
            self.open.clear();
            self.changed = None;
        }
        out
    }

    /// Where the unfinished text starts in this poll.
    fn resume_at(&mut self, text: &str) -> usize {
        if self.anchor.is_empty() {
            return 0;
        }
        // Old lines scroll off the front, so only the end of the anchor may still be there.
        let mut tried = "";
        for n in [ANCHOR_CHARS, 30, 15] {
            let a = last_chars(&self.anchor, n);
            if a == tried {
                break;
            }
            if let Some(p) = text.rfind(a) {
                return p + a.len();
            }
            tried = a;
        }
        // Live Captions revised the finished text (or started over). As SyncLoop does, look at the
        // last sentence: if it is the finished one, revised, resume after its last finished word;
        // otherwise it is new speech.
        let ranges = sentences(text);
        let Some(&(mut s, _)) = ranges.last() else { return 0 };
        if ranges.len() > 1 && body(&text[s..]).len() < SHORT_BYTES {
            s = ranges[ranges.len() - 2].0;
        }
        let last = &text[s..];
        let ranges = sentences(&self.anchor);
        let finished = ranges.last().map_or(self.anchor.as_str(), |&(a, _)| &self.anchor[a..]);
        let resume = if similar(body(last), body(finished)) {
            match last_word(finished).and_then(|w| last.rfind(w).map(|p| p + w.len())) {
                Some(p) => s + p,
                None => text.len(),
            }
        } else {
            s
        };
        self.anchor = tail(text, resume);
        resume
    }
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

/// Byte ranges of the sentences in `text`, each ending after its run of end punctuation; the last
/// one may be unfinished.
fn sentences(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = text.char_indices().peekable();
    while let Some((i, ch)) = it.next() {
        if !is_eos_at(text, i, ch) {
            continue;
        }
        let mut end = i + ch.len_utf8();
        while let Some(&(j, next)) = it.peek() {
            if !EOS.contains(&next) {
                break;
            }
            end = j + next.len_utf8();
            it.next();
        }
        out.push((start, end));
        start = end;
    }
    if !text[start..].trim().is_empty() {
        out.push((start, text.len()));
    }
    out
}

fn is_eos_at(text: &str, i: usize, ch: char) -> bool {
    if !EOS.contains(&ch) {
        return false;
    }
    let digit_before = text[..i].chars().next_back().is_some_and(|c| c.is_ascii_digit());
    let digit_after = text[i + ch.len_utf8()..].chars().next().is_some_and(|c| c.is_ascii_digit());
    !(ch == '.' && digit_before && digit_after)
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end().chars().next_back().is_some_and(|c| EOS.contains(&c))
}

/// A sentence without the spaces and stray punctuation around it.
fn body(s: &str) -> &str {
    s.trim_start_matches(|c: char| c.is_whitespace() || EOS.contains(&c) || matches!(c, ',' | '，' | '、' | '—')).trim_end()
}

/// The last `ANCHOR_CHARS` characters of `text[..end]`.
fn tail(text: &str, end: usize) -> String {
    last_chars(&text[..end], ANCHOR_CHARS).to_string()
}

fn last_chars(s: &str, n: usize) -> &str {
    &s[s.char_indices().rev().nth(n - 1).map_or(0, |(i, _)| i)..]
}

fn last_word(s: &str) -> Option<&str> {
    s.split(|c: char| !c.is_alphanumeric()).rev().find(|w| w.chars().count() >= 2)
}

/// TextUtil.Similarity over the common length, as Translator.IsOverwrite compares a sentence with
/// the one before; case is ignored.
fn similar(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let n = a.len().min(b.len());
    let (a, b) = (&a[..n], &b[..n]);
    if n == 0 || a == b {
        return true;
    }
    1.0 - levenshtein(a, b) as f64 / n as f64 > SIMILAR
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=a.len()).collect();
    let mut cur = vec![0; a.len() + 1];
    for (j, cb) in b.iter().enumerate() {
        cur[0] = j + 1;
        for (i, ca) in a.iter().enumerate() {
            cur[i + 1] = (cur[i] + 1).min(prev[i + 1] + 1).min(prev[i] + usize::from(ca != cb));
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[a.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finals(ev: &[Event]) -> Vec<&str> {
        ev.iter().filter_map(|e| if let Event::Final(t) = e { Some(t.as_str()) } else { None }).collect()
    }

    /// Feed polls 25 ms apart; returns every event.
    fn feed(t: &mut Tracker, t0: Instant, polls: &[&str]) -> Vec<Event> {
        polls.iter().enumerate().flat_map(|(i, p)| t.update(p, t0 + Duration::from_millis(25 * i as u64))).collect()
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
    fn splits_sentences_but_not_decimals() {
        let t = "Revenue grew 3.5 percent. Is that right?! Yes";
        let parts: Vec<&str> = sentences(t).iter().map(|&(s, e)| t[s..e].trim()).collect();
        assert_eq!(parts, ["Revenue grew 3.5 percent.", "Is that right?!", "Yes"]);
    }

    #[test]
    fn one_line_per_sentence_as_the_text_rolls() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let ev = feed(&mut t, t0, &[
            "Thanks everyone",
            "Thanks everyone for joining the call",
            "Thanks everyone for joining the call today.",
            "Thanks everyone for joining the call today. Could you",
            "Thanks everyone for joining the call today. Could you share your screen?",
            // Old text scrolls off the front.
            "joining the call today. Could you share your screen? And walk us",
        ]);
        assert_eq!(finals(&ev), ["Thanks everyone for joining the call today.", "Could you share your screen?"]);
        assert_eq!(ev[0], Event::Interim("Thanks everyone".into()));
        assert_eq!(ev.last(), Some(&Event::Interim("And walk us".into())));
        assert_eq!(t.open(), "And walk us");
    }

    #[test]
    fn a_growing_sentence_stays_gray_until_it_ends() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let words = "so the plan for the next quarter is to move the whole deployment to the new cluster";
        let mut text = String::new();
        let mut ev = Vec::new();
        for (i, w) in words.split(' ').enumerate() {
            text.push_str(w);
            text.push(' ');
            ev.extend(t.update(&text, t0 + Duration::from_millis(300 * i as u64)));
        }
        assert!(finals(&ev).is_empty());
        assert_eq!(t.open(), words);
    }

    #[test]
    fn short_sentences_join_the_next() {
        let mut t = Tracker::default();
        let ev = feed(&mut t, Instant::now(), &["OK.", "OK. So let us begin with the plan."]);
        assert_eq!(finals(&ev), ["OK. So let us begin with the plan."]);
    }

    #[test]
    fn a_pause_finishes_the_sentence() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        assert_eq!(t.update("we will meet again tomorrow", t0), [Event::Interim("we will meet again tomorrow".into())]);
        assert!(t.update("we will meet again tomorrow", t0 + Duration::from_millis(600)).is_empty());
        assert_eq!(t.update("we will meet again tomorrow", t0 + STABLE), [Event::Final("we will meet again tomorrow".into())]);
        // Live Captions punctuates it afterwards and the speaker goes on: nothing is repeated.
        let ev = t.update("we will meet again tomorrow. Bye now", t0 + STABLE * 2);
        assert_eq!(ev, [Event::Interim("Bye now".into())]);
        // A short reply stays gray while it may still join the next sentence, then finishes.
        let mut t = Tracker::default();
        t.update("Yes.", t0);
        assert_eq!(t.update("Yes.", t0 + STABLE), [Event::Final("Yes.".into())]);
    }

    #[test]
    fn a_revised_sentence_is_not_repeated() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let ev = feed(&mut t, t0, &[
            "Thanks everyone for joining the call today.",
            "Thanks, everyone, for joining the call today.",
            "Thanks, everyone, for joining the call today. Next item",
        ]);
        assert_eq!(finals(&ev), ["Thanks everyone for joining the call today."]);
        assert_eq!(ev.last(), Some(&Event::Interim("Next item".into())));
        // An unfinished sentence that was finished at a pause, revised and continued.
        let mut t = Tracker::default();
        t.update("we will meet today", t0);
        t.update("we will meet today", t0 + STABLE);
        let ev = t.update("We will, meet today and tomorrow", t0 + STABLE * 2);
        assert_eq!(ev, [Event::Interim("and tomorrow".into())]);
    }

    #[test]
    fn text_from_before_listening_is_skipped() {
        let mut t = Tracker::default();
        t.skip("This was said before we started listening.");
        let ev = t.update("This was said before we started listening. New words", Instant::now());
        assert_eq!(ev, [Event::Interim("New words".into())]);
        // Live Captions started over with new text: it is all new.
        let ev = t.update("Something else entirely", Instant::now());
        assert_eq!(ev, [Event::Interim("Something else entirely".into())]);
    }

    #[test]
    fn a_gray_line_that_disappears_is_dropped() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        t.update("Done here.", t0);
        t.update("Done here. Um", t0);
        assert_eq!(finals(&t.update("Done here. Um", t0)), Vec::<&str>::new());
        assert_eq!(t.update("Done here.", t0 + Duration::from_millis(50)), [Event::Drop]);
    }
}
