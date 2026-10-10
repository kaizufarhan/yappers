//! Light, rule-based cleanup of transcripts: filler words and stutters.
//!
//! The filler list is the user's to edit. The defaults are deliberately
//! conservative: words like "like" or "so" are often meaningful, so they're
//! only removed if the user adds them.

use regex::Regex;
use std::sync::{LazyLock, Mutex};

/// Fillers removed out of the box. Every letter may repeat when matching, so
/// "um" also catches "umm" and "uum", and "hm" catches "hmmm".
pub const DEFAULT_FILLERS: &[&str] = &["um", "uh", "uhm", "erm", "eh", "ah", "hm", "mhm", "mm"];

static MULTI_SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());
static SPACE_BEFORE_PUNCT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+([,.!?;:])").unwrap());
static DOUBLE_COMMA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r",\s*([,.!?])").unwrap());
static LEADING_PUNCT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\s,.;:]+").unwrap());

/// The compiled pattern for the last filler list used. The live preview cleans
/// text several times a second, so it shouldn't recompile every time.
static FILLER_CACHE: Mutex<Option<(Vec<String>, Option<Regex>)>> = Mutex::new(None);

pub fn default_fillers() -> Vec<String> {
    DEFAULT_FILLERS.iter().map(|s| s.to_string()).collect()
}

/// Tidies a user-entered filler: lowercase, trimmed, single-spaced.
pub fn normalize_filler(filler: &str) -> String {
    filler.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Cleans a transcript. `fillers` is the list to remove, or `None` when filler
/// removal is switched off (which also leaves stutters alone).
pub fn clean(text: &str, fillers: Option<&[String]>) -> String {
    let mut out = text.trim().to_string();
    if let Some(fillers) = fillers {
        if let Some(regex) = filler_regex(fillers) {
            out = remove_filler_words(&out, &regex);
        }
        out = collapse_repeats(&out);
    }
    out = MULTI_SPACE.replace_all(&out, " ").into_owned();
    out = SPACE_BEFORE_PUNCT.replace_all(&out, "$1").into_owned();
    out = DOUBLE_COMMA.replace_all(&out, "$1").into_owned();
    out = LEADING_PUNCT.replace_all(&out, "").into_owned();
    capitalize_first(out.trim())
}

fn filler_regex(fillers: &[String]) -> Option<Regex> {
    let mut cache = FILLER_CACHE.lock().unwrap();
    if let Some((list, regex)) = cache.as_ref() {
        if list == fillers {
            return regex.clone();
        }
    }
    let regex = build_filler_regex(fillers);
    *cache = Some((fillers.to_vec(), regex.clone()));
    regex
}

fn build_filler_regex(fillers: &[String]) -> Option<Regex> {
    let mut entries: Vec<String> = fillers.iter().map(|f| normalize_filler(f)).filter(|f| !f.is_empty()).collect();
    // Longest first, so "you know what" wins over "you know".
    entries.sort_by_key(|f| std::cmp::Reverse(f.len()));
    entries.dedup();
    if entries.is_empty() {
        return None;
    }
    let alternatives: Vec<String> = entries.iter().map(|f| filler_pattern(f)).collect();
    // Each filler also swallows a trailing comma or ellipsis, but not a lone
    // period: "It works, you know." must keep its full stop.
    Regex::new(&format!(r"(?i)(?:{})(?:,|…|\.{{2,}})*\s*", alternatives.join("|"))).ok()
}

/// One filler as a regex: letters may repeat ("um" → "u+m+"), words may be
/// separated by any whitespace, and the whole thing must stand alone (word
/// boundaries), so "um" never matches inside "umbrella".
fn filler_pattern(filler: &str) -> String {
    let words: Vec<String> = filler
        .split(' ')
        .map(|word| {
            word.chars()
                .map(|c| {
                    let escaped = regex::escape(&c.to_string());
                    if c.is_alphabetic() { format!("{escaped}+") } else { escaped }
                })
                .collect()
        })
        .collect();
    // \b only works next to a word character; "c++" can't end on one.
    let boundary = |c: Option<char>| if c.is_some_and(char::is_alphanumeric) { r"\b" } else { "" };
    format!("{}{}{}", boundary(filler.chars().next()), words.join(r"\s+"), boundary(filler.chars().last()))
}

/// Removes fillers. When a filler started a sentence ("Friday. Hmm, and
/// then…"), the word that now starts the sentence gets capitalized.
fn remove_filler_words(text: &str, fillers: &Regex) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut capitalize_at = Vec::new();
    for m in fillers.find_iter(text) {
        out.push_str(&text[last..m.start()]);
        let before = out.trim_end();
        if before.is_empty() || before.ends_with(['.', '!', '?']) {
            capitalize_at.push(out.len());
        }
        last = m.end();
    }
    out.push_str(&text[last..]);
    for pos in capitalize_at.into_iter().rev() {
        if let Some(c) = out[pos..].chars().next().filter(|c| c.is_lowercase()) {
            out.replace_range(pos..pos + c.len_utf8(), &c.to_uppercase().to_string());
        }
    }
    out
}

