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

#[derive(Args, Debug)]
pub struct JumpArgs {
    /// 会话 id 或其唯一前缀
    pub session: String,
    /// 只打印 URI，不做任何跳转
    #[arg(long)]
    pub print_only: bool,
    /// 不打开/聚焦窗口，只写入跳转请求
    #[arg(long)]
    pub no_launch: bool,
    /// 跳转请求文件（默认为系统临时目录的 recall-jump-request.json，扩展默认读同一路径）
    #[arg(long)]
    pub pending: Option<PathBuf>,
    #[arg(long)]
    pub db: Option<PathBuf>,
}

pub fn run(args: JumpArgs) -> Result<()> {
    let db = Db::open(&args.db.clone().unwrap_or_else(default_db_path))?;
    let Some(row) = db.find_session(&args.session)? else {
        bail!("库中没有会话「{}」（前缀也需先 collect）", args.session);
    };

    let uri = session_uri(&row.id);
    println!(
        "会话   : {} ({})",
        row.title.as_deref().unwrap_or("(无标题)"),
        row.id
    );
    println!("URI    : {uri}");
    println!(
        "作用域 : {}",
        if row.is_empty_window {
            "空窗口".to_string()
        } else {
            format!(
                "工作区 {}",
                row.open_target.as_deref().unwrap_or("(未知，无法打开)")
            )
        }
    );

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
    println!("已写入跳转请求: {}", pending_path.display());

    if !args.no_launch {
        // Open (or focus) the owning window. The extension there will consume
        // the pending request and open the session.
        let launched = if row.is_empty_window || row.open_target.is_none() {
            println!("该会话属于空窗口 → 打开一个新的空窗口…");
            std::process::Command::new("cmd")
                .args(["/C", "code", "--new-window"])
                .status()
        } else {
            let target = row.open_target.as_deref().unwrap();
            println!("正在打开/聚焦所属窗口: {target}");
            std::process::Command::new("cmd")
                .args(["/C", "code", target])
                .status()
        };
        match launched {
            Ok(s) if s.success() => {
                println!("窗口已打开/聚焦。目标窗口中的 recall-jump 扩展会自动跳转到该会话。")
            }
            Ok(s) => println!("code 退出码 {s}。"),
            Err(e) => println!("无法执行 code（{e}）。"),
        }
    }

    println!("前提与兜底：");
    println!("  · 需要已在目标窗口中安装并启用扩展 recall-jump（scripts/install-recall-jump.ps1）");
    println!("  · 若未自动跳转，可在该窗口命令面板运行「Recall: 打开历史会话（jump）」并输入会话 id");
    println!("  · 离线兜底：recall view {} --open", row.id);
    println!("  · 工作区兜底：recall open {}", row.id);
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
