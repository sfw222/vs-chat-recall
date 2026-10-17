//! `recall mcp` — MCP server over stdio (newline-delimited JSON-RPC 2.0).
//!
//! Hand-rolled on purpose: a tools-only server needs exactly `initialize`,
//! `notifications/initialized`, `ping`, `tools/list` and `tools/call`. Keeping
//! it dependency-free makes the protocol behavior fully unit- and
//! process-testable, and there is no other consumer than the local MCP client.
//!
//! Privacy: every tool call re-loads the privacy policy (so edits to
//! `recall.config.json` take effect immediately) and applies it twice —
//! SQL-level filtering inside the queries plus a post-fetch guard on every
//! returned session. Config load failures fail closed (no data returned).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;
use serde_json::{Value, json};

use crate::db::{Db, default_db_path};
use crate::privacy::PrivacyPolicy;
use crate::util::{fmt_ms_local, one_line, snippet_for};

/// Latest protocol revision implemented here.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
const KNOWN_PROTOCOL_VERSIONS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];

const MAX_SEARCH_LIMIT: usize = 20;
const DEFAULT_SEARCH_LIMIT: usize = 5;
const MAX_SNIPPET_CHARS: usize = 240;
const MAX_QUERY_CHARS: usize = 200;
const DEFAULT_READ_CHARS: usize = 4_000;
const MAX_READ_CHARS: usize = 20_000;
const MAX_TURNS_LISTED: usize = 60;
const TURN_HEAD_CHARS: usize = 100;

const SERVER_INSTRUCTIONS: &str = "\
recall：本机 VS Code / GitHub Copilot Chat 历史会话的离线模糊回忆检索（数据完全本地）。
用法：先用 search_sessions 用一句中文或关键词找历史会话（返回片段与 session_id、turn 序号），
再用 read_session 读取该会话（或指定 turn）的完整文本。
索引来自最近一次 `recall collect`（可在终端运行以更新）；未采集过的会话不在库中。
隐私：被 recall.config.json 排除的项目在服务端强制过滤，不会以任何形式出现在结果里。";

#[derive(Args, Debug)]
pub struct McpArgs {
    /// 数据库路径（默认 data/recall.db）
    #[arg(long)]
    pub db: Option<PathBuf>,
    /// 隐私配置文件（默认 recall.config.json / 环境变量 RECALL_CONFIG）
    #[arg(long)]
    pub config: Option<PathBuf>,
}

/// Blocking stdio loop: one JSON-RPC message per line in, one per line out.
pub fn run(args: McpArgs) -> Result<()> {
    let db_path = args.db.clone().unwrap_or_else(default_db_path);
    let config_path = args.config.clone().unwrap_or_else(PrivacyPolicy::default_path);
    let db = Db::open(&db_path)?;

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(line) {
            Ok(req) => handle_message(&db, &config_path, req),
            Err(e) => Some(json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": { "code": -32700, "message": format!("JSON 解析失败: {e}") }
            })),
        };
        if let Some(resp) = response {
            serde_json::to_writer(&mut stdout, &resp).context("write response")?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

/// Handle one incoming message; `None` means "notification, do not answer".
pub fn handle_message(db: &Db, config_path: &Path, req: Value) -> Option<Value> {
    if req.is_array() {
        // JSON-RPC batches are not used by the MCP clients we target.
        return Some(error_response(Value::Null, -32600, "不支持 JSON-RPC 批量请求"));
    }
    let Some(id) = req.get("id").cloned() else {
        // Notifications (e.g. notifications/initialized) — nothing to answer.
        return None;
    };
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => {
            let client_ver = params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ver = if KNOWN_PROTOCOL_VERSIONS.contains(&client_ver) {
                client_ver
            } else {
                PROTOCOL_VERSION
            };
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": ver,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": {
                        "name": "recall",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "instructions": SERVER_INSTRUCTIONS,
                }
            }))
        }
        "ping" => Some(json!({ "jsonrpc": "2.0", "id": id, "result": {} })),
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": tool_definitions() }
        })),
        "tools/call" => Some(tool_call(db, config_path, id, &params)),
        other => Some(error_response(
            id,
            -32601,
            &format!("未知方法: {other}"),
        )),
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "search_sessions",
            "description": "在本地 VS Code/Copilot Chat 历史会话中做模糊回忆检索（中文子串语义）。\
                返回命中片段、session_id、turn 序号、项目与时间。适合“我好像讨论过某个 xx 方案”类回忆。\
                每次最多返回 20 条（默认 5）；被隐私策略排除的项目不会出现。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "回忆关键词或短语（中文直接给子串；多词空格分隔需全部命中）"
                    },
                    "limit": {
                        "type": "integer", "minimum": 1, "maximum": 20, "default": 5,
                        "description": "最多返回条数"
                    },
                    "project": {
                        "type": "string",
                        "description": "可选：只搜项目标签包含该子串的会话"
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }
        },
        {
            "name": "read_session",
            "description": "读取某个历史会话的内容。不带 turn 时返回会话信息与逐 turn 的用户消息目录；\
                带 turn 时返回该 turn 的用户/助手完整文本（受 max_chars 限制并标注截断）。\
                被隐私策略排除的会话与不存在的会话返回同样的错误信息。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "会话 id 或其唯一前缀（来自 search_sessions）"
                    },
                    "turn": {
                        "type": "integer", "minimum": 0,
                        "description": "可选：只读该 turn（0 起）"
                    },
                    "max_chars": {
                        "type": "integer", "minimum": 200, "maximum": 20000, "default": 4000,
                        "description": "单次返回文本上限（字符）"
                    }
                },
                "required": ["session_id"],
                "additionalProperties": false
            }
        }
    ])
}

