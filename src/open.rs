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
use crate::i18n::trf;

#[derive(Args, Debug)]
pub struct OpenArgs {
    /// Session id or a unique prefix
    pub session: String,
    #[arg(long)]
    pub db: Option<PathBuf>,
}

pub fn run(args: OpenArgs) -> Result<()> {
    let db = Db::open(&args.db.clone().unwrap_or_else(default_db_path))?;
    let Some(row) = db.find_session(&args.session)? else {
        bail!("{}", trf("open.no_session", &[&args.session]));
    };

    let Some(target) = row.open_target.as_deref() else {
        if row.is_empty_window {
            bail!(
                "{}",
                trf(
                    "open.empty_window",
                    &[row.title.as_deref().unwrap_or("(untitled)")]
                )
            );
        }
        bail!("{}", crate::i18n::tr("open.no_target"));
    };

    println!("{}", trf("open.opening", &[target]));
    #[cfg(windows)]
    let status = Command::new("cmd").args(["/C", "code", target]).status();
    #[cfg(not(windows))]
    let status = Command::new("code").arg(target).status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => bail!("{}", trf("open.exit", &[&s.to_string()])),
        Err(e) => bail!("{}", trf("open.launch_failed", &[&e.to_string()])),
    }
}
