//! SQLite storage for collected sessions (schema v1).
//!
//! Design: `turns` is the source of truth (plain table); `turns_fts` is a
//! standalone FTS5 index with the **trigram** tokenizer (verified available in
//! the bundled SQLite). Queries shorter than 3 characters cannot use the
//! trigram index, so callers fall back to a `LIKE` scan on `turns` — the
//! extracted corpus is tiny (~5MB), so the scan is cheap and deterministic.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use crate::i18n::{tr, trf};
use rusqlite::{Connection, params};

pub const SCHEMA_VERSION: i64 = 1;

pub fn default_db_path() -> PathBuf {
    PathBuf::from("data").join("recall.db")
}

#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: String,
    pub source_path: String,
    pub workspace_hash: Option<String>,
    pub is_empty_window: bool,
    pub project_key: String,
    pub project_label: String,
    pub open_target: Option<String>,
    pub title: Option<String>,
    pub created_ms: Option<i64>,
    pub file_size: i64,
    pub file_mtime_ms: i64,
    pub turns: i64,
}

#[derive(Debug, Clone)]
pub struct TurnRow {
    pub idx: i64,
    pub request_id: Option<String>,
    pub user_text: String,
    pub assistant_text: String,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub session_id: String,
    pub idx: i64,
    pub user_text: String,
    pub assistant_text: String,
}

