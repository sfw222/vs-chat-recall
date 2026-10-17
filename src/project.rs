//! Project identity resolution: map a workspace-storage hash to a normalized
//! project key, so that the same project opened as folder / multi-root
//! workspace (two different hashes) merges into one project.
//!
//! Facts (verified 2026-10-18):
//! - `workspaceStorage\<hash>\workspace.json` holds either
//!   `{"folder": "file:///f%3A/Projects/xxx"}` or
//!   `{"workspace": "file:///c%3A/.../xxx.code-workspace"}`.
//! - A `.code-workspace` file has `folders: [{ "path": "..." }]`; paths may be
//!   absolute, relative (to the workspace file), or `file://` URIs.

use std::collections::BTreeMap;
use std::path::Path;

use percent_encoding::percent_decode_str;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectInfo {
    /// Stable merge key: normalized (lowercase, backslash) sorted root paths
    /// joined by `\n`; `hash:<xxx>` fallback when workspace.json is missing.
    pub key: String,
    /// Human-readable label (folder name, or `.code-workspace` stem).
    pub label: String,
    /// Path suitable for `code <target>` (`None` for empty-window sessions or
    /// unresolvable hashes).
    pub open_target: Option<String>,
}

/// Decode a `file://` URI into a Windows path (`/f%3A/...` → `f:\...`).
/// Returns `None` for non-file URIs.
pub fn file_uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let decoded = percent_decode_str(rest).decode_utf8().ok()?.to_string();
    if let Some(stripped) = decoded.strip_prefix('/') {
        // Drive form: "f:/Projects/xxx"
        Some(stripped.replace('/', "\\"))
    } else if decoded.is_empty() {
        None
    } else {
        // UNC: "server/share" -> "\\server\share"
        Some(format!("\\\\{}", decoded.replace('/', "\\")))
    }
}

/// Normalized merge key for a filesystem root path.
pub fn normalize_root(path: &str) -> String {
    let mut p = path.trim().replace('/', "\\");
    p = p.trim_end_matches('\\').to_string();
    p.to_lowercase()
}

fn label_of(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_string()
}

#[derive(Deserialize)]
struct WorkspaceJson {
    folder: Option<String>,
    workspace: Option<String>,
}

#[derive(Deserialize)]
struct CodeWorkspace {
    folders: Option<Vec<CodeWorkspaceFolder>>,
}

#[derive(Deserialize)]
struct CodeWorkspaceFolder {
    path: Option<String>,
}

/// Resolve project info for one workspace-storage hash directory.
pub fn resolve_project(user_dir: &Path, hash: &str) -> ProjectInfo {
    let hash_dir = user_dir.join("workspaceStorage").join(hash);
    let ws_json_path = hash_dir.join("workspace.json");

    let raw = std::fs::read_to_string(&ws_json_path).ok();
    let parsed: Option<WorkspaceJson> = raw.as_deref().and_then(|t| serde_json::from_str(t).ok());

    if let Some(ws) = parsed {
        if let Some(folder_uri) = ws.folder.as_deref() {
            if let Some(path) = file_uri_to_path(folder_uri) {
                return ProjectInfo {
                    key: normalize_root(&path),
                    label: label_of(&path),
                    open_target: Some(path),
                };
            }
        }
        if let Some(workspace_uri) = ws.workspace.as_deref() {
            if let Some(ws_file) = file_uri_to_path(workspace_uri) {
                return from_code_workspace(&ws_file);
            }
        }
    }

    // Orphan hash (no workspace.json) — keep it addressable but unmerged.
    let short = &hash[..hash.len().min(8)];
    ProjectInfo {
        key: format!("hash:{hash}"),
        label: format!("(未识别工作区 {short})"),
        open_target: None,
    }
}

fn from_code_workspace(ws_file: &str) -> ProjectInfo {
    let label = Path::new(ws_file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| label_of(ws_file));

    let parsed: Option<CodeWorkspace> = std::fs::read_to_string(ws_file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());

    let mut roots: Vec<String> = Vec::new();
    if let Some(folders) = parsed.as_ref().and_then(|c| c.folders.as_ref()) {
        let base_dir = Path::new(ws_file).parent().unwrap_or(Path::new(""));
        for f in folders {
            let Some(raw) = f.path.as_deref() else { continue };
            let resolved = if raw.starts_with("file://") {
                file_uri_to_path(raw)
            } else if Path::new(raw).is_absolute() {
                Some(raw.to_string())
            } else {
                Some(base_dir.join(raw).to_string_lossy().into_owned())
            };
            if let Some(p) = resolved {
                roots.push(p);
            }
        }
    }

    if roots.is_empty() {
        // Workspace file unreadable or without folders: fall back to the file
        // path itself so the project stays distinct and openable.
        return ProjectInfo {
            key: normalize_root(ws_file),
            label,
            open_target: Some(ws_file.to_string()),
        };
    }

    let mut keys: Vec<String> = roots.iter().map(|r| normalize_root(r)).collect();
    keys.sort();
    ProjectInfo {
        key: keys.join("\n"),
        label,
        open_target: Some(ws_file.to_string()),
    }
}

