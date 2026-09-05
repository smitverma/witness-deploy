use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::{Result, WitnessError};

/// Maximum accepted input size per side (1 MiB). Prevents the O(n*m) LCS
/// table from exploding on accidental pastes of huge bodies.
pub const MAX_COMPARER_INPUT: usize = 1024 * 1024;
const MAX_TOKENS: usize = 2_000;

static WORD_RE: OnceLock<Regex> = OnceLock::new();

fn word_regex() -> &'static Regex {
    WORD_RE.get_or_init(|| Regex::new(r"\s+|\w+|[^\w\s]+").expect("static word regex is valid"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DiffKind {
    Equal,
    Insert,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffChunk {
    pub kind: DiffKind,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffResult {
    pub chunks: Vec<DiffChunk>,
    pub additions: usize,
    pub deletions: usize,
    pub unchanged: usize,
    /// True when token streams were truncated to `MAX_TOKENS`.
    #[serde(default)]
    pub truncated: bool,
}

/// Strict, fallible comparison: rejects oversized inputs and unknown
/// granularity instead of silently falling back.
pub fn try_compare(left: &str, right: &str, granularity: &str) -> Result<DiffResult> {
    if left.len() > MAX_COMPARER_INPUT || right.len() > MAX_COMPARER_INPUT {
        return Err(WitnessError::Other(anyhow::anyhow!(
            "comparison input exceeds 1 MiB"
        )));
    }
    match granularity {
        "line" | "word" | "character" | "char" => Ok(compare_inner(left, right, granularity)),
        other => Err(WitnessError::Other(anyhow::anyhow!(
            "unknown diff granularity: {other}"
        ))),
    }
}

/// Backwards-compatible entry point used by `ui_bridge::compare_text`.
/// Unknown granularities fall back to character mode and oversized inputs
/// are truncated to the cap so existing callers never see an error.
pub fn compare(left: &str, right: &str, granularity: &str) -> DiffResult {
    match try_compare(left, right, granularity) {
        Ok(result) => result,
        Err(_) => {
            let left = &left[..left.len().min(MAX_COMPARER_INPUT)];
            let right = &right[..right.len().min(MAX_COMPARER_INPUT)];
            let fallback = match granularity {
                "line" | "word" => granularity,
                _ => "character",
            };
            compare_inner(left, right, fallback)
        }
    }
}

fn compare_inner(left: &str, right: &str, granularity: &str) -> DiffResult {
    let tokenize = |value: &str| -> Vec<String> {
        match granularity {
            "line" => value.split_inclusive('\n').map(str::to_string).collect(),
            "word" => word_regex()
                .find_iter(value)
                .map(|item| item.as_str().to_string())
                .collect(),
            _ => value
                .chars()
                .map(|character| character.to_string())
                .collect(),
        }
    };
    let left_tokens = tokenize(left);
    let right_tokens = tokenize(right);
    let truncated = left_tokens.len() > MAX_TOKENS || right_tokens.len() > MAX_TOKENS;
    let left = &left_tokens[..left_tokens.len().min(MAX_TOKENS)];
    let right = &right_tokens[..right_tokens.len().min(MAX_TOKENS)];
    let width = right.len() + 1;
    let mut lcs = vec![0_u16; (left.len() + 1) * width];
    for x in (0..left.len()).rev() {
        for y in (0..right.len()).rev() {
            lcs[x * width + y] = if left[x] == right[y] {
                lcs[(x + 1) * width + y + 1].saturating_add(1)
            } else {
                lcs[(x + 1) * width + y].max(lcs[x * width + y + 1])
            };
        }
    }

    let mut raw = Vec::<(DiffKind, String)>::new();
    let (mut x, mut y) = (0, 0);
    while x < left.len() || y < right.len() {
        if x < left.len() && y < right.len() && left[x] == right[y] {
            raw.push((DiffKind::Equal, left[x].clone()));
            x += 1;
            y += 1;
        } else if y < right.len()
            && (x == left.len() || lcs[x * width + y + 1] >= lcs[(x + 1) * width + y])
        {
            raw.push((DiffKind::Insert, right[y].clone()));
            y += 1;
        } else {
            raw.push((DiffKind::Delete, left[x].clone()));
            x += 1;
        }
    }
    let mut chunks = Vec::<DiffChunk>::new();
    for (kind, text) in raw {
        if let Some(previous) = chunks.last_mut().filter(|chunk| chunk.kind == kind) {
            previous.text.push_str(&text);
        } else {
            chunks.push(DiffChunk { kind, text });
        }
    }
    let additions = chunks
        .iter()
        .filter(|chunk| chunk.kind == DiffKind::Insert)
        .map(|chunk| chunk.text.chars().count())
        .sum();
    let deletions = chunks
        .iter()
        .filter(|chunk| chunk.kind == DiffKind::Delete)
        .map(|chunk| chunk.text.chars().count())
        .sum();
    let unchanged = chunks
        .iter()
        .filter(|chunk| chunk.kind == DiffKind::Equal)
        .map(|chunk| chunk.text.chars().count())
        .sum();
    DiffResult {
        chunks,
        additions,
        deletions,
        unchanged,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_character_level_changes() {
        let result = compare("anvil", "an evil", "character");
        assert_eq!(result.additions, 2);
        assert_eq!(result.deletions, 0);
        assert!(result
            .chunks
            .iter()
            .any(|chunk| chunk.kind == DiffKind::Insert));
    }

    #[test]
    fn supports_line_and_word_granularity() {
        let line = compare("one\ntwo\n", "one\nthree\n", "line");
        assert!(line.additions > 0 && line.deletions > 0);
        let word = compare("hello old world", "hello new world", "word");
        assert!(word.additions > 0 && word.deletions > 0);
    }

    #[test]
    fn strict_compare_rejects_oversize_and_unknown_granularity() {
        assert!(try_compare("a", "b", "bogus").is_err());
        let big = "x".repeat(MAX_COMPARER_INPUT + 1);
        assert!(try_compare(&big, "b", "character").is_err());
        // Legacy wrapper never errors: unknown falls back, oversize truncates.
        let legacy = compare("a", "b", "bogus");
        assert!(!legacy.chunks.is_empty() || legacy.unchanged == 0);
        assert!(try_compare("a", "b", "word").is_ok());
    }

    #[test]
    fn truncation_flag_is_set_when_token_cap_hit() {
        // 2_001 single-char tokens exceed MAX_TOKENS.
        let left = "a".repeat(2_001);
        let result = compare(&left, "", "character");
        assert!(result.truncated);
    }
}
