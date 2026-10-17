//! `recall dump` — print replayed turns of one session (P1 comparison aid;
//! precursor of the P2 `show` command).

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::{extract, replay};

#[derive(Args, Debug)]
pub struct DumpArgs {
    /// Session `.jsonl` file.
    pub file: PathBuf,
    /// Print only this turn index.
    #[arg(long)]
    pub turn: Option<usize>,
    /// Print full texts instead of 200-char heads.
    #[arg(long)]
    pub full: bool,
    /// Include thinking parts.
    #[arg(long)]
    pub thinking: bool,
    /// Machine-readable JSON output (always full text).
    #[arg(long)]
    pub json: bool,
}

fn head(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        out.push('…');
    }
    out
}

fn one_line(s: &str) -> String {
    s.replace('\r', "").replace('\n', "⏎")
}

pub fn run(args: DumpArgs) -> Result<()> {
    let outcome = replay::replay_file(&args.file, None)?;
    let s = &outcome.stats;

    if !args.json {
        println!("file        : {}", args.file.display());
        println!(
            "lines       : {} (k0/k1/k2/k3={}/{}/{}/{} unk={} perr={} aerr={} missInit={} truncTail={})",
            s.lines,
            s.kind_counts[0],
            s.kind_counts[1],
            s.kind_counts[2],
            s.kind_counts[3],
            s.unknown_kind,
            s.parse_errors,
            s.apply_errors,
            s.missing_initial,
            s.truncated_tail as u8
        );
    }

    let Some(state) = outcome.state.as_ref() else {
        if args.json {
            println!("{{\"error\":\"no initial state (missing kind0)\"}}");
        } else {
            println!("no initial state (missing kind0) — nothing to dump");
        }
        return Ok(());
    };

    if !args.json {
        let session_id = state.get("sessionId").and_then(|v| v.as_str()).unwrap_or("<none>");
        let title = state.get("customTitle").and_then(|v| v.as_str()).unwrap_or("<none>");
        let created = state.get("creationDate").and_then(|v| v.as_i64()).unwrap_or(0);
        println!("sessionId   : {session_id}");
        println!("customTitle : {title}");
        println!("creationDate: {created}");
    }

    let opts = extract::ExtractOptions {
        include_thinking: args.thinking,
        max_part_chars: None,
    };
    let (turns, _ex) = extract::extract_session(state, &opts);

    if !args.json {
        println!("turns       : {}", turns.len());
    }

    if args.json {
        let arr: Vec<_> = turns
            .iter()
            .filter(|t| args.turn.is_none_or(|i| i == t.index))
            .map(|t| {
                serde_json::json!({
                    "index": t.index,
                    "requestId": t.request_id,
                    "user": t.user_text,
                    "assistant": t.assistant_text,
                    "thinking": if args.thinking { Some(&t.thinking_text) } else { None },
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
        return Ok(());
    }

    for t in &turns {
        if let Some(i) = args.turn {
            if i != t.index {
                continue;
            }
        }
        let user = if args.full {
            one_line(&t.user_text)
        } else {
            one_line(&head(&t.user_text, 200))
        };
        let asst = if args.full {
            one_line(&t.assistant_text)
        } else {
            one_line(&head(&t.assistant_text, 200))
        };
        println!("--- turn {} [{:?}]", t.index, t.request_id.as_deref().unwrap_or("-"));
        println!("user: {user}");
        println!("asst: {asst}");
        if args.thinking && !t.thinking_text.is_empty() {
            let th = if args.full {
                one_line(&t.thinking_text)
            } else {
                one_line(&head(&t.thinking_text, 200))
            };
            println!("think: {th}");
        }
    }

    Ok(())
}