/// Project info for empty-window sessions (no workspace at all).
pub fn empty_window_project() -> ProjectInfo {
    ProjectInfo {
        key: "<empty-window>".to_string(),
        label: "空窗口".to_string(),
        open_target: None,
    }
}

/// In-memory cache for per-hash resolution.
#[derive(Default)]
pub struct ProjectCache {
    map: BTreeMap<String, ProjectInfo>,
}

impl ProjectCache {
    pub fn get(&mut self, user_dir: &Path, hash: &str) -> &ProjectInfo {
        self.map
            .entry(hash.to_string())
            .or_insert_with(|| resolve_project(user_dir, hash))
    }
}

/// Convenience for tests: the resolved roots of a project key.
pub fn key_roots(key: &str) -> Vec<&str> {
    if key.starts_with("hash:") || key == "<empty-window>" {
        Vec::new()
    } else {
        key.split('\n').collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Minimal file-URI encoding for test fixtures (drive paths, spaces).
    pub(crate) fn encode_uri(p: &str) -> String {
        let s = p.replace('\\', "/").replace(':', "%3A").replace(' ', "%20");
        format!("file:///{s}")
    }

    #[test]
    fn uri_decoding() {
        assert_eq!(
            file_uri_to_path("file:///f%3A/Projects/xxx").as_deref(),
            Some("f:\\Projects\\xxx")
        );
        assert_eq!(
            file_uri_to_path("file://server/share/x").as_deref(),
            Some("\\\\server\\share\\x")
        );
        assert_eq!(file_uri_to_path("file:///c%3A/a%20b").as_deref(), Some("c:\\a b"));
        assert_eq!(file_uri_to_path("https://example.com"), None);
        assert_eq!(file_uri_to_path("file://"), None);
    }

    #[test]
    fn folder_and_workspace_hash_merge() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("Code").join("User");
        let demo = tmp.path().join("demo");
        fs::create_dir_all(&demo).unwrap();

        // Hash A: opened as a folder.
        let a = user.join("workspaceStorage").join("hashA");
        fs::create_dir_all(&a).unwrap();
        let demo_s = demo.display().to_string();
        fs::write(
            a.join("workspace.json"),
            serde_json::json!({ "folder": encode_uri(&demo_s) }).to_string(),
        )
        .unwrap();

        // Hash B: opened as a multi-root workspace pointing at the same folder.
        let ws_file = tmp.path().join("demo.code-workspace");
        fs::write(
            &ws_file,
            serde_json::json!({ "folders": [{ "path": demo_s }] }).to_string(),
        )
        .unwrap();
        let b = user.join("workspaceStorage").join("hashB");
        fs::create_dir_all(&b).unwrap();
        fs::write(
            b.join("workspace.json"),
            serde_json::json!({ "workspace": encode_uri(&ws_file.display().to_string()) }).to_string(),
        )
        .unwrap();

        let pa = resolve_project(&user, "hashA");
        let pb = resolve_project(&user, "hashB");
        assert_eq!(pa.key, pb.key, "same project must merge to one key");
        assert_eq!(pa.label, "demo");
        assert_eq!(pb.label, "demo");
        assert_eq!(pa.open_target.as_deref(), Some(demo_s.as_str()));
        assert_eq!(
            pb.open_target.as_deref(),
            Some(ws_file.display().to_string().as_str())
        );
    }

    #[test]
    fn relative_workspace_folder_and_orphan_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("Code").join("User");
        let ws_dir = tmp.path().join("wsdir");
        fs::create_dir_all(ws_dir.join("app")).unwrap();
        let ws_file = ws_dir.join("multi.code-workspace");
        fs::write(&ws_file, "{\"folders\":[{\"path\":\"app\"}]}").unwrap();
        let h = user.join("workspaceStorage").join("hashC");
        fs::create_dir_all(&h).unwrap();
        fs::write(
            h.join("workspace.json"),
            serde_json::json!({ "workspace": encode_uri(&ws_file.display().to_string()) }).to_string(),
        )
        .unwrap();

        let p = resolve_project(&user, "hashC");
        assert_eq!(p.key, normalize_root(&ws_dir.join("app").display().to_string()));
        assert_eq!(p.label, "multi");

        // Orphan hash: no workspace.json at all.
        let o = resolve_project(&user, "missinghash000");
        assert!(o.key.starts_with("hash:"));
        assert!(o.open_target.is_none());
        assert!(o.label.contains("未识别"));
    }
}
