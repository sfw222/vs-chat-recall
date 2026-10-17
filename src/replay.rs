//! Streaming replay of VS Code chat session operation logs (`.jsonl`).
//!
//! Format authority: upstream `objectMutationLog.ts` / `chatSessionOperationLog.ts`
//! (verified 2026-10-18 against microsoft/vscode `main` and real local files).
//!
//! Entry kinds:
//! - 0 Initial: `{"kind":0,"v":<full session object>}` — replaces all state.
//!   (Also appears mid-file: a compaction rewrite produces a fresh Initial.)
//! - 1 Set:     `{"kind":1,"k":[path...],"v":<value>}` — assign at path.
//! - 2 Push:    `{"kind":2,"k":[path...],"i"?:<index>,"v"?:[items]}` —
//!   if `i` is present the array is first truncated to length `i`, then all
//!   items of `v` are appended (upstream: `arr.length = i; arr.push(...v)`).
//! - 3 Delete:  `{"kind":3,"k":[path...]}` — equivalent to Set with value
//!   `undefined` (upstream applies it via `_applySet`); for objects this
//!   removes the key, for arrays the slot becomes JSON `null`.
//!
//! Wide-in policy: unknown kinds, malformed lines, and path/type apply errors
//! are counted and skipped instead of aborting the whole file (upstream throws;
//! we must survive concurrent writes and unknown future format changes).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

#[derive(Debug, Default, Clone)]
pub struct ReplayStats {
    /// Non-empty lines seen.
    pub lines: u64,
    pub blank_lines: u64,
    /// Count per kind index (0..=3).
    pub kind_counts: [u64; 4],
    /// Lines whose `kind` is missing, non-numeric, or outside 0..=3.
    pub unknown_kind: u64,
    /// Set/Push/Delete applied before any Initial was seen (upstream throws here).
    pub missing_initial: u64,
    /// Whole-line JSON parse failures (skipped).
    pub parse_errors: u64,
    /// Path/type errors while applying an otherwise valid entry.
    pub apply_errors: u64,
    /// Lines skipped because they exceeded the configured max line size.
    pub oversize_lines: u64,
    /// True when the last non-empty line had no trailing newline and failed to
    /// parse — signature of an interrupted concurrent write.
    pub truncated_tail: bool,
    /// Total bytes consumed from the file.
    pub bytes_read: u64,
    /// Longest single line seen (bytes).
    pub max_line_bytes: u64,
}

pub struct ReplayOutcome {
    /// Final replayed session object (`None` if no Initial was ever seen).
    pub state: Option<Value>,
    pub stats: ReplayStats,
}

/// Replay a session log file. Read-only; never fails on malformed content.
pub fn replay_file(path: &Path, max_line_bytes: Option<u64>) -> Result<ReplayOutcome> {
    let f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = BufReader::with_capacity(1 << 20, f);
    Ok(replay_reader(reader, max_line_bytes))
}

pub fn replay_reader<R: BufRead>(mut reader: R, max_line_bytes: Option<u64>) -> ReplayOutcome {
    let mut stats = ReplayStats::default();
    let mut state: Option<Value> = None;
    let mut buf: Vec<u8> = Vec::with_capacity(1 << 16);
    let mut last_nonempty_had_newline = true;

    loop {
        buf.clear();
        let n = match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break, // unreadable tail: stop, keep what we have
        };
        stats.bytes_read += n as u64;

        let had_newline = buf.last() == Some(&b'\n');
        let mut line: &[u8] = &buf;
        if had_newline {
            line = &line[..line.len() - 1];
        }
        if line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        if line.is_empty() {
            stats.blank_lines += 1;
            continue;
        }

        stats.lines += 1;
        stats.max_line_bytes = stats.max_line_bytes.max(line.len() as u64);
        last_nonempty_had_newline = had_newline;

        if let Some(limit) = max_line_bytes {
            if line.len() as u64 > limit {
                stats.oversize_lines += 1;
                continue;
            }
        }

        let entry: Value = match serde_json::from_slice(line) {
            Ok(v) => v,
            Err(_) => {
                stats.parse_errors += 1;
                continue;
            }
        };
        let Value::Object(mut obj) = entry else {
            stats.parse_errors += 1;
            continue;
        };

        let kind = match obj.remove("kind").and_then(|k| k.as_u64()) {
            Some(k) => k,
            None => {
                stats.unknown_kind += 1;
                continue;
            }
        };

        match kind {
            0 => {
                stats.kind_counts[0] += 1;
                match obj.remove("v") {
                    Some(v) => state = Some(v),
                    None => stats.apply_errors += 1,
                }
            }
            1 => {
                stats.kind_counts[1] += 1;
                let path = obj.remove("k");
                let value = obj.remove("v");
                match path {
                    Some(p) => apply_set(&mut state, &p, value, &mut stats),
                    None => stats.apply_errors += 1,
                }
            }
            2 => {
                stats.kind_counts[2] += 1;
                let path = obj.remove("k");
                let values = obj.remove("v");
                let index = obj.remove("i");
                match path {
                    Some(p) => apply_push(&mut state, &p, values, index, &mut stats),
                    None => stats.apply_errors += 1,
                }
            }
            3 => {
                stats.kind_counts[3] += 1;
                let path = obj.remove("k");
                match path {
                    Some(p) => apply_set(&mut state, &p, None, &mut stats),
                    None => stats.apply_errors += 1,
                }
            }
            _ => stats.unknown_kind += 1,
        }
    }

    if !last_nonempty_had_newline && stats.parse_errors > 0 {
        stats.truncated_tail = true;
    }

    ReplayOutcome { state, stats }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Seg {
    Key(String),
    Idx(usize),
}

