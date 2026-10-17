//! recall — cross-workspace VS Code / Copilot Chat session recall.
//!
//! Library crate so integration tests and future tools (P3 MCP) can reuse the
//! replay / extraction logic; the `recall` binary is a thin CLI on top.

pub mod alloc_counter;
pub mod bench;
pub mod collect;
pub mod db;
pub mod dump;
pub mod export;
pub mod extract;
pub mod fixtures;
pub mod i18n;
pub mod jump;
pub mod mcp;
pub mod md;
pub mod open;
pub mod privacy;
pub mod project;
pub mod projects;
pub mod replay;
pub mod search;
pub mod sessions;
pub mod show;
pub mod util;
pub mod view;
