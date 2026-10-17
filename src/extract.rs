//! Text extraction from a replayed session object.
//!
//! Targets (verified against real files, 2026-10-18, VS Code 1.142 / copilot-chat 0.70):
//! - User text:    `requests[].message.text` (string in legacy data; objects in
//!   current data, with an optional `parts` array whose items also carry `text`).
//! - Assistant text: `requests[].response[]` — the visible prose parts are
//!   **kindless** objects `{"value":"<markdown>"}` in the current format (upstream
//!   serializes `markdownContent` by unwrapping `.content`). Legacy-wrapped
//!   `{"kind":"markdownContent","content":{"value":...}}` is also accepted.
//! - Thinking:     `{"kind":"thinking","value":...}` (optional, off by default).
//! - Everything else (`toolInvocationSerialized`, `textEditGroup`, ...) is
//!   skipped and counted by kind.

use std::collections::BTreeMap;

use serde_json::Value;

#[derive(Debug, Default, Clone)]
pub struct Turn {
    pub index: usize,
    pub request_id: Option<String>,
    pub user_text: String,
    pub assistant_text: String,
    pub thinking_text: String,
}

#[derive(Debug, Default, Clone)]
pub struct ExtractStats {
    pub requests: u64,
    pub user_chars: u64,
    pub assistant_chars: u64,
    pub thinking_chars: u64,
    pub response_parts: u64,
    /// Response parts that produced assistant text (kindless `value` forms).
    pub markdown_parts: u64,
    /// Count of response parts by kind label; kindless markdown parts are
    /// counted under `"(markdown)"`.
    pub part_kinds: BTreeMap<String, u64>,
    /// Parts whose text was truncated by `max_part_chars`.
    pub parts_truncated: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ExtractOptions {
    pub include_thinking: bool,
    /// Cap on an individual part's stored text (chars); `None` = unlimited.
    pub max_part_chars: Option<usize>,
}

pub fn extract_session(state: &Value, opts: &ExtractOptions) -> (Vec<Turn>, ExtractStats) {
    let mut stats = ExtractStats::default();
    let mut turns = Vec::new();

    let Some(requests) = state.get("requests").and_then(|r| r.as_array()) else {
        return (turns, stats);
    };

    for (index, req) in requests.iter().enumerate() {
        stats.requests += 1;
        let mut turn = Turn {
            index,
            request_id: req
                .get("requestId")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            ..Turn::default()
        };

        turn.user_text = message_text(req.get("message"), opts, &mut stats);

        match req.get("response") {
            // Legacy: whole response stored as one string.
            Some(Value::String(s)) => {
                turn.assistant_text = s.clone();
            }
            Some(Value::Array(parts)) => {
                for part in parts {
                    extract_response_part(part, opts, &mut turn, &mut stats);
                }
            }
            _ => {}
        }

        stats.user_chars += turn.user_text.chars().count() as u64;
        stats.assistant_chars += turn.assistant_text.chars().count() as u64;
        stats.thinking_chars += turn.thinking_text.chars().count() as u64;
        turns.push(turn);
    }

    (turns, stats)
}

fn message_text(message: Option<&Value>, opts: &ExtractOptions, stats: &mut ExtractStats) -> String {
    match message {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(obj)) => {
            if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                return clamp(text, opts, stats);
            }
            // Fallback: concatenate `parts[].text` in order.
            let mut out = String::new();
            if let Some(parts) = obj.get("parts").and_then(|v| v.as_array()) {
                for p in parts {
                    if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                        append_joined(&mut out, t);
                    }
                }
            }
            out
        }
        _ => String::new(),
    }
}

fn extract_response_part(
    part: &Value,
    opts: &ExtractOptions,
    turn: &mut Turn,
    stats: &mut ExtractStats,
) {
    stats.response_parts += 1;
    match part {
        Value::Object(obj) => {
            let kind = obj.get("kind").and_then(|k| k.as_str());
            match kind {
                // Current format: markdownContent serialized as its bare content.
                None => {
                    let label = if obj.contains_key("value") {
                        "(markdown)"
                    } else {
                        "(unknown-object)"
                    };
                    bump(&mut stats.part_kinds, label);
                    if let Some(v) = obj.get("value").and_then(|v| v.as_str()) {
                        stats.markdown_parts += 1;
                        let text = clamp(v, opts, stats);
                        append_joined(&mut turn.assistant_text, &text);
                    }
                }
                Some("thinking") => {
                    bump(&mut stats.part_kinds, "thinking");
                    if opts.include_thinking
                        && let Some(v) = obj.get("value").and_then(|v| v.as_str())
                    {
                        let text = clamp(v, opts, stats);
                        append_joined(&mut turn.thinking_text, &text);
                    }
                }
                // Legacy-wrapped markdown form.
                Some("markdownContent") => {
                    bump(&mut stats.part_kinds, "markdownContent");
                    let inner = obj
                        .get("content")
                        .and_then(|c| c.get("value"))
                        .and_then(|v| v.as_str())
                        .or_else(|| obj.get("value").and_then(|v| v.as_str()));
                    if let Some(v) = inner {
                        stats.markdown_parts += 1;
                        let text = clamp(v, opts, stats);
                        append_joined(&mut turn.assistant_text, &text);
                    }
                }
                Some(k) => bump(&mut stats.part_kinds, k),
            }
        }
        // Legacy: plain strings inside the response array.
        Value::String(s) => {
            bump(&mut stats.part_kinds, "(string)");
            let text = clamp(s, opts, stats);
            append_joined(&mut turn.assistant_text, &text);
        }
        _ => bump(&mut stats.part_kinds, "(other)"),
    }
}

fn bump(map: &mut BTreeMap<String, u64>, key: &str) {
    *map.entry(key.to_string()).or_insert(0) += 1;
}

fn append_joined(dst: &mut String, s: &str) {
    if s.is_empty() {
        return;
    }
    if !dst.is_empty() {
        dst.push('\n');
    }
    dst.push_str(s);
}

/// Truncate by chars when over the cap; appends a marker note.
fn clamp(text: &str, opts: &ExtractOptions, stats: &mut ExtractStats) -> String {
    match opts.max_part_chars {
        Some(limit) if text.chars().count() > limit => {
            stats.parts_truncated += 1;
            let head: String = text.chars().take(limit).collect();
            format!("{head}\n[recall: part truncated, original {} chars]", text.chars().count())
        }
        _ => text.to_string(),
    }
}
