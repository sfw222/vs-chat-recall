//! `recall search` — Chinese fuzzy recall over collected sessions.
//!
//! Strategy: every whitespace-separated token must appear in the turn's
//! user/assistant text. Tokens of >= 3 chars use the FTS5 **trigram** phrase
//! index; any token shorter than 3 chars cannot (trigram minimum), so the
//! whole query falls back to an escaped `LIKE` scan — the corpus is ~5MB, so
//! this is cheap.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Args;

use crate::db::{Db, default_db_path};
use crate::privacy::PrivacyPolicy;
use crate::util::{fmt_ms_local, one_line, snippet_for};

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// 查询词（中文记忆片段；多词空格分隔需全部命中）
    pub query: String,
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
    #[arg(long)]
    pub db: Option<PathBuf>,
    /// 只搜索项目标签包含该子串的会话
    #[arg(long)]
    pub project: Option<String>,
    /// 片段上下文长度（字符）
    #[arg(long, default_value_t = 90)]
    pub context: usize,
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: SearchArgs) -> Result<()> {
    let tokens: Vec<String> = args
        .query
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if tokens.is_empty() {
        bail!("查询词为空");
    }
    let db = Db::open(&args.db.clone().unwrap_or_else(default_db_path))?;
    let (sessions_total, turns_total) = db.counts()?;
    if sessions_total == 0 {
        println!("库为空——请先运行 recall collect");
        return Ok(());
    }

    let use_fallback = tokens.iter().any(|t| t.chars().count() < 3);
    // CLI search is the local user's own shell: no MCP privacy filtering here.
    let privacy = PrivacyPolicy::empty();
    let hits = if use_fallback {
        db.search_like(&tokens, args.limit, args.project.as_deref(), &privacy)?
    } else {
        db.search_fts(&tokens, args.limit, args.project.as_deref(), &privacy)?
    };

    let mut meta_cache: HashMap<String, (String, String, Option<i64>)> = HashMap::new();
    let mut results = Vec::with_capacity(hits.len());
    for hit in &hits {
        let meta = meta_cache
            .entry(hit.session_id.clone())
            .or_insert_with(|| meta_of(&db, &hit.session_id));
        let (title, project, created) = meta.clone();

        let user_snip = snippet_for(&hit.user_text, &tokens, args.context);
        let asst_snip = if user_snip.is_none() {
            snippet_for(&hit.assistant_text, &tokens, args.context)
        } else {
            None
        };
        let (field, snippet) = match (user_snip, asst_snip) {
            (Some(s), _) => ("user", s),
            (None, Some(s)) => ("assistant", s),
            (None, None) => ("-", String::new()),
        };

        results.push(serde_json::json!({
            "session_id": hit.session_id,
            "turn": hit.idx,
            "title": title,
            "project": project,
            "created_ms": created,
            "field": field,
            "snippet": snippet,
        }));
    }

    if args.json {
        let out = serde_json::json!({
            "query": args.query,
            "mode": if use_fallback { "like" } else { "fts5-trigram" },
            "hits": results,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!(
        "查询「{}」 mode={} 命中 {} 条（库内 {} 会话 / {} turns）",
        args.query,
        if use_fallback { "like" } else { "fts5-trigram" },
        hits.len(),
        sessions_total,
        turns_total
    );
    for (i, r) in results.iter().enumerate() {
        let sid = r["session_id"].as_str().unwrap_or("");
        let turn = r["turn"].as_i64().unwrap_or(0);
        let title = r["title"].as_str().unwrap_or("");
        let project = r["project"].as_str().unwrap_or("");
        let created = r["created_ms"].as_i64();
        let field = r["field"].as_str().unwrap_or("-");
        let snippet = r["snippet"].as_str().unwrap_or("");
        println!(
            "{:>2}. [{}] {} · {} · turn {}",
            i + 1,
            project,
            short(title, 60),
            fmt_ms_local(created),
            turn
        );
        println!("    {}: {}", field, one_line(snippet));
        println!("    {}   (recall view {} --turn {} --open)", sid, short_id(sid), turn);
    }
    Ok(())
}

fn meta_of(db: &Db, session_id: &str) -> (String, String, Option<i64>) {
    match db.find_session(session_id) {
        Ok(Some(s)) => {
            let title = s.title.unwrap_or_else(|| {
                db.turns_for(session_id)
                    .ok()
                    .and_then(|t| t.into_iter().find_map(|t| {
                        let u = t.user_text.trim();
                        if u.is_empty() { None } else { Some(short(u, 40)) }
                    }))
                    .unwrap_or_else(|| "(无标题)".to_string())
            });
            (title, s.project_label, s.created_ms)
        }
        _ => ("(未知)".to_string(), "(未知)".to_string(), None),
    }
}

fn short_id(id: &str) -> &str {
    &id[..id.len().min(8)]
}

fn short(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        out.push('…');
    }
    out.replace('\n', " ")
}
