//! `recall projects` — list merged projects (folder/workspace dual-hash keys).
//!
//! Two different root-path sets can share a label (e.g. two "FModel" entries:
//! the folder form and a multi-root workspace). Duplicate labels are shown
//! with a short root hint taken from the project key.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::db::{Db, default_db_path};
use crate::i18n::{tr, trf};

#[derive(Args, Debug)]
pub struct ProjectsArgs {
    #[arg(long)]
    pub db: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

fn root_hint(key: &str) -> String {
    if key == "<empty-window>" {
        return String::new();
    }
    if key.starts_with("hash:") {
        return key.to_string();
    }
    // All roots, each shortened to its last two path components — duplicate
    // labels are then distinguished by their root sets (e.g. folder vs
    // multi-root workspace).
    key.split('\n')
        .map(|root| {
            let parts: Vec<&str> = root.split('\\').filter(|s| !s.is_empty()).collect();
            let take = parts.len().min(2);
            parts[parts.len() - take..].join("\\")
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

pub fn run(args: ProjectsArgs) -> Result<()> {
    let db = Db::open(&args.db.clone().unwrap_or_else(default_db_path))?;
    let projects = db.project_summaries()?;

    let mut label_counts = std::collections::HashMap::new();
    for (_, label, _, _) in &projects {
        *label_counts.entry(label.clone()).or_insert(0usize) += 1;
    }
    let display_label = |key: &str, label: &str| -> String {
        if label_counts.get(label).copied().unwrap_or(0) > 1 {
            let hint = root_hint(key);
            if hint.is_empty() {
                label.to_string()
            } else {
                format!("{label} ({hint})")
            }
        } else {
            label.to_string()
        }
    };

    if args.json {
        let out: Vec<_> = projects
            .iter()
            .map(|(key, label, sessions, turns)| {
                serde_json::json!({
                    "key": key,
                    "label": label,
                    "display": display_label(key, label),
                    "sessions": sessions,
                    "turns": turns,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!(
        "{:<44} {:>8} {:>8}",
        tr("projects.col_project"),
        tr("projects.col_sessions"),
        tr("projects.col_turns")
    );
    println!("{}", "─".repeat(62));
    let mut total_sessions = 0i64;
    let mut total_turns = 0i64;
    for (key, label, sessions, turns) in &projects {
        println!(
            "{:<44} {:>8} {:>8}",
            display_label(key, label),
            sessions,
            turns
        );
        total_sessions += sessions;
        total_turns += turns;
    }
    println!("{}", "─".repeat(62));
    println!(
        "{:<44} {:>8} {:>8}",
        trf("projects.total", &[&projects.len().to_string()]),
        total_sessions,
        total_turns
    );
    Ok(())
}
