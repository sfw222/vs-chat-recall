//! `recall show` — print the full text of one session from the index.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Args;

use crate::db::{Db, default_db_path};
use crate::util::fmt_ms_local;

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// 会话 id 或其唯一前缀
    pub session: String,
    /// 只看某一 turn
    #[arg(long)]
    pub turn: Option<i64>,
    /// 只输出会话元信息（JSON 时供扩展/脚本使用，不含 turns 正文）
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
        bail!("库中没有会话「{}」（前缀也需先 collect）", args.session);
    };
    let turns = db.turns_for(&row.id)?;
    let selected: Vec<_> = turns
        .iter()
        .filter(|t| args.turn.is_none_or(|i| i == t.idx))
        .collect();
    if args.turn.is_some() && selected.is_empty() {
        bail!("会话 {} 没有 turn {}", row.id, args.turn.unwrap());
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

    println!("会话   : {}", row.id);
    println!(
        "标题   : {}",
        row.title.as_deref().unwrap_or("(无标题)")
    );
    println!("项目   : {}", row.project_label);
    println!(
        "创建   : {}（本地时间）",
        fmt_ms_local(row.created_ms)
    );
    println!("消息数 : {}{}", row.turns, if row.is_empty_window { "（空窗口会话）" } else { "" });
    println!("源文件 : {}", row.source_path);
    println!("{}", "─".repeat(72));

    for t in selected {
        println!();
        println!(
            "── turn {} [{}] ──",
            t.idx,
            t.request_id.as_deref().unwrap_or("-")
        );
        println!("【用户】");
        println!("{}", if t.user_text.is_empty() { "(无文本)" } else { t.user_text.as_str() });
        println!("【助手】");
        println!(
            "{}",
            if t.assistant_text.is_empty() {
                "(无文本)"
            } else {
                t.assistant_text.as_str()
            }
        );
    }
    Ok(())
}
