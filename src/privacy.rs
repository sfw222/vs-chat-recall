//! Privacy policy for the MCP entry point (`mcp.exclude_projects`).
//!
//! Config file (`recall.config.json`, path via `--config` / `RECALL_CONFIG`,
//! default `recall.config.json` in cwd):
//!
//! ```json
//! {
//!   "mcp": {
//!     "exclude_projects": ["fmodel", "工作"],
//!     "allow_projects": []
//!   }
//! }
//! ```
//!
//! Semantics:
//! - Patterns are case-insensitive substrings matched against the project
//!   **label** or the project **key** (root path set).
//! - `exclude_projects` always wins (deny).
//! - If `allow_projects` is non-empty, a project must match at least one
//!   allow pattern (allowlist mode); an empty list means "allow everything
//!   not excluded".
//! - Enforced twice: `sql_clause()` filters rows inside the query, and
//!   `is_allowed()` re-checks every returned row (defense in depth).

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct PrivacyPolicy {
    exclude: Vec<String>,
    allow: Vec<String>,
}

#[derive(Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    mcp: McpSection,
}

#[derive(Deserialize, Default)]
struct McpSection {
    #[serde(default)]
    exclude_projects: Vec<String>,
    #[serde(default)]
    allow_projects: Vec<String>,
}

impl PrivacyPolicy {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn new(exclude: Vec<String>, allow: Vec<String>) -> Self {
        Self {
            exclude: exclude.into_iter().map(|s| s.to_lowercase()).collect(),
            allow: allow.into_iter().map(|s| s.to_lowercase()).collect(),
        }
    }

    /// Load from an explicit path. A missing file is an empty (allow-all)
    /// policy — the file is optional.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::empty()),
            Err(e) => return Err(e).with_context(|| format!("read config {}", path.display())),
        };
        let cfg: ConfigFile = serde_json::from_str(&text)
            .with_context(|| format!("parse config {}", path.display()))?;
        Ok(Self::new(cfg.mcp.exclude_projects, cfg.mcp.allow_projects))
    }

    /// Resolve the default config path (env `RECALL_CONFIG` wins).
    pub fn default_path() -> std::path::PathBuf {
        std::env::var_os("RECALL_CONFIG")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("recall.config.json"))
    }

    /// The single matching rule: case-insensitive substring on label or key.
    fn matches(patterns: &[String], label: &str, key: &str) -> bool {
        let l = label.to_lowercase();
        let k = key.to_lowercase();
        patterns.iter().any(|p| l.contains(p) || k.contains(p))
    }

    /// True when a session of this project may be shown through the MCP entry.
    pub fn is_allowed(&self, label: &str, key: &str) -> bool {
        if Self::matches(&self.exclude, label, key) {
            return false;
        }
        if self.allow.is_empty() {
            true
        } else {
            Self::matches(&self.allow, label, key)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.exclude.is_empty() && self.allow.is_empty()
    }

    /// SQL fragment + parameters for query-level filtering.
    /// The fragment is appended to a WHERE clause and references the sessions
    /// table as `s`; parameters are positional `?` values in appearance order.
    pub fn sql_clause(&self) -> (String, Vec<String>) {
        let mut clause = String::new();
        let mut args: Vec<String> = Vec::new();

        for p in &self.exclude {
            clause.push_str(
                " AND (s.project_label NOT LIKE ? ESCAPE '\\' AND s.project_key NOT LIKE ? ESCAPE '\\')",
            );
            // The clause binds the pattern twice (label and key), so two values.
            let pat = format!("%{}%", escape_like(p));
            args.extend([pat.clone(), pat]);
        }

        if !self.allow.is_empty() {
            // One OR-group per allow pattern (each group binds two placeholders).
            let ors: Vec<&str> = vec![
                "(s.project_label LIKE ? ESCAPE '\\' OR s.project_key LIKE ? ESCAPE '\\')";
                self.allow.len()
            ];
            clause.push_str(&format!(" AND ({})", ors.join(" OR ")));
            for p in &self.allow {
                // Same as above: one pattern per `?` placeholder.
                let pat = format!("%{}%", escape_like(p));
                args.extend([pat.clone(), pat]);
            }
        }

        (clause, args)
    }
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_wins_and_substring_matching() {
        let p = PrivacyPolicy::new(vec!["fmodel".into()], vec![]);
        assert!(!p.is_allowed("FModel", "f:\\fmodel"));
        assert!(!p.is_allowed("FModel (f:\\fmodel)", "f:\\fmodel\nf:\\x"));
        assert!(p.is_allowed("myweb", "f:\\projects\\myweb"));

        // key-based match too
        let p = PrivacyPolicy::new(vec!["tools\\dumper".into()], vec![]);
        assert!(!p.is_allowed("FModel", "f:\\fmodel\nf:\\tools\\dumper-7"));
    }

    #[test]
    fn allowlist_mode_and_deny_precedence() {
        let p = PrivacyPolicy::new(vec!["secret".into()], vec!["myweb".into(), "secret".into()]);
        assert!(p.is_allowed("myweb", "f:\\projects\\myweb"));
        assert!(!p.is_allowed("secret", "f:\\secret")); // deny beats allow
        assert!(!p.is_allowed("other", "f:\\other")); // not in allowlist
    }

    #[test]
    fn empty_policy_allows_all() {
        let p = PrivacyPolicy::empty();
        assert!(p.is_allowed("anything", "x"));
        let (clause, args) = p.sql_clause();
        assert!(clause.is_empty());
        assert!(args.is_empty());
    }

    #[test]
    fn config_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recall.config.json");
        std::fs::write(
            &path,
            r#"{"mcp":{"exclude_projects":["FModel"],"allow_projects":["myweb"]}}"#,
        )
        .unwrap();
        let p = PrivacyPolicy::load(&path).unwrap();
        assert!(!p.is_allowed("fmodel", ""));
        assert!(p.is_allowed("myweb", ""));
        assert!(!p.is_allowed("other", ""));

        // missing file → empty policy
        let p2 = PrivacyPolicy::load(&dir.path().join("nope.json")).unwrap();
        assert!(p2.is_empty());
        assert!(p2.is_allowed("fmodel", ""));
    }

    #[test]
    fn sql_clause_escapes_wildcards() {
        let p = PrivacyPolicy::new(vec!["100%_x".into()], vec![]);
        let (clause, args) = p.sql_clause();
        assert!(clause.contains("NOT LIKE"));
        assert_eq!(args[0], "%100\\%\\_x%");
    }
}
