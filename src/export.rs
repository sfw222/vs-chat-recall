//! `recall export` — export a session to a markdown file with Obsidian-ready
//! YAML frontmatter (fields reserved for later vault sync).

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Args;

use crate::db::{Db, SessionRow, TurnRow, default_db_path};
use crate::i18n::{Lang, current, tr_in, trf, trf_in};
use crate::util::fmt_ms_local;

#[derive(Args, Debug)]
pub struct ExportArgs {
    /// Session id or a unique prefix
    pub session: String,
    /// Export only this turn
    #[arg(long)]
    pub turn: Option<i64>,
    /// Output file or directory (default: <temp dir>/recall-export/<title>-<id8>.md)
    #[arg(long)]
    pub out: Option<PathBuf>,
    #[arg(long)]
    pub db: Option<PathBuf>,
}

pub fn run(args: ExportArgs) -> Result<()> {
    let db = Db::open(&args.db.clone().unwrap_or_else(default_db_path))?;
    let Some(row) = db.find_session(&args.session)? else {
        bail!("{}", trf("export.no_session", &[&args.session]));
    };
    let turns = db.turns_for(&row.id)?;
    let selected: Vec<&TurnRow> = turns
        .iter()
        .filter(|t| args.turn.is_none_or(|i| i == t.idx))
        .collect();
    if selected.is_empty()
        && let Some(want) = args.turn
    {
        bail!(
            "{}",
            trf("export.no_turn", &[&row.id, &want.to_string()])
        );
    }

    let text = render_markdown(&row, &selected, current());

    let out = match args.out {
        Some(p) if p.extension().and_then(|e| e.to_str()) == Some("md") => p,
        Some(dir) => dir.join(default_file_name(&row)),
        None => PathBuf::from("data")
            .join("export")
            .join(default_file_name(&row)),
    };
    if let Some(dir) = out.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&out, text)?;
    println!("{}", trf("export.done", &[&out.display().to_string()]));
    Ok(())
}

fn default_file_name(row: &SessionRow) -> String {
    let title = row.title.clone().unwrap_or_default();
    let id8 = &row.id[..row.id.len().min(8)];
    format!("{}-{id8}.md", safe_filename(&title, 60))
}

/// Filesystem-safe stem: keeps CJK/alnum/space/dash/parens, replaces the rest.
pub fn safe_filename(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if out.chars().count() >= max_chars {
            break;
        }
        match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' | '\t' => {
                out.push('_')
            }
            _ => out.push(c),
        }
    }
    let trimmed = out.trim().trim_matches('.').trim();
    if trimmed.is_empty() || trimmed.chars().all(|c| c == '_') {
        "session".to_string()
    } else {
        trimmed.to_string()
    }
}

fn yaml_scalar(s: &str) -> String {
    // JSON string escaping is valid YAML double-quoted scalar syntax.
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

pub fn render_markdown(row: &SessionRow, turns: &[&TurnRow], lang: Lang) -> String {
    let title = row
        .title
        .clone()
        .unwrap_or_else(|| tr_in(lang, "common.no_title").to_string());
    let mut out = String::with_capacity(1 << 16);

    out.push_str("---\n");
    out.push_str(&format!("title: {}\n", yaml_scalar(&title)));
    out.push_str("aliases: []\n");
    out.push_str("tags: []\n");
    out.push_str(&format!(
        "created: {}\n",
        yaml_scalar(&fmt_ms_local(row.created_ms))
    ));
    out.push_str(&format!("project: {}\n", yaml_scalar(&row.project_label)));
    out.push_str(&format!("session_id: {}\n", yaml_scalar(&row.id)));
    if let Some(t) = row.open_target.as_deref() {
        out.push_str(&format!("workspace: {}\n", yaml_scalar(t)));
    }
    out.push_str(&format!("turns: {}\n", row.turns));
    out.push_str(&format!("source: {}\n", yaml_scalar(&row.source_path)));
    out.push_str(&format!(
        "exported: {}\n",
        yaml_scalar(&fmt_ms_local(Some(
            chrono::Utc::now().timestamp_millis()
        )))
    ));
    out.push_str("---\n\n");
    out.push_str(&format!("# {title}\n\n"));
    out.push_str(&format!(
        "> {} · {} · `{}`\n",
        row.project_label,
        trf_in(lang, "page.turns", &[&row.turns.to_string()]),
        row.id
    ));

    for t in turns {
        out.push_str(&format!(
            "\n---\n\n{}\n\n",
            trf_in(lang, "export.turn", &[&t.idx.to_string()])
        ));
        out.push_str(tr_in(lang, "export.user"));
        out.push_str("\n\n");
        out.push_str(if t.user_text.trim().is_empty() {
            tr_in(lang, "export.no_text")
        } else {
            t.user_text.trim_end()
        });
        out.push_str("\n\n");
        out.push_str(tr_in(lang, "export.assistant"));
        out.push_str("\n\n");
        out.push_str(if t.assistant_text.trim().is_empty() {
            tr_in(lang, "export.no_text")
        } else {
            t.assistant_text.trim_end()
        });
        out.push('\n');
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> SessionRow {
        SessionRow {
            id: "abcdef12-0000".into(),
            source_path: "C:/x/abcdef12.jsonl".into(),
            workspace_hash: None,
            is_empty_window: false,
            project_key: "k".into(),
            project_label: "proj".into(),
            open_target: Some("c:\\ws\\demo.code-workspace".into()),
            title: Some("a/b:c 会话".into()),
            created_ms: Some(1000),
            file_size: 1,
            file_mtime_ms: 1,
            turns: 1,
        }
    }

    #[test]
    fn safe_filename_replaces_bad_chars() {
        assert_eq!(safe_filename("a/b:c", 60), "a_b_c");
        assert_eq!(safe_filename("中文 标题", 60), "中文 标题");
        assert_eq!(safe_filename("///", 60), "session");
        assert_eq!(safe_filename("..", 60), "session");
    }

    #[test]
    fn markdown_has_frontmatter_and_turns() {
        let t = TurnRow {
            idx: 0,
            request_id: None,
            user_text: "问题".into(),
            assistant_text: "**答**".into(),
        };
        let v = vec![&t];
        let md = render_markdown(&row(), &v, Lang::Zh);
        assert!(md.starts_with("---\n"));
        assert!(md.contains("session_id: \"abcdef12-0000\""));
        assert!(md.contains("workspace: \"c:\\\\ws\\\\demo.code-workspace\""));
        assert!(md.contains("## turn 0"));
        assert!(md.contains("**答**"));
    }
}