fn parse_path(v: &Value) -> Option<Vec<Seg>> {
    let arr = v.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for s in arr {
        match s {
            Value::String(k) => out.push(Seg::Key(k.clone())),
            Value::Number(n) => out.push(Seg::Idx(n.as_u64()? as usize)),
            _ => return None,
        }
    }
    Some(out)
}

/// Walk to the parent object/array of the last path segment.
/// Returns the parent and a clone of the last segment.
fn walk_parent<'a>(
    root: &'a mut Value,
    segs: &[Seg],
    stats: &mut ReplayStats,
) -> Option<(&'a mut Value, Seg)> {
    let (last, parents) = segs.split_last()?;
    let mut cur = root;
    for seg in parents {
        match seg {
            Seg::Key(k) => match cur.get_mut(k.as_str()) {
                Some(next) => cur = next,
                None => {
                    stats.apply_errors += 1;
                    return None;
                }
            },
            Seg::Idx(i) => match cur.get_mut(*i) {
                Some(next) => cur = next,
                None => {
                    stats.apply_errors += 1;
                    return None;
                }
            },
        }
    }
    Some((cur, last.clone()))
}

fn apply_set(
    state: &mut Option<Value>,
    path: &Value,
    value: Option<Value>,
    stats: &mut ReplayStats,
) {
    let Some(segs) = parse_path(path) else {
        stats.apply_errors += 1;
        return;
    };
    if segs.is_empty() {
        stats.apply_errors += 1;
        return;
    }
    let Some(root) = state.as_mut() else {
        stats.missing_initial += 1;
        return;
    };
    let Some((parent, last)) = walk_parent(root, &segs, stats) else {
        return;
    };
    match (&last, parent) {
        (Seg::Key(k), Value::Object(map)) => match value {
            Some(v) => {
                map.insert(k.clone(), v);
            }
            None => {
                // Delete semantics (upstream `current[k] = undefined`).
                map.remove(k.as_str());
            }
        },
        (Seg::Idx(i), Value::Array(arr)) => {
            if *i < arr.len() {
                // JS `arr[i] = undefined` keeps the length; JSON re-serializes
                // it as null, so null is the faithful in-memory representation.
                arr[*i] = value.unwrap_or(Value::Null);
            } else {
                stats.apply_errors += 1;
            }
        }
        _ => stats.apply_errors += 1,
    }
}

fn apply_push(
    state: &mut Option<Value>,
    path: &Value,
    values: Option<Value>,
    index: Option<Value>,
    stats: &mut ReplayStats,
) {
    let Some(segs) = parse_path(path) else {
        stats.apply_errors += 1;
        return;
    };
    if segs.is_empty() {
        stats.apply_errors += 1;
        return;
    }
    let truncate_at = match index {
        None => None,
        Some(Value::Number(n)) => match n.as_u64() {
            Some(i) => Some(i as usize),
            None => {
                stats.apply_errors += 1;
                return;
            }
        },
        Some(_) => {
            stats.apply_errors += 1;
            return;
        }
    };
    let Some(root) = state.as_mut() else {
        stats.missing_initial += 1;
        return;
    };
    let Some((parent, last)) = walk_parent(root, &segs, stats) else {
        return;
    };

    // Resolve (or create) the target array (upstream: `const arr = current[k] || []`).
    let arr: &mut Vec<Value> = match (&last, parent) {
        (Seg::Key(k), Value::Object(map)) => {
            if !map.contains_key(k.as_str()) {
                map.insert(k.clone(), Value::Array(Vec::new()));
            }
            match map.get_mut(k.as_str()) {
                Some(Value::Array(a)) => a,
                _ => {
                    stats.apply_errors += 1;
                    return;
                }
            }
        }
        (Seg::Idx(i), Value::Array(outer)) => match outer.get_mut(*i) {
            Some(Value::Array(a)) => a,
            _ => {
                stats.apply_errors += 1;
                return;
            }
        },
        _ => {
            stats.apply_errors += 1;
            return;
        }
    };

    if let Some(i) = truncate_at {
        if i > arr.len() {
            // JS `arr.length = i` extends with holes (serialized as null).
            arr.resize(i, Value::Null);
        }
        arr.truncate(i);
    }
    if let Some(v) = values {
        match v {
            Value::Array(items) => arr.extend(items),
            _ => stats.apply_errors += 1,
        }
    }
}

/// Convenience for callers that want a replay from bytes (tests).
pub fn replay_bytes(bytes: &[u8], max_line_bytes: Option<u64>) -> ReplayOutcome {
    let reader = std::io::BufReader::new(std::io::Cursor::new(bytes));
    replay_reader(reader, max_line_bytes)
}

/// Extract a nested value by path (used by tests).
pub fn get_in<'a>(v: &'a Value, path: &[Seg]) -> Option<&'a Value> {
    let mut cur = v;
    for seg in path {
        cur = match seg {
            Seg::Key(k) => cur.get(k.as_str())?,
            Seg::Idx(i) => cur.get(*i)?,
        };
    }
    Some(cur)
}

/// Public path helper for tests: `path(&["requests", "0", "response"])`
/// where all-numeric segments are treated as array indices.
pub fn path(parts: &[&str]) -> Vec<Seg> {
    parts
        .iter()
        .map(|p| match p.parse::<usize>() {
            Ok(i) => Seg::Idx(i),
            Err(_) => Seg::Key((*p).to_string()),
        })
        .collect()
}
