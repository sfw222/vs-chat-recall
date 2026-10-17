//! Regression tests over the synthetic boundary fixtures
//! (`fixtures/synthetic/`), which are committed to git.
//!
//! Each `.jsonl` fixture has an `expected/<name>.json` describing the turns,
//! anomaly counters, and (optionally) session-level facts. `legacy-v3.json`
//! is a whole-document JSON (not a mutation log) and goes straight to the
//! extractor; `huge-line.jsonl` has programmatic assertions (its 1 MiB
//! strings cannot live in an expected JSON file readably).

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use vs_chat_recall::{extract, replay};

fn syn_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join("synthetic")
}

#[derive(Deserialize)]
struct Expected {
    turns: Vec<ExpTurn>,
    #[serde(default)]
    stats: ExpStats,
    session: ExpSession,
}

#[derive(Deserialize)]
struct ExpTurn {
    user: String,
    assistant: String,
}

#[derive(Deserialize, Default)]
struct ExpStats {
    #[serde(default)]
    unknown_kind: u64,
    #[serde(default)]
    parse_errors: u64,
    #[serde(default)]
    apply_errors: u64,
    #[serde(default)]
    missing_initial: u64,
    #[serde(default)]
    truncated_tail: bool,
    #[serde(default)]
    oversize_lines: u64,
}

#[derive(Deserialize)]
struct ExpSession {
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
}

fn load_expected(name: &str) -> Expected {
    let p = syn_dir().join("expected").join(format!("{name}.json"));
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", p.display()))
}

fn extract_turns(state: &Value) -> Vec<(String, String)> {
    let (turns, _) = extract::extract_session(state, &extract::ExtractOptions::default());
    turns
        .into_iter()
        .map(|t| (t.user_text, t.assistant_text))
        .collect()
}

fn assert_fixture(name: &str) {
    let expected = load_expected(name);
    let path = syn_dir().join(format!("{name}.jsonl"));
    let outcome = replay::replay_file(&path, None).unwrap_or_else(|e| panic!("{name}: {e}"));
    let s = &outcome.stats;

    assert_eq!(s.unknown_kind, expected.stats.unknown_kind, "{name}: unknown_kind");
    assert_eq!(s.parse_errors, expected.stats.parse_errors, "{name}: parse_errors");
    assert_eq!(s.apply_errors, expected.stats.apply_errors, "{name}: apply_errors");
    assert_eq!(s.missing_initial, expected.stats.missing_initial, "{name}: missing_initial");
    assert_eq!(s.truncated_tail, expected.stats.truncated_tail, "{name}: truncated_tail");
    assert_eq!(s.oversize_lines, expected.stats.oversize_lines, "{name}: oversize_lines");

    let state = outcome.state.unwrap_or_else(|| panic!("{name}: no state"));

    let sid = state.get("sessionId").and_then(|v| v.as_str());
    assert_eq!(sid, Some(expected.session.session_id.as_str()), "{name}: sessionId");

    let title = state.get("customTitle").and_then(|v| v.as_str()).map(str::to_string);
    assert_eq!(title, expected.session.custom_title, "{name}: customTitle");

    let got = extract_turns(&state);
    let want: Vec<(String, String)> = expected
        .turns
        .iter()
        .map(|t| (t.user.clone(), t.assistant.clone()))
        .collect();
    assert_eq!(got, want, "{name}: turns");
}

#[test]
fn all_jsonl_fixtures_match_expected() {
    for name in [
        "initial-only",
        "set-nested",
        "push-append",
        "push-insert",
        "push-truncate",
        "delete-restore",
        "compact-rewrite",
        "partial-tail",
        "unknown-kind",
    ] {
        assert_fixture(name);
    }
}

#[test]
fn set_nested_deep_value() {
    let path = syn_dir().join("set-nested.jsonl");
    let outcome = replay::replay_file(&path, None).unwrap();
    let state = outcome.state.unwrap();
    let got = replay::get_in(&state, &replay::path(&["requests", "0", "customField", "b"]));
    assert_eq!(got, Some(&Value::from(42)));
    // message was replaced wholesale by kind:1
    let msg = replay::get_in(&state, &replay::path(&["requests", "0", "message"]));
    assert_eq!(msg.and_then(|v| v.get("text")).and_then(|v| v.as_str()), Some("hello v2"));
}

