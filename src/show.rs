//! `recall show` — print the full text of one session from the index.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Args;

use crate::db::{Db, default_db_path};
use crate::i18n::{tr, trf};
use crate::util::fmt_ms_local;

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// Session id or a unique prefix
    pub session: String,
    /// Only this turn
    #[arg(long)]
    pub turn: Option<i64>,
    /// Print only session metadata (JSON stays free of turn texts)
    #[arg(long)]
    pub meta: bool,
    #[arg(long)]
    pub db: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: ShowArgs) -> Result<()> {
    let db = Db::open(&args.db.clone().unwrap_or_else(default_db_path))?;
    let Some(row) = db.find_session(&args.session)? else {
        bail!("{}", trf("show.no_session", &[&args.session]));
    };
    let turns = db.turns_for(&row.id)?;
    let selected: Vec<_> = turns
        .iter()
        .filter(|t| args.turn.is_none_or(|i| i == t.idx))
        .collect();
    if args.turn.is_some() && selected.is_empty() {
        bail!(
            "{}",
            trf(
                "show.no_turn",
                &[&row.id, &args.turn.unwrap().to_string()]
            )
        );
    }

    if args.json {
        let turns_json = if args.meta {
            serde_json::Value::Array(Vec::new())
        } else {
            serde_json::Value::Array(
                selected
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "idx": t.idx,
                            "request_id": t.request_id,
                            "user": t.user_text,
                            "assistant": t.assistant_text,
                        })
                    })
                    .collect(),
            )
        };
        let out = serde_json::json!({
            "session": {
                "id": row.id,
                "title": row.title,
                "project": row.project_label,
                "project_key": row.project_key,
                "created_ms": row.created_ms,
                "turns": row.turns,
                "source_path": row.source_path,
                "is_empty_window": row.is_empty_window,
                "open_target": row.open_target,
            },
            "turns": turns_json,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!("{}", trf("show.session", &[&row.id]));
    println!(
        "{}",
        trf("show.title", &[row.title.as_deref().unwrap_or(tr("common.no_title"))])
    );
    println!("{}", trf("show.project", &[&row.project_label]));
    println!(
        "{}",
        trf("show.created", &[&fmt_ms_local(row.created_ms)])
    );
    println!(
        "{}",
        trf(
            "show.messages",
            &[
                &row.turns.to_string(),
                if row.is_empty_window {
                    tr("show.empty_window_suffix")
                } else {
                    ""
                },
            ]
        )
    );
    println!("{}", trf("show.source", &[&row.source_path]));
    println!("{}", "─".repeat(72));

    for t in selected {
        println!();
        println!(
            "{}",
            trf(
                "show.turn",
                &[
                    &t.idx.to_string(),
                    t.request_id.as_deref().unwrap_or("-")
                ]
            )
        );
        println!("{}", tr("show.user"));
        println!(
            "{}",
            if t.user_text.is_empty() {
                tr("show.no_text")
            } else {
                t.user_text.as_str()
            }
        );
        println!("{}", tr("show.assistant"));
        println!(
            "{}",
            if t.assistant_text.is_empty() {
                tr("show.no_text")
            } else {
                t.assistant_text.as_str()
            }
        );
    }
    Ok(())
}