fn tool_error(id: Value, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{ "type": "text", "text": message }],
            "isError": true
        }
    })
}

fn tool_ok(id: Value, text: String, structured: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{ "type": "text", "text": text }],
            "structuredContent": structured,
            "isError": false
        }
    })
}

fn tool_call(db: &Db, config_path: &Path, id: Value, params: &Value) -> Value {
    let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    // Fail closed: a broken config must not silently expose everything.
    let policy = match PrivacyPolicy::load(config_path) {
        Ok(p) => p,
        Err(e) => {
            return tool_error(
                id,
                format!("隐私配置加载失败（{e:#}）；已拒绝访问，修复 recall.config.json 后重试。"),
            );
        }
    };

    match name {
        "search_sessions" => tool_search(db, &policy, id, &arguments),
        "read_session" => tool_read(db, &policy, id, &arguments),
        _ => tool_error(id, format!("未知工具: {name}")),
    }
}

fn tool_search(db: &Db, policy: &PrivacyPolicy, id: Value, args: &Value) -> Value {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if query.is_empty() {
        return tool_error(id, "query 不能为空".to_string());
    }
    if query.chars().count() > MAX_QUERY_CHARS {
        return tool_error(id, format!("query 过长（>{MAX_QUERY_CHARS} 字符）"));
    }
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|n| (n as usize).clamp(1, MAX_SEARCH_LIMIT))
        .unwrap_or(DEFAULT_SEARCH_LIMIT);
    let project = args.get("project").and_then(|v| v.as_str()).map(str::to_string);

    let tokens: Vec<String> = query.split_whitespace().map(str::to_string).collect();
    let use_fallback = tokens.iter().any(|t| t.chars().count() < 3);
    let hits = match if use_fallback {
        db.search_like(&tokens, limit, project.as_deref(), policy)
    } else {
        db.search_fts(&tokens, limit, project.as_deref(), policy)
    } {
        Ok(h) => h,
        Err(e) => return tool_error(id, format!("检索失败: {e:#}")),
    };

    let mut blocks: Vec<String> = Vec::new();
    let mut structured_hits: Vec<Value> = Vec::new();
    let mut meta_cache: std::collections::HashMap<String, Option<crate::db::SessionRow>> =
        std::collections::HashMap::new();

    for hit in &hits {
        let meta = meta_cache
            .entry(hit.session_id.clone())
            .or_insert_with(|| db.find_session(&hit.session_id).ok().flatten());
        let Some(row) = meta else { continue };

        // Post-fetch guard (second enforcement layer after the SQL filter).
        if !policy.is_allowed(&row.project_label, &row.project_key) {
            continue;
        }

        let title = row
            .title
            .clone()
            .unwrap_or_else(|| "(无标题)".to_string());
        let user_snip = snippet_for(&hit.user_text, &tokens, 60);
        let asst_snip = if user_snip.is_none() {
            snippet_for(&hit.assistant_text, &tokens, 60)
        } else {
            None
        };
        let (field, snippet) = match (user_snip, asst_snip) {
            (Some(s), _) => ("user", s),
            (None, Some(s)) => ("assistant", s),
            (None, None) => ("-", String::new()),
        };
        let snippet = clamp_chars(&snippet, MAX_SNIPPET_CHARS);

        blocks.push(format!(
            "{}. [{}] {} · {} · turn {}\n   {}: {}\n   session: {}（read_session: {{\"session_id\": \"{}\", \"turn\": {}}}）",
            blocks.len() + 1,
            row.project_label,
            title,
            fmt_ms_local(row.created_ms),
            hit.idx,
            field,
            snippet,
            row.id,
            row.id,
            hit.idx,
        ));
        structured_hits.push(json!({
            "session_id": row.id,
            "turn": hit.idx,
            "title": title,
            "project": row.project_label,
            "created_ms": row.created_ms,
            "field": field,
            "snippet": snippet,
        }));
    }

    if structured_hits.is_empty() {
        return tool_ok(
            id,
            "无命中。提示：换更短或更长的关键词（≥3 字走索引，<3 字走全扫）；\
             若库较旧，先运行 `recall collect` 更新索引。"
                .to_string(),
            json!({ "mode": mode_name(use_fallback), "hits": [] }),
        );
    }

    // NOTE: responses never echo the query back — the caller already has it,
    // and the privacy acceptance requires zero excluded characters anywhere
    // in the returned context (echo is harmless for privacy but the criterion
    // is literal).
    tool_ok(
        id,
        format!(
            "命中 {} 条：\n{}",
            structured_hits.len(),
            blocks.join("\n")
        ),
        json!({
            "mode": mode_name(use_fallback),
            "hits": structured_hits,
        }),
    )
}

