//! Local VS Code session file discovery.
//!
//! Layout (verified 2026-10-18, VS Code 1.142):
//! - `<user>\workspaceStorage\<hash>\chatSessions\*.jsonl`
//! - `<user>\globalStorage\emptyWindowChatSessions\*.jsonl` (no workspace window)
//!
//! where `<user>` = `%APPDATA%\Code\User` on Windows.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct SessionFile {
    pub path: PathBuf,
    pub size: u64,
    /// Workspace hash directory name; `None` for empty-window sessions.
    pub workspace_hash: Option<String>,
}

impl SessionFile {
    pub fn is_empty_window(&self) -> bool {
        self.workspace_hash.is_none()
    }
}

/// `%APPDATA%\Code\User`
pub fn default_user_dir() -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").context("APPDATA env var not set")?;
    Ok(PathBuf::from(appdata).join("Code").join("User"))
}

/// Enumerate all session `.jsonl` files under a VS Code user data directory.
/// Read-only; missing subtrees are tolerated.
pub fn enumerate_sessions(user_dir: &Path) -> Result<Vec<SessionFile>> {
    let mut out = Vec::new();

    let ws_root = user_dir.join("workspaceStorage");
    if ws_root.is_dir() {
        for entry in std::fs::read_dir(&ws_root)
            .with_context(|| format!("read_dir {}", ws_root.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let hash = entry.file_name().to_string_lossy().into_owned();
            collect_jsonl(&entry.path().join("chatSessions"), Some(hash), &mut out)?;
        }
    }

    let empty_window = user_dir
        .join("globalStorage")
        .join("emptyWindowChatSessions");
    collect_jsonl(&empty_window, None, &mut out)?;

    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn collect_jsonl(dir: &Path, hash: Option<String>, out: &mut Vec<SessionFile>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(SessionFile {
            path,
            size,
            workspace_hash: hash.clone(),
        });
    }
    Ok(())
}

/// Recursively collect `*.jsonl` files under an arbitrary path (for `bench <paths>`).
pub fn collect_jsonl_under(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    if !path.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(path).with_context(|| format!("read_dir {}", path.display()))? {
        let entry = entry?;
        let p = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            collect_jsonl_under(&p, out)?;
        } else if ft.is_file() && p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(p);
        }
    }
    Ok(())
}
