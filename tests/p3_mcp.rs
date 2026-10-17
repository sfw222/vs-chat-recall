//! P3 integration: drive the `recall mcp` server over stdio exactly like a
//! real MCP client (spawn process, newline-delimited JSON-RPC), including the
//! privacy acceptance: an excluded project must be invisible **everywhere** —
//! no label, no session id, no content substring in any response.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Minimal MCP stdio client
// ---------------------------------------------------------------------------

struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl McpClient {
    fn spawn(db: &Path, config: &Path) -> Self {
        let exe = env!("CARGO_BIN_EXE_recall");
        let mut child = Command::new(exe)
            .arg("mcp")
            .arg("--db")
            .arg(db)
            .arg("--config")
            .arg(config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn recall mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    fn send_raw(&mut self, value: &Value) -> Value {
        let line = serde_json::to_string(value).unwrap();
        self.stdin.write_all(line.as_bytes()).unwrap();
        self.stdin.write_all(b"\n").unwrap();
        self.stdin.flush().unwrap();
        let mut buf = String::new();
        let n = self.stdout.read_line(&mut buf).expect("read response");
        assert!(n > 0, "server closed stdout unexpectedly");
        serde_json::from_str(buf.trim()).expect("parse response json")
    }

    /// Send a request and return the response (id auto-incremented).
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send_raw(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
    }

    /// Send a notification (no id, no response expected).
    fn notify(&mut self, method: &str) {
        let line = serde_json::to_string(&json!({ "jsonrpc": "2.0", "method": method })).unwrap();
        self.stdin.write_all(line.as_bytes()).unwrap();
        self.stdin.write_all(b"\n").unwrap();
        self.stdin.flush().unwrap();
    }

    fn call_tool(&mut self, name: &str, args: Value) -> Value {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": args }),
        )
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Fixture: temp user dir with a visible and an excluded project + collect
// ---------------------------------------------------------------------------

fn encode_uri(p: &str) -> String {
    let s = p.replace('\\', "/").replace(':', "%3A").replace(' ', "%20");
    format!("file:///{s}")
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

struct Fixture {
    _tmp: tempfile::TempDir,
    user: PathBuf,
    db: PathBuf,
    config: PathBuf,
}

fn setup() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("Code").join("User");
    let vis_dir = tmp.path().join("visproj");
    let sec_dir = tmp.path().join("secproj");
    fs::create_dir_all(&vis_dir).unwrap();
    fs::create_dir_all(&sec_dir).unwrap();

    // Visible project
    let hv = user.join("workspaceStorage").join("hashVis");
    write_text(
        &hv.join("workspace.json"),
        &json!({ "folder": encode_uri(&vis_dir.display().to_string()) }).to_string(),
    );
    write_text(
        &hv.join("chatSessions").join("vis.jsonl"),
        &jsonl(&[
            r#"{"kind":0,"v":{"version":3,"creationDate":1000,"sessionId":"mcp-vis","customTitle":"可见会话","requests":[]}}"#,
            r#"{"kind":2,"k":["requests"],"v":[{"requestId":"r1","message":{"text":"我们讨论一下可见路由 QWERTY 的设计"},"response":[{"value":"可见路由 QWERTY 的方案是分层处理"}]}]}"#,
        ]),
    );

    // Excluded project
    let hs = user.join("workspaceStorage").join("hashSec");
    write_text(
        &hs.join("workspace.json"),
        &json!({ "folder": encode_uri(&sec_dir.display().to_string()) }).to_string(),
    );
    write_text(
        &hs.join("chatSessions").join("sec.jsonl"),
        &jsonl(&[
            r#"{"kind":0,"v":{"version":3,"creationDate":2000,"sessionId":"mcp-sec","customTitle":"机密会话","requests":[]}}"#,
            r#"{"kind":2,"k":["requests"],"v":[{"requestId":"r1","message":{"text":"绝密词条 ZXQPLAN 的处理"},"response":[{"value":"绝密词条 ZXQPLAN 的秘密回复"}]}]}"#,
        ]),
    );

    let db = tmp.path().join("db").join("recall.db");
    let config = tmp.path().join("recall.config.json");
    write_text(&config, &json!({ "mcp": { "exclude_projects": ["secproj"] } }).to_string());

    // collect via the library (same code path as the CLI)
    let args = vs_chat_recall::collect::CollectArgs {
        db: Some(db.clone()),
        user_dir: Some(user.clone()),
        force: false,
        prune: false,
        vacuum: false,
        json: false,
        verbose: false,
    };
    let report = vs_chat_recall::collect::collect(&args, &user, &db).unwrap();
    assert_eq!(report.new_sessions, 2);

    Fixture {
        _tmp: tmp,
        user,
        db,
        config,
    }
}

// ---------------------------------------------------------------------------
// End-to-end test
// ---------------------------------------------------------------------------

#[test]
fn mcp_stdio_end_to_end_with_privacy() {
    let f = setup();
    let _keep_user = &f.user; // keep the temp tree alive through the test
    let mut c = McpClient::spawn(&f.db, &f.config);

    // ---- initialize handshake ----
    let init = c.request(
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "vs-code-test", "version": "1.0" }
        }),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "recall");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    assert!(!init["result"]["instructions"].as_str().unwrap().is_empty());
    c.notify("notifications/initialized");

    // ---- tools/list ----
    let tools = c.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["search_sessions", "read_session"]);

    // ---- search: visible hit works ----
    let hit = c.call_tool("search_sessions", json!({ "query": "可见路由 QWERTY" }));
    let hit_s = hit.to_string();
    assert!(hit_s.contains("mcp-vis"), "visible session must be found: {hit_s}");
    assert_eq!(
        hit["result"]["structuredContent"]["hits"].as_array().unwrap().len(),
        1
    );

    // ---- privacy: query that exists ONLY in the excluded project ----
    let denied = c.call_tool("search_sessions", json!({ "query": "绝密词条 ZXQPLAN" }));
    let denied_s = denied.to_string();
    for needle in ["绝密词条", "ZXQPLAN", "mcp-sec", "机密会话", "secproj"] {
        assert!(
            !denied_s.contains(needle),
            "excluded marker `{needle}` leaked in search response: {denied_s}"
        );
    }
    assert_eq!(denied["result"]["isError"], false);
    assert!(denied["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("无命中"));

    // ---- privacy: read_session on the excluded session refuses (2025-06 id identical to not-found) ----
    let refused = c.call_tool("read_session", json!({ "session_id": "mcp-sec" }));
    let refused_s = refused.to_string();
    assert_eq!(refused["result"]["isError"], true);
    for needle in ["绝密", "ZXQPLAN", "secproj", "机密会话"] {
        assert!(
            !refused_s.contains(needle),
            "excluded marker `{needle}` leaked in read refusal: {refused_s}"
        );
    }
    let missing = c.call_tool("read_session", json!({ "session_id": "no-such-id" }));
    assert_eq!(
        refused["result"]["content"][0]["text"], missing["result"]["content"][0]["text"],
        "refusal must be indistinguishable from not-found"
    );

    // ---- read_session: bounded turn text ----
    let turn = c.call_tool(
        "read_session",
        json!({ "session_id": "mcp-vis", "turn": 0 }),
    );
    let turn_s = turn.to_string();
    assert!(turn_s.contains("可见路由 QWERTY"));
    assert_eq!(turn["result"]["structuredContent"]["truncated"], false);

    // ---- policy reloads per call: lift the exclusion mid-session ----
    fs::write(&f.config, json!({ "mcp": { "exclude_projects": [] } }).to_string()).unwrap();
    let now_visible = c.call_tool("search_sessions", json!({ "query": "绝密词条 ZXQPLAN" }));
    assert!(
        now_visible.to_string().contains("mcp-sec"),
        "after removing the exclusion the session must be visible again"
    );

    // ---- unknown tool / method errors stay JSON-RPC shaped ----
    let unknown = c.call_tool("nope", json!({}));
    assert_eq!(unknown["result"]["isError"], true);
    let bad_method = c.request("does/not/exist", json!({}));
    assert_eq!(bad_method["error"]["code"], -32601);
}
