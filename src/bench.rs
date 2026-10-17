//! `recall bench` — replay + extract session logs, report time/memory/stats.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use clap::Args;
use serde::Serialize;

use crate::alloc_counter as mem;
use crate::{extract, replay, sessions};

#[derive(Args, Debug)]
pub struct BenchArgs {
    /// Session files or directories (directories are scanned for *.jsonl).
    pub paths: Vec<PathBuf>,
    /// Enumerate all local VS Code sessions (%APPDATA%\Code\User) instead.
    #[arg(long)]
    pub local: bool,
    /// Replay only; skip text extraction.
    #[arg(long)]
    pub no_extract: bool,
    /// Max line size (bytes) to parse; larger lines are counted and skipped.
    #[arg(long)]
    pub max_line_bytes: Option<u64>,
    /// Truncate individual extracted part texts over this many chars.
    #[arg(long)]
    pub max_part_chars: Option<usize>,
    /// Machine-readable JSON output.
    #[arg(long)]
    pub json: bool,
}

#[derive(Serialize, Debug, Default, Clone)]
pub struct FileReport {
    pub path: String,
    pub size: u64,
    pub lines: u64,
    pub kind0: u64,
    pub kind1: u64,
    pub kind2: u64,
    pub kind3: u64,
    pub unknown_kind: u64,
    pub missing_initial: u64,
    pub parse_errors: u64,
    pub apply_errors: u64,
    pub oversize_lines: u64,
    pub truncated_tail: bool,
    pub max_line_bytes: u64,
    pub turns: u64,
    pub user_chars: u64,
    pub assistant_chars: u64,
    pub markdown_parts: u64,
    pub response_parts: u64,
    pub parts_truncated: u64,
    pub replay_ms: u64,
    pub extract_ms: u64,
    pub peak_bytes: u64,
}

fn human_bytes(n: u64) -> String {
    const UNITS: [(&str, u64); 4] = [
        ("GB", 1 << 30),
        ("MB", 1 << 20),
        ("KB", 1 << 10),
        ("B", 1),
    ];
    for (unit, scale) in UNITS {
        if n >= scale {
            return format!("{:.1}{}", n as f64 / scale as f64, unit);
        }
    }
    format!("{n}B")
}

