//! P2 integration: collect (incremental + project merge) → SQLite → search.

use std::fs;
use std::path::{Path, PathBuf};

use vs_chat_recall::collect::{CollectArgs, collect};
use vs_chat_recall::db::Db;
use vs_chat_recall::privacy::PrivacyPolicy;

fn encode_uri(p: &str) -> String {
    let s = p.replace('\\', "/").replace(':', "%3A").replace(' ', "%20");
    format!("file:///{s}")
}

fn make_args(db: &Path, user_dir: &Path, prune: bool) -> CollectArgs {
    CollectArgs {
        db: Some(db.to_path_buf()),
        user_dir: Some(user_dir.to_path_buf()),
        force: false,
        prune,
        vacuum: false,
        json: false,
        verbose: false,
    }
}

fn write_text(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn jsonl(lines: &[&str]) -> String {
    let mut s = String::new();
    for l in lines {
        s.push_str(l);
        s.push('\n');
    }
    s
}

const KIND0_A: &str = r#"{"kind":0,"v":{"version":3,"creationDate":1000,"sessionId":"syn-a","customTitle":"A 会话","requests":[]}}"#;

fn session_a_v1() -> String {
    jsonl(&[
        KIND0_A,
        r#"{"kind":2,"k":["requests"],"v":[{"requestId":"r1","message":{"text":"我们讨论个登录流程方案"},"response":[{"value":"登录流程分三步走"}]}]}"#,
    ])
}

fn session_a_v2() -> String {
    jsonl(&[
        KIND0_A,
        r#"{"kind":2,"k":["requests"],"v":[{"requestId":"r1","message":{"text":"改过的新内容标题"},"response":[{"value":"新回复内容正文"}]}]}"#,
    ])
}

struct Fixture {
    tmp: tempfile::TempDir,
    user: PathBuf,
    db_path: PathBuf,
    s1: PathBuf,
    e1: PathBuf,
}

fn setup() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("Code").join("User");
    let demo = tmp.path().join("demo");
    fs::create_dir_all(&demo).unwrap();

    // hash A — folder form
    let ha = user.join("workspaceStorage").join("hashA");
    write_text(
        &ha.join("workspace.json"),
        &serde_json::json!({ "folder": encode_uri(&demo.display().to_string()) }).to_string(),
    );
    let s1 = ha.join("chatSessions").join("s1.jsonl");
    write_text(&s1, &session_a_v1());

    // hash B — workspace form → same folder (must merge with hash A)
    let ws_file = tmp.path().join("demo.code-workspace");
    write_text(
        &ws_file,
        &serde_json::json!({ "folders": [{ "path": demo.display().to_string() }] }).to_string(),
    );
    let hb = user.join("workspaceStorage").join("hashB");
    write_text(
        &hb.join("workspace.json"),
        &serde_json::json!({ "workspace": encode_uri(&ws_file.display().to_string()) }).to_string(),
    );
    write_text(
        &hb.join("chatSessions").join("s2.jsonl"),
        &jsonl(&[
            r#"{"kind":0,"v":{"version":3,"creationDate":2000,"sessionId":"syn-b","requests":[]}}"#,
            r#"{"kind":2,"k":["requests"],"v":[{"requestId":"r1","message":{"text":"另一个会话关于部署"},"response":[{"value":"好的部署方案"}]}]}"#,
        ]),
    );

    // empty-window session
    let e1 = user
        .join("globalStorage")
        .join("emptyWindowChatSessions")
        .join("e1.jsonl");
    write_text(
        &e1,
        &jsonl(&[r#"{"kind":0,"v":{"version":3,"creationDate":3000,"sessionId":"syn-e","requests":[]}}"#]),
    );

    let db_path = tmp.path().join("db").join("recall.db");
    Fixture {
        tmp,
        user,
        db_path,
        s1,
        e1,
    }
}

#[test]
fn duplicate_session_id_keeps_newest_file() {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("Code").join("User");
    let db_path = tmp.path().join("db").join("recall.db");

    let mk = |key: &str, text: &str| {
        jsonl(&[
            r#"{"kind":0,"v":{"version":3,"creationDate":1000,"sessionId":"dup","requests":[]}}"#,
            &format!(
                r#"{{"kind":2,"k":["requests"],"v":[{{"requestId":"r1","message":{{"text":"{text}"}},"response":[{{"value":"{key}-回复"}}]}}]}}"#
            ),
        ])
    };

    let old_path = user
        .join("workspaceStorage")
        .join("hashOld")
        .join("chatSessions")
        .join("dup.jsonl");
    write_text(&old_path, &mk("old", "旧副本内容原始"));
    let new_path = user
        .join("workspaceStorage")
        .join("hashNew")
        .join("chatSessions")
        .join("dup.jsonl");
    write_text(&new_path, &mk("new", "新副本内容最新"));

    // explicit mtimes: old = now-100s, new = now
    let now = std::time::SystemTime::now();
    let old_t = now - std::time::Duration::from_secs(100);
    std::fs::File::options()
        .write(true)
        .open(&old_path)
        .unwrap()
        .set_modified(old_t)
        .unwrap();
    std::fs::File::options()
        .write(true)
        .open(&new_path)
        .unwrap()
        .set_modified(now)
        .unwrap();

    let r = collect(&make_args(&db_path, &user, false), &user, &db_path).unwrap();
    assert_eq!(r.session_id_collisions, 1);
    let db = Db::open(&db_path).unwrap();
    let (sessions, turns) = db.counts().unwrap();
    assert_eq!(sessions, 1);
    assert_eq!(turns, 1);
    let row = db.find_session("dup").unwrap().unwrap();
    assert!(row.source_path.contains("hashNew"), "newest file must win");
    let hits = db.search_like(&["新副本内容最新".into()], 10, None, &PrivacyPolicy::empty()).unwrap();
    assert_eq!(hits.len(), 1, "content of the newest copy must be indexed");
    let hits = db.search_like(&["旧副本内容原始".into()], 10, None, &PrivacyPolicy::empty()).unwrap();
    assert_eq!(hits.len(), 0);

    // incremental re-run must be stable: zero writes, the newer copy still wins
    let r2 = collect(&make_args(&db_path, &user, false), &user, &db_path).unwrap();
    assert_eq!(r2.new_sessions, 0);
    assert_eq!(r2.updated_sessions, 0);
    assert_eq!(r2.session_id_collisions, 1);
    let db = Db::open(&db_path).unwrap();
    let row = db.find_session("dup").unwrap().unwrap();
    assert!(row.source_path.contains("hashNew"));
    let hits = db.search_like(&["新副本内容最新".into()], 10, None, &PrivacyPolicy::empty()).unwrap();
    assert_eq!(hits.len(), 1);
}

#[test]
fn collect_merges_hashes_incremental_and_prune() {
    let f = setup();
    let _keep = &f.tmp; // keep tempdir alive

    // ---- first collect ----
    let r1 = collect(&make_args(&f.db_path, &f.user, false), &f.user, &f.db_path).unwrap();
    assert_eq!(r1.files_scanned, 3);
    assert_eq!(r1.new_sessions, 3);
    assert_eq!(r1.skipped_unchanged, 0);
    assert_eq!(r1.errors, 0);

    let db = Db::open(&f.db_path).unwrap();
    let (sessions, turns) = db.counts().unwrap();
    assert_eq!(sessions, 3);
    assert_eq!(turns, 2); // two non-empty sessions, one turn each

    // ---- project merge across folder/workspace hashes ----
    let a = db.find_session("syn-a").unwrap().unwrap();
    let b = db.find_session("syn-b").unwrap().unwrap();
    assert_eq!(a.project_key, b.project_key, "dual hashes must merge");
    assert_eq!(a.project_label, "demo");
    assert_eq!(b.project_label, "demo");
    assert!(a.open_target.is_some(), "folder form has a code target");
    assert!(b.open_target.is_some(), "workspace form has a code target");

    // ---- empty window ----
    let e = db.find_session("syn-e").unwrap().unwrap();
    assert_eq!(e.project_label, "空窗口");
    assert!(e.is_empty_window);
    assert!(e.open_target.is_none());

    // ---- FTS trigram search over collected data ----
    let hits = db.search_fts(&["登录流程".into()], 10, None, &PrivacyPolicy::empty()).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].session_id, "syn-a");

    // ---- LIKE fallback (2-char query) ----
    let hits = db.search_like(&["部署".into()], 10, None, &PrivacyPolicy::empty()).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].session_id, "syn-b");

    // ---- project filter ----
    let hits = db.search_like(&["部署".into()], 10, Some("nomatch"), &PrivacyPolicy::empty()).unwrap();
    assert_eq!(hits.len(), 0);

    // ---- second collect: everything unchanged → skipped ----
    let r2 = collect(&make_args(&f.db_path, &f.user, false), &f.user, &f.db_path).unwrap();
    assert_eq!(r2.skipped_unchanged, 3);
    assert_eq!(r2.new_sessions + r2.updated_sessions, 0);

    // ---- change s1 → re-collected, old text leaves the index ----
    write_text(&f.s1, &session_a_v2());
    let r3 = collect(&make_args(&f.db_path, &f.user, false), &f.user, &f.db_path).unwrap();
    assert_eq!(r3.updated_sessions, 1);
    assert_eq!(r3.skipped_unchanged, 2);

    let db = Db::open(&f.db_path).unwrap();
    let hits = db.search_fts(&["改过的新内容标题".into()], 10, None, &PrivacyPolicy::empty()).unwrap();
    assert_eq!(hits.len(), 1);
    let hits = db.search_fts(&["登录流程".into()], 10, None, &PrivacyPolicy::empty()).unwrap();
    assert_eq!(hits.len(), 0, "stale text must be removed on re-collect");

    // ---- prune: file disappears ----
    fs::remove_file(&f.e1).unwrap();
    let r4 = collect(&make_args(&f.db_path, &f.user, false), &f.user, &f.db_path).unwrap();
    assert_eq!(r4.stale_in_db, 1);
    assert_eq!(r4.pruned, 0, "prune requires --prune");

    let r5 = collect(&make_args(&f.db_path, &f.user, true), &f.user, &f.db_path).unwrap();
    assert_eq!(r5.pruned, 1);
    let db = Db::open(&f.db_path).unwrap();
    let (sessions, _) = db.counts().unwrap();
    assert_eq!(sessions, 2);
    assert!(db.find_session("syn-e").unwrap().is_none());
}