pub struct Db {
    pub conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("create db dir {}", dir.display()))?;
            }
        }
        let conn = Connection::open(path).with_context(|| format!("open db {}", path.display()))?;
        init(&conn)?;
        Ok(Db { conn })
    }

    pub fn open_memory() -> Result<Db> {
        let conn = Connection::open_in_memory()?;
        init(&conn)?;
        Ok(Db { conn })
    }

    /// Insert or fully replace one session and its turns (single transaction).
    pub fn upsert_session(
        &mut self,
        session: &SessionRow,
        turns: &[TurnRow],
        now_ms: i64,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO sessions (
                id, source_path, workspace_hash, is_empty_window, project_key,
                project_label, open_target, title, created_ms,
                file_size, file_mtime_ms, turns, last_collected_ms
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(id) DO UPDATE SET
                source_path=excluded.source_path,
                workspace_hash=excluded.workspace_hash,
                is_empty_window=excluded.is_empty_window,
                project_key=excluded.project_key,
                project_label=excluded.project_label,
                open_target=excluded.open_target,
                title=excluded.title,
                created_ms=excluded.created_ms,
                file_size=excluded.file_size,
                file_mtime_ms=excluded.file_mtime_ms,
                turns=excluded.turns,
                last_collected_ms=excluded.last_collected_ms",
            params![
                session.id,
                session.source_path,
                session.workspace_hash,
                session.is_empty_window as i64,
                session.project_key,
                session.project_label,
                session.open_target,
                session.title,
                session.created_ms,
                session.file_size,
                session.file_mtime_ms,
                session.turns,
                now_ms,
            ],
        )?;
        tx.execute("DELETE FROM turns WHERE session_id = ?1", params![session.id])?;
        tx.execute(
            "DELETE FROM turns_fts WHERE session_id = ?1",
            params![session.id],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO turns (session_id, idx, request_id, user_text, assistant_text)
                 VALUES (?1,?2,?3,?4,?5)",
            )?;
            for t in turns {
                stmt.execute(params![
                    session.id,
                    t.idx,
                    t.request_id,
                    t.user_text,
                    t.assistant_text
                ])?;
            }
        }
        {
            let mut stmt = tx.prepare(
                "INSERT INTO turns_fts (session_id, idx, user_text, assistant_text)
                 VALUES (?1,?2,?3,?4)",
            )?;
            for t in turns {
                stmt.execute(params![
                    session.id,
                    t.idx,
                    t.user_text,
                    t.assistant_text
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// source_path → (file_size, file_mtime_ms, session_id) for incremental scans.
    pub fn file_index(&self) -> Result<HashMap<String, (i64, i64, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT source_path, file_size, file_mtime_ms, id FROM sessions")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                ),
            ))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (path, v) = row?;
            map.insert(path, v);
        }
        Ok(map)
    }

    /// Delete sessions whose source files are gone; returns removed count.
    pub fn prune_missing(&mut self, keep_paths: &std::collections::HashSet<String>) -> Result<usize> {
        let ids: Vec<(String, String)> = {
            let mut stmt = self.conn.prepare("SELECT id, source_path FROM sessions")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut removed = 0usize;
        for (id, path) in ids {
            if !keep_paths.contains(&path) {
                self.delete_session(&id)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn delete_session(&mut self, id: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM turns WHERE session_id = ?1", params![id])?;
        tx.execute("DELETE FROM turns_fts WHERE session_id = ?1", params![id])?;
        tx.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// Resolve a full session id or unique prefix.
    pub fn find_session(&self, id_or_prefix: &str) -> Result<Option<SessionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_path, workspace_hash, is_empty_window, project_key,
                    project_label, open_target, title, created_ms,
                    file_size, file_mtime_ms, turns
             FROM sessions WHERE id = ?1 OR id LIKE ?1 || '%' LIMIT 3",
        )?;
        let rows = stmt
            .query_map(params![id_or_prefix], map_session)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        match rows.len() {
            0 => Ok(None),
            1 => Ok(Some(rows.into_iter().next().unwrap())),
            _ => bail!(
                "{}",
                trf("db.session_prefix_ambiguous", &[&rows.len().to_string()])
            ),
        }
    }

    pub fn turns_for(&self, session_id: &str) -> Result<Vec<TurnRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT idx, request_id, user_text, assistant_text
             FROM turns WHERE session_id = ?1 ORDER BY idx",
        )?;
        let rows = stmt.query_map(params![session_id], |r| {
            Ok(TurnRow {
                idx: r.get(0)?,
                request_id: r.get(1)?,
                user_text: r.get(2)?,
                assistant_text: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// FTS5 trigram phrase search over turns. `phrases` are joined with AND,
    /// each as a quoted phrase. Returns hits ordered by bm25 (best first).
    /// `privacy` adds query-level filtering (MCP entry); pass an empty policy
    /// for unfiltered CLI use.
    pub fn search_fts(
        &self,
        phrases: &[String],
        limit: usize,
        project_like: Option<&str>,
        privacy: &crate::privacy::PrivacyPolicy,
    ) -> Result<Vec<SearchHit>> {
        let query = phrases
            .iter()
            .map(|p| format!("\"{}\"", p.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        let (priv_clause, priv_args) = privacy.sql_clause();
        let sql = format!(
            "SELECT f.session_id, f.idx, f.user_text, f.assistant_text
             FROM turns_fts f
             JOIN sessions s ON s.id = f.session_id
             WHERE turns_fts MATCH ?
               AND (? IS NULL OR s.project_label LIKE '%' || ? || '%')
               {priv_clause}
             ORDER BY bm25(turns_fts)
             LIMIT ?"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut boxed: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        boxed.push(Box::new(query));
        boxed.push(Box::new(project_like.map(str::to_string)));
        boxed.push(Box::new(project_like.map(str::to_string)));
        for a in &priv_args {
            boxed.push(Box::new(a.clone()));
        }
        boxed.push(Box::new(limit as i64));
        let refs: Vec<&dyn rusqlite::ToSql> = boxed.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), |r| {
            Ok(SearchHit {
                session_id: r.get(0)?,
                idx: r.get(1)?,
                user_text: r.get(2)?,
                assistant_text: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// LIKE fallback for queries with < 3 chars per token (trigram minimum).
    /// All `tokens` must appear in `user_text` or `assistant_text`; results are
    /// ordered by recency (session creation date, newest first).
    pub fn search_like(
        &self,
        tokens: &[String],
        limit: usize,
        project_like: Option<&str>,
        privacy: &crate::privacy::PrivacyPolicy,
    ) -> Result<Vec<SearchHit>> {
        // Build "user_text LIKE ? OR assistant_text LIKE ?" per token, AND-joined.
        let mut where_parts = Vec::new();
        let mut args: Vec<String> = Vec::new();
        for t in tokens {
            let pat = format!("%{}%", escape_like(t));
            where_parts.push(
                "(t.user_text LIKE ? ESCAPE '\\' OR t.assistant_text LIKE ? ESCAPE '\\')"
                    .to_string(),
            );
            args.push(pat.clone());
            args.push(pat);
        }
        let (priv_clause, priv_args) = privacy.sql_clause();
        let sql = format!(
            "SELECT t.session_id, t.idx, t.user_text, t.assistant_text
             FROM turns t
             JOIN sessions s ON s.id = t.session_id
             WHERE {}
               AND (? IS NULL OR s.project_label LIKE '%' || ? || '%')
               {priv_clause}
             ORDER BY COALESCE(s.created_ms, 0) DESC, s.id, t.idx
             LIMIT ?",
            where_parts.join(" AND "),
        );
        let mut stmt = self.conn.prepare(&sql)?;
        // rusqlite binds a param slice positionally: refs[i] fills the i-th `?`.
        let mut boxed: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        for a in &args {
            boxed.push(Box::new(a.clone()));
        }
        let proj = project_like.map(str::to_string);
        boxed.push(Box::new(proj.clone()));
        boxed.push(Box::new(proj));
        for a in &priv_args {
            boxed.push(Box::new(a.clone()));
        }
        boxed.push(Box::new(limit as i64));
        let refs: Vec<&dyn rusqlite::ToSql> = boxed.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), |r| {
            Ok(SearchHit {
                session_id: r.get(0)?,
                idx: r.get(1)?,
                user_text: r.get(2)?,
                assistant_text: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn counts(&self) -> Result<(i64, i64)> {
        let sessions: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
        let turns: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM turns", [], |r| r.get(0))?;
        Ok((sessions, turns))
    }

    /// (project_key, label, session count, turn count) — used by `projects`.
    pub fn project_summaries(&self) -> Result<Vec<(String, String, i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.project_key, s.project_label, COUNT(DISTINCT s.id), COALESCE(SUM(s.turns), 0)
             FROM sessions s GROUP BY s.project_key, s.project_label
             ORDER BY 3 DESC, 4 DESC",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Compact the database (rebuilds the FTS index, drops free pages).
    pub fn vacuum(&self) -> Result<()> {
        self.conn.execute_batch("VACUUM;")?;
        Ok(())
    }
}

fn map_session(r: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        id: r.get(0)?,
        source_path: r.get(1)?,
        workspace_hash: r.get(2)?,
        is_empty_window: r.get::<_, i64>(3)? != 0,
        project_key: r.get(4)?,
        project_label: r.get(5)?,
        open_target: r.get(6)?,
        title: r.get(7)?,
        created_ms: r.get(8)?,
        file_size: r.get(9)?,
        file_mtime_ms: r.get(10)?,
        turns: r.get(11)?,
    })
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;",
    )?;
    let user_version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if user_version == 0 {
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                source_path TEXT NOT NULL,
                workspace_hash TEXT,
                is_empty_window INTEGER NOT NULL DEFAULT 0,
                project_key TEXT NOT NULL,
                project_label TEXT NOT NULL,
                open_target TEXT,
                title TEXT,
                created_ms INTEGER,
                file_size INTEGER NOT NULL DEFAULT 0,
                file_mtime_ms INTEGER NOT NULL DEFAULT 0,
                turns INTEGER NOT NULL DEFAULT 0,
                last_collected_ms INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_key);
             CREATE TABLE IF NOT EXISTS turns (
                session_id TEXT NOT NULL,
                idx INTEGER NOT NULL,
                request_id TEXT,
                user_text TEXT NOT NULL DEFAULT '',
                assistant_text TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (session_id, idx)
             ) WITHOUT ROWID;
             CREATE VIRTUAL TABLE IF NOT EXISTS turns_fts USING fts5(
                session_id UNINDEXED,
                idx UNINDEXED,
                user_text,
                assistant_text,
                tokenize='trigram'
             );
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
    } else if user_version > SCHEMA_VERSION {
        bail!(
            "{}",
            trf(
                "db.schema_newer",
                &[&user_version.to_string(), &SCHEMA_VERSION.to_string()]
            )
        );
    }
    verify_fts5(conn)?;
    Ok(())
}

fn verify_fts5(conn: &Connection) -> Result<()> {
    let has_fts5: i64 = conn.query_row(
        "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
        [],
        |r| r.get(0),
    )?;
    if has_fts5 == 0 {
        bail!("{}", tr("db.fts5_missing"));
    }
    conn.execute_batch("DROP TABLE IF EXISTS _fts_probe; CREATE VIRTUAL TABLE _fts_probe USING fts5(x, tokenize='trigram'); DROP TABLE _fts_probe;")
        .context(tr("db.trigram_missing"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy::PrivacyPolicy;

    fn session(id: &str, label: &str, created: i64) -> SessionRow {
        SessionRow {
            id: id.to_string(),
            source_path: format!("C:/x/{id}.jsonl"),
            workspace_hash: Some("h".into()),
            is_empty_window: false,
            project_key: label.to_lowercase(),
            project_label: label.to_string(),
            open_target: None,
            title: Some(format!("title {id}")),
            created_ms: Some(created),
            file_size: 1,
            file_mtime_ms: 1,
            turns: 1,
        }
    }

    fn turn(idx: i64, user: &str, asst: &str) -> TurnRow {
        TurnRow {
            idx,
            request_id: None,
            user_text: user.to_string(),
            assistant_text: asst.to_string(),
        }
    }

    #[test]
    fn fts5_trigram_available() {
        let conn = Connection::open_in_memory().unwrap();
        init(&conn).unwrap();
    }

    #[test]
    fn fts_search_and_like_fallback() {
        let mut db = Db::open_memory().unwrap();
        db.upsert_session(
            &session("s1", "Demo", 100),
            &[
                turn(0, "帮我设计一个登录流程", "登录流程可以分为三步"),
                turn(1, "方案的取舍是什么", "方案 A 更稳"),
            ],
            0,
        )
        .unwrap();

        // 3+ chars → FTS trigram phrase (turn 0 matches once even though both
        // columns contain the phrase — hits are per turn row).
        let hits = db.search_fts(&["登录流程".into()], 10, None, &PrivacyPolicy::empty()).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, "s1");
        assert_eq!(hits[0].idx, 0);

        // 2 chars → LIKE fallback path (caller decides; here verify it works)
        let hits = db.search_like(&["方案".into()], 10, None, &PrivacyPolicy::empty()).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].idx, 1);

        // LIKE escaping: % should be literal
        let hits = db.search_like(&["%".into()], 10, None, &PrivacyPolicy::empty()).unwrap();
        assert_eq!(hits.len(), 0);
    }

    #[test]
    fn replace_on_recollect_is_clean() {
        let mut db = Db::open_memory().unwrap();
        db.upsert_session(&session("s1", "Demo", 100), &[turn(0, "旧文本", "x")], 0)
            .unwrap();
        db.upsert_session(&session("s1", "Demo", 100), &[turn(0, "新文本内容", "y")], 0)
            .unwrap();
        let (s, t) = db.counts().unwrap();
        assert_eq!((s, t), (1, 1));
        let hits = db.search_fts(&["新文本内容".into()], 10, None, &PrivacyPolicy::empty()).unwrap();
        assert_eq!(hits.len(), 1);
        let hits = db.search_fts(&["旧文本".into()], 10, None, &PrivacyPolicy::empty()).unwrap();
        assert_eq!(hits.len(), 0);
    }
}