fn mode_name(fallback: bool) -> &'static str {
    if fallback { "like" } else { "fts5-trigram" }
}

fn tool_read(db: &Db, policy: &PrivacyPolicy, id: Value, args: &Value) -> Value {
    let Some(session_arg) = args.get("session_id").and_then(|v| v.as_str()) else {
        return tool_error(id, "session_id 不能为空".to_string());
    };
    let max_chars = args
        .get("max_chars")
        .and_then(|v| v.as_u64())
        .map(|n| (n as usize).clamp(200, MAX_READ_CHARS))
        .unwrap_or(DEFAULT_READ_CHARS);
    let turn = args.get("turn").and_then(|v| v.as_i64());

    // Identical message for "not found" and "excluded" — and no echo of the
    // caller-supplied id, so the two cases are byte-for-byte indistinguishable.
    let not_found = || {
        "没有找到该会话。（可能是 id 不正确，或索引未包含；可先 search_sessions，或运行 recall collect 更新索引）"
            .to_string()
    };

    let row = match db.find_session(session_arg) {
        Ok(Some(r)) => r,
        Ok(None) => return tool_error(id, not_found()),
        Err(e) => return tool_error(id, format!("查询失败: {e:#}")),
    };
    if !policy.is_allowed(&row.project_label, &row.project_key) {
        return tool_error(id, not_found());
    }

    let turns = match db.turns_for(&row.id) {
        Ok(t) => t,
        Err(e) => return tool_error(id, format!("读取失败: {e:#}")),
    };

    match turn {
        None => {
            // Bounded turn directory.
            let mut lines: Vec<String> = Vec::new();
            for t in turns.iter().take(MAX_TURNS_LISTED) {
                lines.push(format!(
                    "  turn {}: {}",
                    t.idx,
                    if t.user_text.trim().is_empty() {
                        "(无用户文本)".to_string()
                    } else {
                        clamp_chars(&one_line(t.user_text.trim()), TURN_HEAD_CHARS)
                    }
                ));
            }
            let more = turns.len().saturating_sub(MAX_TURNS_LISTED);
            let text = format!(
                "会话 {}\n标题: {}\n项目: {}\n创建: {}（本地时间）\n消息数: {}{}\nturns:\n{}{}",
                row.id,
                row.title.as_deref().unwrap_or("(无标题)"),
                row.project_label,
                fmt_ms_local(row.created_ms),
                turns.len(),
                if row.is_empty_window { "（空窗口会话）" } else { "" },
                lines.join("\n"),
                if more > 0 {
                    format!("\n  …还有 {more} 个 turn 未列出（用 turn 参数读取）")
                } else {
                    String::new()
                }
            );
            tool_ok(
                id,
                text,
                json!({
                    "session_id": row.id,
                    "title": row.title,
                    "project": row.project_label,
                    "created_ms": row.created_ms,
                    "turns": turns.len(),
                    "listed": turns.len().min(MAX_TURNS_LISTED),
                }),
            )
        }
        Some(t_idx) => {
            let Some(t) = turns.iter().find(|t| t.idx == t_idx) else {
                return tool_error(
                    id,
                    format!("会话 {} 没有 turn {}（共 {} 个 turn）", row.id, t_idx, turns.len()),
                );
            };
            let header = format!(
                "会话 {} ｜ {} ｜ turn {}\n【用户】\n",
                row.id,
                row.title.as_deref().unwrap_or("(无标题)"),
                t_idx
            );
            let body = format!(
                "{header}{}\n【助手】\n{}",
                if t.user_text.is_empty() { "(无文本)" } else { &t.user_text },
                if t.assistant_text.is_empty() { "(无文本)" } else { &t.assistant_text },
            );
            let total_chars = body.chars().count();
            let truncated = total_chars > max_chars;
            let text = if truncated {
                format!(
                    "{}\n\n…[已截断：原始 {total_chars} 字符，本次返回 {max_chars} 字符]",
                    clamp_chars(&body, max_chars)
                )
            } else {
                body
            };
            tool_ok(
                id,
                text,
                json!({
                    "session_id": row.id,
                    "turn": t_idx,
                    "title": row.title,
                    "project": row.project_label,
                    "user": t.user_text,
                    "assistant": t.assistant_text,
                    "truncated": truncated,
                }),
            )
        }
    }
}