pub fn run(args: BenchArgs) -> Result<()> {
    // Collect the file list.
    let files: Vec<PathBuf> = if args.local {
        let user_dir = sessions::default_user_dir()?;
        sessions::enumerate_sessions(&user_dir)?
            .into_iter()
            .map(|s| s.path)
            .collect()
    } else {
        let mut v = Vec::new();
        for p in &args.paths {
            sessions::collect_jsonl_under(p, &mut v)?;
        }
        v.sort();
        v
    };

    if files.is_empty() {
        anyhow::bail!("no .jsonl files to bench");
    }

    let run_start = Instant::now();
    let mut reports: Vec<FileReport> = Vec::with_capacity(files.len());

    for path in &files {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        mem::reset_peak();

        let t0 = Instant::now();
        let outcome = replay::replay_file(path, args.max_line_bytes)?;
        let replay_ms = t0.elapsed().as_millis() as u64;

        let mut report = FileReport {
            path: path.display().to_string(),
            size,
            lines: outcome.stats.lines,
            kind0: outcome.stats.kind_counts[0],
            kind1: outcome.stats.kind_counts[1],
            kind2: outcome.stats.kind_counts[2],
            kind3: outcome.stats.kind_counts[3],
            unknown_kind: outcome.stats.unknown_kind,
            missing_initial: outcome.stats.missing_initial,
            parse_errors: outcome.stats.parse_errors,
            apply_errors: outcome.stats.apply_errors,
            oversize_lines: outcome.stats.oversize_lines,
            truncated_tail: outcome.stats.truncated_tail,
            max_line_bytes: outcome.stats.max_line_bytes,
            replay_ms,
            peak_bytes: mem::peak() as u64,
            ..FileReport::default()
        };

        if !args.no_extract {
            let t1 = Instant::now();
            if let Some(state) = outcome.state.as_ref() {
                let opts = extract::ExtractOptions {
                    include_thinking: false,
                    max_part_chars: args.max_part_chars,
                };
                let (turns, ex) = extract::extract_session(state, &opts);
                report.turns = turns.len() as u64;
                report.user_chars = ex.user_chars;
                report.assistant_chars = ex.assistant_chars;
                report.markdown_parts = ex.markdown_parts;
                report.response_parts = ex.response_parts;
                report.parts_truncated = ex.parts_truncated;
            }
            report.extract_ms = t1.elapsed().as_millis() as u64;
            report.peak_bytes = mem::peak() as u64; // include extraction peak
        }

        if !args.json {
            println!(
                "{:>9} {:>6}L k0/k1/k2/k3={}/{}/{}/{} unk={} missInit={} perr={} aerr={} trunc={} turns={} t={}ms (+{}ms) peak={}",
                human_bytes(size),
                report.lines,
                report.kind0,
                report.kind1,
                report.kind2,
                report.kind3,
                report.unknown_kind,
                report.missing_initial,
                report.parse_errors,
                report.apply_errors,
                report.truncated_tail as u8,
                report.turns,
                report.replay_ms,
                report.extract_ms,
                human_bytes(report.peak_bytes),
            );
            println!("            {}", path.display());
        }

        reports.push(report);
    }

    let total_ms = run_start.elapsed().as_millis() as u64;

    if args.json {
        let out = serde_json::json!({
            "files": reports,
            "total_ms": total_ms,
            "peak_bytes_overall": mem::peak() as u64,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        let total_size: u64 = reports.iter().map(|r| r.size).sum();
        let total_lines: u64 = reports.iter().map(|r| r.lines).sum();
        let total_turns: u64 = reports.iter().map(|r| r.turns).sum();
        let total_user: u64 = reports.iter().map(|r| r.user_chars).sum();
        let total_asst: u64 = reports.iter().map(|r| r.assistant_chars).sum();
        let total_parse_err: u64 = reports.iter().map(|r| r.parse_errors).sum();
        let total_apply_err: u64 = reports.iter().map(|r| r.apply_errors).sum();
        let total_unknown: u64 = reports.iter().map(|r| r.unknown_kind).sum();
        let total_missing_init: u64 = reports.iter().map(|r| r.missing_initial).sum();
        let total_oversize: u64 = reports.iter().map(|r| r.oversize_lines).sum();
        let total_replay: u64 = reports.iter().map(|r| r.replay_ms).sum();
        let total_extract: u64 = reports.iter().map(|r| r.extract_ms).sum();
        let max_line: u64 = reports.iter().map(|r| r.max_line_bytes).max().unwrap_or(0);
        let max_peak: u64 = reports.iter().map(|r| r.peak_bytes).max().unwrap_or(0);
        let truncated: usize = reports.iter().filter(|r| r.truncated_tail).count();

        println!("\n== totals ==");
        println!("files        : {}", reports.len());
        println!("bytes        : {}", human_bytes(total_size));
        println!("lines        : {total_lines}");
        println!("turns        : {total_turns}");
        println!(
            "text chars   : user={} assistant={}",
            human_bytes(total_user),
            human_bytes(total_asst)
        );
        println!(
            "anomalies    : parseErr={total_parse_err} applyErr={total_apply_err} unknownKind={total_unknown} missingInitial={total_missing_init} oversize={total_oversize} truncatedTailFiles={truncated}"
        );
        println!("max line     : {}", human_bytes(max_line));
        println!("max file peak: {}", human_bytes(max_peak));
        println!(
            "time         : replay={}ms extract={}ms wall={}ms",
            total_replay, total_extract, total_ms
        );
    }

    Ok(())
}
