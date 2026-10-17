//! `recall jump` — open the original session in VS Code.
//!
//! P5 findings (verified against upstream source + a live test, 2026-10-18):
//! - the editor-resolver route works (`vscode.open` on
//!   `vscode-chat-session://local/<b64url-nopad(id)>` reaches ChatEditorInput);
//! - but a chat session store is **window-scoped** (`ChatSessionStore`: a
//!   workspace window reads only its own `workspaceStorage/<hash>/chatSessions`;
//!   only an empty window reads `globalStorage/emptyWindowChatSessions`), and
//!   `ChatEditorInput` **starts a fresh empty session** when the requested one
//!   cannot be loaded.
//!
//! Therefore jumping across workspaces needs the *owning window*: this command
//! writes a pending-jump request (`data/jump-request.json`), then opens/focuses
//! the owning window; the `recall-jump` extension in that window consumes the
//! request and runs `vscode.open`.

use std::path::PathBuf;

use anyhow::{Result, bail, Context};
use clap::Args;
use serde_json::json;

use crate::db::{Db, default_db_path};
use crate::i18n::{tr, trf};

#[derive(Args, Debug)]
pub struct JumpArgs {
    /// Session id or a unique prefix
    pub session: String,
    /// Print the URI only; do not queue or open anything
    #[arg(long)]
    pub print_only: bool,
    /// Do not open/focus the owning window (only write the jump request)
    #[arg(long)]
    pub no_launch: bool,
    /// Jump request file (default: recall-jump-request.json in the system temp dir)
    #[arg(long)]
    pub pending: Option<PathBuf>,
    #[arg(long)]
    pub db: Option<PathBuf>,
}

pub fn run(args: JumpArgs) -> Result<()> {
    let db = Db::open(&args.db.clone().unwrap_or_else(default_db_path))?;
    let Some(row) = db.find_session(&args.session)? else {
        bail!("{}", trf("jump.no_session", &[&args.session]));
    };

    let uri = session_uri(&row.id);
    println!(
        "{}",
        trf(
            "jump.session",
            &[row.title.as_deref().unwrap_or(tr("common.no_title")), &row.id]
        )
    );
    println!("{}", trf("jump.uri", &[&uri]));
    let scope = if row.is_empty_window {
        tr("jump.scope_empty").to_string()
    } else {
        match row.open_target.as_deref() {
            Some(t) => trf("jump.scope_workspace", &[t]),
            None => tr("jump.scope_unknown").to_string(),
        }
    };
    println!("{}", trf("jump.scope", &[&scope]));

    if args.print_only {
        return Ok(());
    }

    let pending_path = args
        .pending
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("recall-jump-request.json"));
    let req = json!({
        "session_id": row.id,
        "title": row.title,
        "open_target": row.open_target,
        "is_empty_window": row.is_empty_window,
        "project_key": row.project_key,
        "ts": crate::collect::now_ms(),
    });
    if let Some(dir) = pending_path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("create dir {}", dir.display()))?;
        }
    }
    std::fs::write(&pending_path, serde_json::to_string_pretty(&req)? + "\n")
        .with_context(|| format!("write {}", pending_path.display()))?;
    println!(
        "{}",
        trf("jump.request_written", &[&pending_path.display().to_string()])
    );

    if !args.no_launch {
        // Open (or focus) the owning window. The extension there will consume
        // the pending request and open the session.
        let launched = if row.is_empty_window || row.open_target.is_none() {
            println!("{}", tr("jump.opening_empty"));
            std::process::Command::new("cmd")
                .args(["/C", "code", "--new-window"])
                .status()
        } else {
            let target = row.open_target.as_deref().unwrap();
            println!("{}", trf("jump.opening_target", &[target]));
            std::process::Command::new("cmd")
                .args(["/C", "code", target])
                .status()
        };
        match launched {
            Ok(s) if s.success() => {
                println!("{}", tr("jump.window_done"));
            }
            Ok(s) => println!("{}", trf("jump.code_exit", &[&s.to_string()])),
            Err(e) => println!("{}", trf("jump.code_missing", &[&e.to_string()])),
        }
    }

    println!("{}", tr("jump.prereq"));
    println!("{}", tr("jump.prereq_ext"));
    println!("{}", tr("jump.fallback_palette"));
    println!("{}", trf("jump.fallback_view", &[&row.id]));
    println!("{}", trf("jump.fallback_open", &[&row.id]));
    Ok(())
}

/// Build the `vscode-chat-session` editor URI for a session id.
pub fn session_uri(session_id: &str) -> String {
    format!(
        "vscode-chat-session://local/{}",
        b64url_nopad(session_id.as_bytes())
    )
}

/// base64url without padding (RFC 4648 §5, unpadded).
pub fn b64url_nopad(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_known_vector() {
        // computed independently in PowerShell: [Convert]::ToBase64String(...).TrimEnd('=').Replace(...)
        assert_eq!(
            b64url_nopad(b"6047f10f-e64b-4801-95c5-33ecfa32668f"),
            "NjA0N2YxMGYtZTY0Yi00ODAxLTk1YzUtMzNlY2ZhMzI2Njhm"
        );
        assert_eq!(session_uri("abc"), "vscode-chat-session://local/YWJj");
        // padding-free and url-safe
        let t = b64url_nopad(&[251, 255, 254, 250]);
        assert!(!t.contains('='));
        assert!(!t.contains('+') && !t.contains('/'));
    }
}
