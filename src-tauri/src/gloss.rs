// "補對照": fill in the missing half of a bilingual script. A line to read in English with no meaning
// under it gets its Chinese meaning; a line written in Chinese becomes English to read aloud, with
// the Chinese kept as its meaning. Only the lines that need it go to the model, as a numbered JSON
// list; the answer is merged back here, so headings, cues, copy blocks and lines that already have a
// meaning are never touched. Format: see src/script/format.ts.

use std::sync::Mutex;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::llm::{self, Http};
use crate::prompts;
use crate::session::Session;
use crate::settings;
use crate::usage;

const BATCH: usize = 40;

const SYSTEM: &str = "You prepare a bilingual script for a Taiwanese participant in an English-language meeting or talk.
You get a JSON list of items. Each has \"to\": \"zh\" or \"en\", and \"text\".
- to = zh: give the meaning in natural Traditional Chinese as used in Taiwan, as a reading aid. Keep names and technical terms in English where Taiwanese speakers would.
- to = en: write the English the participant will read aloud: short spoken sentences, plain words, their meaning and tone, nothing added.
Answer with only a JSON array of strings, one per item, in the same order. No other text.";

#[derive(Debug, PartialEq)]
pub enum Dir {
    ToZh,
    ToEn,
}

/// A line that needs its other half: its index in the script, the prefix to keep ("" or "Q: "),
/// the text, and which way to translate.
#[derive(Debug, PartialEq)]
pub struct Need {
    pub line: usize,
    pub prefix: &'static str,
    pub text: String,
    pub dir: Dir,
}

fn is_chinese(s: &str) -> bool {
    let cjk = s.chars().filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c) || ('\u{3400}'..='\u{4dbf}').contains(c)).count();
    let latin = s.chars().filter(|c| c.is_ascii_alphabetic()).count();
    cjk > 0 && cjk * 2 >= latin
}

/// Lines to fill, in the same terms as the script parser (format.ts).
pub fn plan(script: &str) -> Vec<Need> {
    let lines: Vec<&str> = script.lines().collect();
    let mut needs = Vec::new();
    let mut in_block = false;
    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim_end();
        if in_block {
            if line.trim() == "```" {
                in_block = false;
            }
            continue;
        }
        if line.starts_with("```copy:") {
            in_block = true;
            continue;
        }
        if line.trim().is_empty() || line.starts_with("# ") || line.starts_with("## ") || line.starts_with("! ") || line.starts_with("> ") {
            continue;
        }
        let (prefix, text) = match line.strip_prefix("Q: ") {
            Some(q) => ("Q: ", q),
            None => ("", line),
        };
        // Already has a meaning: the next non-empty line is a "> " line.
        let next = lines[i + 1..].iter().map(|l| l.trim_end()).find(|l| !l.trim().is_empty());
        if next.is_some_and(|n| n.starts_with("> ")) {
            continue;
        }
        let dir = if is_chinese(text) { Dir::ToEn } else { Dir::ToZh };
        needs.push(Need { line: i, prefix, text: text.trim().to_string(), dir });
    }
    needs
}