fn clamp_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{SessionRow, TurnRow};

    fn seed(db: &mut Db) {
        let mk = |id: &str, label: &str, key: &str, _text: &str| SessionRow {
            id: id.to_string(),
            source_path: format!("C:/x/{id}.jsonl"),
            workspace_hash: Some("h".into()),
            is_empty_window: false,
            project_key: key.to_string(),
            project_label: label.to_string(),
            open_target: None,
            title: Some(format!("标题 {id}")),
            created_ms: Some(1000),
            file_size: 1,
            file_mtime_ms: 1,
            turns: 1,
        };
        let turn = |user: &str, asst: &str| TurnRow {
            idx: 0,
            request_id: None,
            user_text: user.to_string(),
            assistant_text: asst.to_string(),
        };
        db.upsert_session(
            &mk("s-visible", "myweb", "f:\\projects\\myweb", "x"),
            &[turn(
                "讨论登录流程与鉴权的方案",
                &format!("登录流程分三步：{}", "第一步".repeat(80)),
            )],
            0,
        )
        .unwrap();
        db.upsert_session(
            &mk("s-secret", "secretproj", "f:\\secret", "x"),
            &[turn("绝密方案 ZXQ 的讨论", "绝密回复 ZXQ")],
            0,
        )
        .unwrap();
    }

    fn config_with(exclude: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let cfg = json!({ "mcp": { "exclude_projects": exclude } });
        std::fs::write(dir.path().join("recall.config.json"), cfg.to_string()).unwrap();
        dir
    }

    fn call(db: &Db, cfg: &Path, name: &str, args: Value) -> Value {
        let req = json!({
            "jsonrpc": "2.0", "id": 7, "method": "tools/call",
            "params": { "name": name, "arguments": args }
        });
        handle_message(db, cfg, req).unwrap()
    }

    #[test]
    fn initialize_negotiates_and_reports_tools_capability() {
        let db = Db::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("recall.config.json");

        let req = json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name":"test","version":"0"} }
        });
        let resp = handle_message(&db, &cfg, req).unwrap();
        assert_eq!(resp["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(resp["result"]["serverInfo"]["name"], "recall");
        assert!(resp["result"]["capabilities"]["tools"].is_object());

        // unknown (newer) version → respond with our latest
        let req2 = json!({
            "jsonrpc": "2.0", "id": 2, "method": "initialize",
            "params": { "protocolVersion": "2099-01-01" }
        });
        let resp2 = handle_message(&db, &cfg, req2).unwrap();
        assert_eq!(resp2["result"]["protocolVersion"], PROTOCOL_VERSION);

        // notifications are not answered
        let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle_message(&db, &cfg, note).is_none());
    }

    #[test]
    fn tools_list_shape() {
        let db = Db::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let resp = handle_message(
            &db,
            &dir.path().join("c.json"),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
        )
        .unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["search_sessions", "read_session"]);
        for t in tools {
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn search_excludes_denied_project_everywhere() {
        let mut db = Db::open_memory().unwrap();
        seed(&mut db);
        let cfg = config_with(&["secret"]);

        // Query matches BOTH projects; only the visible one may appear.
        let resp = call(
            &db,
            &cfg.path().join("recall.config.json"),
            "search_sessions",
            json!({ "query": "方案" }),
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let full = resp.to_string();
        assert!(text.contains("myweb"));
        assert!(!full.contains("secretproj"), "excluded label leaked: {full}");
        assert!(!full.contains("绝密"), "excluded content leaked: {full}");
        assert!(!full.contains("s-secret"), "excluded id leaked: {full}");
        assert_eq!(resp["result"]["structuredContent"]["hits"].as_array().unwrap().len(), 1);

        // read_session on the excluded session refuses with the generic message.
        let resp2 = call(
            &db,
            &cfg.path().join("recall.config.json"),
            "read_session",
            json!({ "session_id": "s-secret" }),
        );
        assert_eq!(resp2["result"]["isError"], true);
        let text2 = resp2["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text2.contains("没有找到"), "{text2}");
        assert!(!resp2.to_string().contains("绝密"));
    }

    #[test]
    fn read_session_turn_text_and_truncation() {
        let mut db = Db::open_memory().unwrap();
        seed(&mut db);
        let cfg = tempfile::tempdir().unwrap();

        let resp = call(
            &db,
            &cfg.path().join("recall.config.json"),
            "read_session",
            json!({ "session_id": "s-vis", "turn": 0 }),
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("讨论登录流程与鉴权"));
        assert!(text.contains("登录流程分三步"));
        assert_eq!(resp["result"]["structuredContent"]["truncated"], false);

        // clamp with tiny max_chars
        let resp2 = call(
            &db,
            &cfg.path().join("recall.config.json"),
            "read_session",
            json!({ "session_id": "s-vis", "turn": 0, "max_chars": 200 }),
        );
        assert_eq!(resp2["result"]["structuredContent"]["truncated"], true);
        let text2 = resp2["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text2.contains("已截断"));

        // directory mode (no turn)
        let resp3 = call(
            &db,
            &cfg.path().join("recall.config.json"),
            "read_session",
            json!({ "session_id": "s-vis" }),
        );
        let text3 = resp3["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text3.contains("turn 0"), "{text3}");
    }

    #[test]
    fn broken_config_fails_closed() {
        let mut db = Db::open_memory().unwrap();
        seed(&mut db);
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("recall.config.json");
        std::fs::write(&bad, "{ not json").unwrap();

        let resp = call(&db, &bad, "search_sessions", json!({ "query": "方案" }));
        assert_eq!(resp["result"]["isError"], true);
        let full = resp.to_string();
        assert!(!full.contains("myweb"), "no data may leak when config is broken");
        assert!(resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("隐私配置"));
    }
}