#[test]
fn push_creates_missing_array_and_holes_extend_with_null() {
    // push-truncate fixture also pushes a brand-new top-level array.
    let path = syn_dir().join("push-truncate.jsonl");
    let outcome = replay::replay_file(&path, None).unwrap();
    let state = outcome.state.unwrap();
    let arr = replay::get_in(&state, &replay::path(&["brandNewArray"])).cloned();
    assert_eq!(arr, Some(serde_json::json!([1, 2])));

    // JS `arr.length = i` with i > len extends with holes (null in JSON).
    let log = concat!(
        r#"{"kind":0,"v":{"version":3,"sessionId":"t","requests":[{"message":{"text":"u1"},"response":[{"value":"a1"}]}]}}"#,
        "\n",
        r#"{"kind":2,"k":["requests"],"i":3,"v":[{"message":{"text":"ux"},"response":[{"value":"ax"}]}]}"#,
        "\n",
    );
    let outcome = replay::replay_bytes(log.as_bytes(), None);
    let state = outcome.state.unwrap();
    let turns = extract_turns(&state);
    assert_eq!(turns.len(), 4);
    assert_eq!(turns[0].1, "a1");
    assert_eq!(turns[3].1, "ax");
    // The two holes carry no texts.
    assert_eq!(turns[1].0, "");
    assert_eq!(turns[2].0, "");
}

#[test]
fn delete_restore_vote() {
    let path = syn_dir().join("delete-restore.jsonl");
    let outcome = replay::replay_file(&path, None).unwrap();
    let state = outcome.state.unwrap();
    let vote = replay::get_in(&state, &replay::path(&["requests", "0", "vote"])).cloned();
    assert_eq!(vote, Some(Value::from("down")));
}

#[test]
fn push_append_thinking_and_part_kinds() {
    let path = syn_dir().join("push-append.jsonl");
    let outcome = replay::replay_file(&path, None).unwrap();
    let state = outcome.state.unwrap();

    // Default: thinking excluded from assistant text.
    let (turns, stats) = extract::extract_session(&state, &extract::ExtractOptions::default());
    assert_eq!(turns[2].thinking_text, "");
    assert_eq!(stats.part_kinds.get("toolInvocationSerialized"), Some(&1));

    // With thinking enabled.
    let opts = extract::ExtractOptions {
        include_thinking: true,
        max_part_chars: None,
    };
    let (turns2, _) = extract::extract_session(&state, &opts);
    assert_eq!(turns2[2].thinking_text, "t3");
    assert_eq!(turns2[2].assistant_text, "a3");
}

#[test]
fn missing_initial_is_counted_not_fatal() {
    let log = "{\"kind\":1,\"k\":[\"customTitle\"],\"v\":\"x\"}\n";
    let outcome = replay::replay_bytes(log.as_bytes(), None);
    assert_eq!(outcome.stats.missing_initial, 1);
    assert!(outcome.state.is_none());
}

#[test]
fn huge_line_fixture() {
    let path = syn_dir().join("huge-line.jsonl");

    // Default: everything parsed.
    let outcome = replay::replay_file(&path, None).unwrap();
    assert_eq!(outcome.stats.oversize_lines, 0);
    assert_eq!(outcome.stats.parse_errors, 0);
    let state = outcome.state.unwrap();
    let turns = extract_turns(&state);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0], ("u1".into(), "a1".into()));
    assert_eq!(turns[1].0, "u2");
    assert_eq!(turns[1].1.chars().count(), 1_048_576);

    // Part clamp: truncate the 1 MiB assistant text to 1000 chars.
    let opts = extract::ExtractOptions {
        include_thinking: false,
        max_part_chars: Some(1000),
    };
    let (_turns, stats) = extract::extract_session(&state, &opts);
    assert_eq!(stats.parts_truncated, 1);

    // Line-size skip: with a 500 KB cap both big lines are skipped.
    let outcome = replay::replay_file(&path, Some(500_000)).unwrap();
    assert_eq!(outcome.stats.oversize_lines, 2);
    let state = outcome.state.unwrap();
    let turns = extract_turns(&state);
    assert_eq!(turns.len(), 1);
    assert!(replay::get_in(&state, &replay::path(&["requests", "0", "result"])).is_none());
}

#[test]
fn legacy_v3_document() {
    let expected = load_expected("legacy-v3");
    let path = syn_dir().join("legacy-v3.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let state: Value = serde_json::from_str(&text).unwrap();

    let sid = state.get("sessionId").and_then(|v| v.as_str());
    assert_eq!(sid, Some(expected.session.session_id.as_str()));

    let got = extract_turns(&state);
    let want: Vec<(String, String)> = expected
        .turns
        .iter()
        .map(|t| (t.user.clone(), t.assistant.clone()))
        .collect();
    assert_eq!(got, want);
}
