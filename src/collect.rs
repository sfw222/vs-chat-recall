//! `recall collect` — scan local VS Code sessions, replay + extract, upsert
//! into the SQLite index. Incremental: a file is re-processed only when its
//! size or mtime changed (compaction rewrites change both anyway — since we
//! always re-replay whole files, correctness does not depend on detecting it).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use clap::Args;
use serde::Serialize;

use crate::db::{Db, SessionRow, TurnRow, default_db_path};
use crate::i18n::{tr, trf};
use crate::project::{ProjectCache, empty_window_project};
use crate::{extract, replay, sessions};

#[derive(Args, Debug)]
pub struct CollectArgs {
    /// Path to the SQLite index (default: data/recall.db)
    #[arg(long)]
    pub db: Option<PathBuf>,
    /// VS Code user directory (default: %APPDATA%\Code\User)
    #[arg(long)]
    pub user_dir: Option<PathBuf>,
    /// Re-process every file, ignoring the size/mtime fast path
    #[arg(long)]
    pub force: bool,
    /// Delete sessions whose source files are gone
    #[arg(long)]
    pub prune: bool,
    /// Run VACUUM after collecting (rebuilds the FTS index, reclaims space)
    #[arg(long)]
    pub vacuum: bool,
    #[arg(long)]
    pub json: bool,
    /// Print one line per processed file
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Serialize, Default, Debug)]
pub struct CollectReport {
    pub files_scanned: u64,
    pub new_sessions: u64,
    pub updated_sessions: u64,
    pub skipped_unchanged: u64,
    /// 已存在且文件变小 → 大概率发生压缩重写（1024 事件上限）
    pub rewritten_likely_compaction: u64,
    /// 无 kind0 / 无 requests 的会话数（真实存在，照常入库）
    pub empty_or_anomalous: u64,
    pub errors: u64,
    pub stale_in_db: u64,
    pub pruned: u64,
    /// 同一 sessionId 出现在多个文件（跨 hash 幽灵副本）——保留 mtime 最新者
    pub session_id_collisions: u64,
    pub turns_total: u64,
    pub elapsed_ms: u64,
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn mtime_ms(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn run(args: CollectArgs) -> Result<()> {
    let user_dir = match args.user_dir.clone() {
        Some(d) => d,
        None => sessions::default_user_dir()?,
    };
    let db_path = args.db.clone().unwrap_or_else(default_db_path);
    let report = collect(&args, &user_dir, &db_path)?;
    let db = Db::open(&db_path)?;
    if args.vacuum {
        let before = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
        db.vacuum()?;
        let after = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
        println!(
            "{}",
            trf(
                "collect.vacuum",
                &[
                    &format!("{:.1}", before as f64 / 1_000_000.0),
                    &format!("{:.1}", after as f64 / 1_000_000.0),
                ]
            )
        );
    }
    let (db_sessions, db_turns) = db.counts()?;

    if args.json {
        let out = serde_json::json!({
            "report": report,
            "db": { "path": db_path.display().to_string(), "sessions": db_sessions, "turns": db_turns }
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("{}", trf("collect.scanned", &[&report.files_scanned.to_string()]));
        println!(
            "{}",
            trf(
                "collect.new_updated",
                &[
                    &report.new_sessions.to_string(),
                    &report.updated_sessions.to_string(),
                    &report.skipped_unchanged.to_string(),
                ]
            )
        );
        println!(
            "{}",
            trf(
                "collect.rewrite",
                &[
                    &report.rewritten_likely_compaction.to_string(),
                    &report.empty_or_anomalous.to_string(),
                    &report.errors.to_string(),
                ]
            )
        );
        println!(
            "{}",
            trf("collect.dup_ids", &[&report.session_id_collisions.to_string()])
        );
        let stale_note = if args.prune {
            trf("collect.stale.pruned", &[&report.pruned.to_string()])
        } else {
            tr("collect.stale.hint").to_string()
        };
        println!(
            "{}",
            trf(
                "collect.stale",
                &[&report.stale_in_db.to_string(), &stale_note]
            )
        );
        println!(
            "{}",
            trf(
                "collect.elapsed",
                &[&report.elapsed_ms.to_string(), &report.turns_total.to_string()]
            )
        );
        println!(
            "{}",
            trf(
                "collect.db",
                &[
                    &db_path.display().to_string(),
                    &db_sessions.to_string(),
                    &db_turns.to_string(),
                ]
            )
        );
    }
    Ok(())
}

/// Core collection pass (used by `run` and integration tests).
pub fn collect(args: &CollectArgs, user_dir: &Path, db_path: &Path) -> Result<CollectReport> {
    let mut db = Db::open(db_path)?;

    let files = sessions::enumerate_sessions(user_dir)?;
    let index = db.file_index()?;
    let mut cache = ProjectCache::default();
    let mut seen_paths: HashSet<String> = HashSet::new();
    // sessionId → (path, mtime) seen in THIS pass, for cross-hash duplicate ids.
    let mut seen_ids: std::collections::HashMap<String, (String, i64)> =
        std::collections::HashMap::new();
    let mut report = CollectReport::default();
    let start = Instant::now();

    for f in &files {
        let path_str = f.path.to_string_lossy().into_owned();
        seen_paths.insert(path_str.clone());
        report.files_scanned += 1;

        let file_size = f.size as i64;
        let file_mtime = mtime_ms(&f.path);

        let prev = index.get(&path_str);
        if !args.force
            && let Some((psize, pmtime, pid)) = prev
            && *psize == file_size
            && *pmtime == file_mtime
        {
            report.skipped_unchanged += 1;
            // Register the skipped file as this session's current copy so
            // a duplicate-id file processed later compares against it.
            seen_ids
                .entry(pid.clone())
                .or_insert((path_str.clone(), file_mtime));
            continue;
        }

        let outcome = match replay::replay_file(&f.path, None) {
            Ok(o) => o,
            Err(e) => {
                report.errors += 1;
                eprintln!("ERROR {}: {e:#}", f.path.display());
                continue;
            }
        };

        let session_id = outcome
            .state
            .as_ref()
            .and_then(|s| s.get("sessionId"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| {
                f.path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });

        let (title, created_ms) = match outcome.state.as_ref() {
            Some(s) => (
                s.get("customTitle")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                s.get("creationDate").and_then(|v| v.as_i64()),
            ),
            None => (None, None),
        };

        // Cross-hash duplicate session ids (same session file under two hashes,
        // e.g. after switching folder ↔ .code-workspace): keep the newest.
        if let Some((prev_path, prev_mtime)) = seen_ids.get(&session_id) {
            report.session_id_collisions += 1;
            if file_mtime > *prev_mtime {
                if args.verbose {
                    println!(
                        "DUP  {}: {} 较新，覆盖 {}",
                        session_id,
                        f.path.display(),
                        prev_path
                    );
                }
            } else {
                if args.verbose {
                    println!("DUP  {}: 跳过旧副本 {}", session_id, f.path.display());
                }
                continue;
            }
        }
        seen_ids.insert(session_id.clone(), (path_str.clone(), file_mtime));

        let project = if f.is_empty_window() {
            empty_window_project()
        } else {
            cache
                .get(user_dir, f.workspace_hash.as_deref().unwrap_or(""))
                .clone()
        };

        let turns: Vec<TurnRow> = match outcome.state.as_ref() {
            Some(state) => {
                let opts = extract::ExtractOptions::default();
                let (turns, _stats) = extract::extract_session(state, &opts);
                turns
                    .into_iter()
                    .map(|t| TurnRow {
                        idx: t.index as i64,
                        request_id: t.request_id,
                        user_text: t.user_text,
                        assistant_text: t.assistant_text,
                    })
                    .collect()
            }
            None => Vec::new(),
        };

        if turns.is_empty() {
            report.empty_or_anomalous += 1;
        }
        report.turns_total += turns.len() as u64;

        let row = SessionRow {
            id: session_id.clone(),
            source_path: path_str.clone(),
            workspace_hash: f.workspace_hash.clone(),
            is_empty_window: f.is_empty_window(),
            project_key: project.key.clone(),
            project_label: project.label.clone(),
            open_target: project.open_target.clone(),
            title: title.clone(),
            created_ms,
            file_size,
            file_mtime_ms: file_mtime,
            turns: turns.len() as i64,
        };

        match prev {
            Some((psize, _, _)) => {
                report.updated_sessions += 1;
                if file_size < *psize {
                    report.rewritten_likely_compaction += 1;
                }
            }
            None => report.new_sessions += 1,
        }

        db.upsert_session(&row, &turns, now_ms())?;

        if args.verbose {
            println!(
                "{}  turns={}  {}  [{}]",
                if prev.is_some() { "UPDATE" } else { "NEW   " },
                turns.len(),
                session_id,
                project.label
            );
        }
    }

    // stale sessions: in db but source file gone
    let stale: Vec<String> = index
        .keys()
        .filter(|p| !seen_paths.contains(*p))
        .cloned()
        .collect();
    report.stale_in_db = stale.len() as u64;
    if args.prune && !stale.is_empty() {
        report.pruned = db.prune_missing(&seen_paths)? as u64;
    }

    report.elapsed_ms = start.elapsed().as_millis() as u64;
    Ok(report)
}
