//! `recall open` — open the VS Code window that owns a session.
//!
//! Uses the project's `open_target` (folder path or `.code-workspace` path)
//! and runs `code <target>`. Empty-window sessions have no target — VS Code
//! gives no way to programmatically focus a specific session (P5 spike), so
//! we print guidance instead.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Result, bail};
use clap::Args;

use crate::db::{Db, default_db_path};

#[derive(Args, Debug)]
pub struct OpenArgs {
    /// 会话 id 或其唯一前缀
    pub session: String,
    #[arg(long)]
    pub db: Option<PathBuf>,
}

pub fn run(args: OpenArgs) -> Result<()> {
    let db = Db::open(&args.db.clone().unwrap_or_else(default_db_path))?;
    let Some(row) = db.find_session(&args.session)? else {
        bail!("库中没有会话「{}」", args.session);
    };

    let Some(target) = row.open_target.as_deref() else {
        if row.is_empty_window {
            bail!(
                "该会话为空窗口会话（无工作区），无法用 code 打开。\n\
                 可手动在 VS Code 的 Agent Sessions/历史中按标题「{}」定位。",
                row.title.as_deref().unwrap_or("(无标题)")
            );
        }
        bail!("该会话没有可用的工作区目标（workspace.json 缺失或无法解析）");
    };

    println!("打开 VS Code：{target}");
    #[cfg(windows)]
    let status = Command::new("cmd").args(["/C", "code", target]).status();
    #[cfg(not(windows))]
    let status = Command::new("code").arg(target).status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => bail!("code 退出码 {s}"),
        Err(e) => bail!("无法执行 code：{e}（确认 VS Code 的 bin 在 PATH 中）"),
    }
}