/// Collapses immediate stutters like "I I think" or "the the" into one word.
fn collapse_repeats(text: &str) -> String {
    let mut words: Vec<&str> = Vec::new();
    for word in text.split_whitespace() {
        if let Some(prev) = words.last() {
            let norm = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
            // Only collapse when the earlier word carries no punctuation
            // ("no, no" and "very, very" can be intentional).
            if norm(prev) == norm(word) && !norm(word).is_empty() && prev.chars().all(|c| c.is_alphanumeric() || c == '\'') {
                words.pop();
            }
        }
        words.push(word);
    }
    words.join(" ")
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{clean, default_fillers};

    fn defaults(text: &str) -> String {
        clean(text, Some(&default_fillers()))
    }

    fn with(list: &[&str], text: &str) -> String {
        let list: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        clean(text, Some(&list))
    }

    #[test]
    fn removes_fillers() {
        assert_eq!(defaults("Um, I think, uh, we should go."), "I think, we should go.");
        assert_eq!(defaults("So umm yeah"), "So yeah");
        assert_eq!(defaults("uh hello"), "Hello");
        assert_eq!(defaults("Hmmm, mhm, okay."), "Okay.");
    }

    #[test]
    fn recapitalizes_sentence_after_filler() {
        assert_eq!(
            defaults("Ship it on Friday. Hmm, and then write notes."),
            "Ship it on Friday. And then write notes."
        );
    }

    #[test]
    fn keeps_real_words() {
        assert_eq!(defaults("I like the umbrella."), "I like the umbrella.");
        assert_eq!(defaults("Hummus is great."), "Hummus is great.");
    }

    #[test]
    fn collapses_stutters() {
        assert_eq!(defaults("I I think the the plan works."), "I think the plan works.");
        assert_eq!(defaults("No, no, that's wrong."), "No, no, that's wrong.");
    }

    #[test]
    fn disabled_keeps_fillers() {
        assert_eq!(clean("um hello", None), "Um hello");
    }

    #[test]
    fn custom_words_and_phrases() {
        assert_eq!(with(&["like"], "It was, like, really good."), "It was, really good.");
        assert_eq!(with(&["you know"], "You know, it works. It works, you know."), "It works. It works.");
        assert_eq!(with(&["basically"], "Basically we ship."), "We ship.");
        assert_eq!(defaults("I think um... yes."), "I think yes.");
        // Whole words only.
        assert_eq!(with(&["like"], "I liked it."), "I liked it.");
    }

    #[test]
    fn custom_entries_are_normalized_and_escaped() {
        assert_eq!(with(&["  You   KNOW "], "you know it"), "It");
        assert_eq!(with(&["c++"], "I write c++ daily"), "I write daily");
    }

    #[test]
    fn empty_list_only_collapses_stutters() {
        assert_eq!(with(&[], "um I I think"), "Um I think");
    }
}