/// Put the answers back: English lines get "> Chinese" under them; Chinese lines become the English
/// to read, with "> the Chinese" under it.
pub fn merge(script: &str, needs: &[Need], answers: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut k = 0;
    for (i, raw) in script.lines().enumerate() {
        match needs.get(k).filter(|n| n.line == i) {
            Some(n) => {
                let a = answers[k].trim();
                match n.dir {
                    Dir::ToZh => {
                        out.push(raw.to_string());
                        out.push(format!("> {a}"));
                    }
                    Dir::ToEn => {
                        out.push(format!("{}{a}", n.prefix));
                        out.push(format!("> {}", n.text));
                    }
                }
                k += 1;
            }
            None => out.push(raw.to_string()),
        }
    }
    let mut text = out.join("\n");
    if script.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// The model's answer as a list of strings, tolerating a code fence around it.
fn parse_answers(text: &str, expect: usize) -> Result<Vec<String>, String> {
    let t = text.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    let v: Value = serde_json::from_str(t).map_err(|_| "模型沒有回傳可讀的清單，請再試一次".to_string())?;
    let list: Vec<String> = v.as_array().ok_or("模型沒有回傳清單")?.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect();
    if list.len() != expect || list.iter().any(|s| s.trim().is_empty()) {
        return Err(format!("模型回了 {} 項、需要 {expect} 項，請再試一次", list.len()));
    }
    Ok(list)
}

/// Fill the script's missing halves with the translation model chosen in Settings.
/// Returns the new script and how many lines were filled.
pub async fn fill(app: &AppHandle, script: &str) -> Result<(String, usize), String> {
    let s = settings::load(app);
    let target = llm::target(&s)?;
    let client = app.state::<Http>().0.clone();
    fill_with(&client, &target, &prompts::parse_glossary(&s.glossary), script, |u| usage::add_tokens(app, &target.catalog_id, u)).await
}

/// The work of `fill`, given a client and model; `on_usage` gets each request's token counts.
pub async fn fill_with(
    client: &reqwest::Client,
    target: &llm::Target,
    glossary: &[(String, String)],
    script: &str,
    mut on_usage: impl FnMut(usage::Tally),
) -> Result<(String, usize), String> {
    let needs = plan(script);
    if needs.is_empty() {
        return Ok((script.to_string(), 0));
    }
    let mut answers = Vec::with_capacity(needs.len());
    for chunk in needs.chunks(BATCH) {
        let items: Vec<Value> = chunk
            .iter()
            .map(|n| json!({ "to": if n.dir == Dir::ToZh { "zh" } else { "en" }, "text": n.text }))
            .collect();
        let mut text = String::new();
        let u = llm::stream(client, target, prompts::system_blocks(SYSTEM, glossary, ""), Value::Array(items).to_string(), |d| text.push_str(d)).await?;
        on_usage(u);
        answers.extend(parse_answers(&text, chunk.len())?);
    }
    Ok((merge(script, &needs, &answers), needs.len()))
}

/// Fill, then make the result the open script (saved, and context for translations).
pub async fn fill_open_script(app: &AppHandle, script: String) -> Result<(String, usize), String> {
    let (filled, n) = fill(app, &script).await?;
    if n > 0 {
        if let Ok(mut sess) = app.state::<Mutex<Session>>().lock() {
            sess.script = filled.clone();
        }
        settings::save_script(app, &filled)?;
    }
    Ok((filled, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = "# Pitch\n! cue stays\nHello everyone.\n> 大家好。\nThis has no meaning yet.\n我們下週上線。\n```copy:link\nhttps://example.com\nnot a line\n```\nQ: 怎麼貢獻？\n";

    #[test]
    fn plan_finds_only_lines_missing_their_other_half() {
        let n = plan(SCRIPT);
        assert_eq!(n.len(), 3, "{n:?}");
        assert_eq!((n[0].text.as_str(), &n[0].dir), ("This has no meaning yet.", &Dir::ToZh));
        assert_eq!((n[1].text.as_str(), &n[1].dir), ("我們下週上線。", &Dir::ToEn));
        assert_eq!((n[2].prefix, n[2].text.as_str(), &n[2].dir), ("Q: ", "怎麼貢獻？", &Dir::ToEn));
    }

    #[test]
    fn merge_keeps_everything_else_and_fills_both_directions() {
        let n = plan(SCRIPT);
        let out = merge(SCRIPT, &n, &["這還沒有意思。".into(), "We launch next week.".into(), "How do I contribute?".into()]);
        assert_eq!(
            out,
            "# Pitch\n! cue stays\nHello everyone.\n> 大家好。\nThis has no meaning yet.\n> 這還沒有意思。\nWe launch next week.\n> 我們下週上線。\n```copy:link\nhttps://example.com\nnot a line\n```\nQ: How do I contribute?\n> 怎麼貢獻？\n"
        );
        assert!(plan(&out).is_empty(), "nothing left to fill");
    }

    #[test]
    fn answers_must_match_the_items() {
        assert_eq!(parse_answers("```json\n[\"a\",\"b\"]\n```", 2).unwrap(), vec!["a", "b"]);
        assert!(parse_answers("[\"a\"]", 2).is_err());
        assert!(parse_answers("not json", 1).is_err());
    }

    // Real API call with the translation model's key from .env.local:
    //   cargo test --lib -- --ignored --nocapture gloss_live
    #[tokio::test]
    #[ignore]
    async fn gloss_live_fills_both_directions() {
        let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../.env.local"));
        let Some(key) = std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.is_empty()) else {
            println!("ANTHROPIC_API_KEY not set; skipped");
            return;
        };
        let _ = rustls::crypto::ring::default_provider().install_default();
        let target = llm::Target {
            catalog_id: "claude-haiku-5-5".into(),
            transport: "anthropic",
            url: "https://api.anthropic.com/v1/messages".into(),
            key: Some(key),
            model: "claude-haiku-5-5".into(),
            effort: "low".into(),
        };
        let script = "# 開場\nHi everyone, thanks for having me.\n我們的工具不會替你作答。\nQ: 費用大概多少？\n";
        let t0 = std::time::Instant::now();
        let (out, n) = fill_with(&reqwest::Client::new(), &target, &[], script, |_| {}).await.expect("fill");
        println!("{:.2}s, filled {n}:\n{out}", t0.elapsed().as_secs_f32());
        assert_eq!(n, 3);
        assert!(plan(&out).is_empty(), "every line has its other half");
        assert!(out.contains("> 我們的工具不會替你作答。"), "Chinese kept as the meaning");
        assert!(out.contains("\nQ: ") && out.contains("> 費用大概多少？"), "question turned into English, Chinese kept");
    }

    #[test]
    fn mixed_lines_go_by_their_main_language() {
        assert!(is_chinese("我們用 Claude Code 安裝"));
        assert!(!is_chinese("Install it in Claude Code, 很快"));
    }
}
