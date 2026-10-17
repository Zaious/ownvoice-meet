// Prompts and context, carried over from the PyQt prototype where they were checked in a live
// meeting: the caption prompt never comments on recognition errors (its output is the subtitle),
// replies are short spoken sentences, and the script reaches both directions as a cached block.

use serde_json::{json, Value};

pub const LISTEN_SYSTEM: &str = "You are a live interpreter in an online meeting. Translate the latest utterance into natural Traditional Chinese as used in Taiwan.
The utterance comes from speech recognition: it may contain recognition errors or be a sentence fragment. Translate the most plausible meaning and keep a fragment as a fragment; do not complete it or add anything.
When a word looks misrecognized, silently use the most plausible reading; keep an unknown name as heard. Never comment on the recognition or on your translation.
Your entire output is shown as a subtitle: output only the Chinese translation, with no quotes, labels or notes.";

pub const REPLY_SYSTEM: &str = "You help a Taiwanese participant speak in an English-language meeting. They type what they want to say in Chinese (it may be casual, incomplete, or mixed with English). Write the English they will read aloud word for word.
- Spoken style: short sentences of at most about 15 words, plain words that are easy to pronounce, natural meeting register.
- Keep their meaning, stance and tone. Do not add points, greetings, or filler they did not imply.
- Use the conversation context to pick the right terms and refer to what others said.
- If their prepared script is provided and what they type matches a point in it, reuse the script's English wording and terms.
Output only the English script, with no quotes, labels or notes.";

/// `中文 = English` per line; `#` starts a comment.
pub fn parse_glossary(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(zh, en)| (zh.trim().to_string(), en.trim().to_string()))
        .filter(|(zh, en)| !zh.is_empty() && !en.is_empty())
        .collect()
}

/// System blocks: instructions (+ glossary), then the script as a separate cached block.
pub fn system_blocks(instructions: &str, glossary: &[(String, String)], script: &str) -> Vec<Value> {
    let mut text = instructions.to_string();
    if !glossary.is_empty() {
        text.push_str("\n\nGlossary (always use these renderings):\n");
        for (zh, en) in glossary {
            text.push_str(&format!("- {zh} = {en}\n"));
        }
    }
    let mut blocks = vec![json!({ "type": "text", "text": text })];
    let script = script.trim();
    if !script.is_empty() {
        blocks.push(json!({
            "type": "text",
            "text": format!(
                "The participant's prepared script for today's meeting follows. It is reference material for topic, terminology, names and what they intend to say; others may refer to it. Never translate or recite it unless asked.\n<script>\n{script}\n</script>"
            ),
            // The script is the same on every call: cache the prefix up to here.
            "cache_control": { "type": "ephemeral" }
        }));
    }
    blocks
}

/// `unfinished`: the speaker is still in the middle of this sentence (an early translation).
pub fn listen_user(context: &[String], utterance: &str, unfinished: bool) -> String {
    let ctx = if context.is_empty() { "(none)".to_string() } else { context.join("\n") };
    // The script is named in the request itself: with only the system prompt saying so, a long
    // line now and then came back in Simplified Chinese.
    let ask = if unfinished {
        "The speaker is still talking: this utterance stops mid-sentence. Translate only what is there, as a fragment, into Traditional Chinese as used in Taiwan (繁體中文), and output only that"
    } else {
        "Translate this utterance into Traditional Chinese as used in Taiwan (繁體中文)"
    };
    format!("Recent conversation, for context only (do not translate):\n{ctx}\n\n{ask}:\n{utterance}")
}

pub fn reply_user(context: &[String], chinese: &str) -> String {
    let ctx = if context.is_empty() { "(none)".to_string() } else { context.join("\n") };
    format!("Recent conversation (speech-recognized, may have errors):\n{ctx}\n\nWhat I want to say (Chinese):\n{chinese}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unfinished_utterance_is_marked_as_such() {
        assert!(listen_user(&[], "so the plan is", true).contains("stops mid-sentence"));
        assert!(!listen_user(&[], "So the plan is set.", false).contains("mid-sentence"));
    }

    #[test]
    fn glossary_skips_comments_and_blanks() {
        let g = parse_glossary("# terms\n季度營收 = quarterly revenue\n\nbad line\n = x\n");
        assert_eq!(g, vec![("季度營收".to_string(), "quarterly revenue".to_string())]);
    }

    #[test]
    fn script_is_a_separate_cached_block_only_when_present() {
        assert_eq!(system_blocks("I", &[], "  ").len(), 1);
        let b = system_blocks("I", &[], "# Pitch");
        assert_eq!(b.len(), 2);
        assert_eq!(b[1]["cache_control"]["type"], "ephemeral");
    }
}
