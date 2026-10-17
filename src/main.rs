//! recall — cross-workspace VS Code / Copilot Chat session recall.
//!
//! P1 scope: streaming replay of session logs + text extraction + bench.

use clap::{Parser, Subcommand};
use vs_chat_recall::{
    alloc_counter, bench, collect, dump, export, fixtures, jump, mcp, open, projects, search, show,
    view,
};

#[global_allocator]
static ALLOC: alloc_counter::CountingAlloc = alloc_counter::CountingAlloc;

#[derive(Parser)]
#[command(
    name = "recall",
    version,
    about = "Cross-workspace VS Code / Copilot Chat session recall (local only)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 扫描本机会话并增量入库（SQLite + FTS5 trigram）
    Collect(collect::CollectArgs),
    /// 中文模糊回忆检索（定位到具体 turn）
    Search(search::SearchArgs),
    /// 终端打印会话完整内容
    Show(show::ShowArgs),
    /// 生成可读 HTML 页面（turn 锚点，可 --open）
    View(view::ViewArgs),
    /// 导出 markdown（Obsidian frontmatter）
    Export(export::ExportArgs),
    /// 列出已归并的项目（folder/workspace 双 hash 合并结果）
    Projects(projects::ProjectsArgs),
    /// 打开会话所属的 VS Code 工作区窗口
    Open(open::OpenArgs),
    /// P5 跳回：写入跳转请求并打开/聚焦会话所属窗口（目标窗口自动跳转；含兜底提示）
    Jump(jump::JumpArgs),
    /// 以 MCP 服务运行（stdio，newline-delimited JSON-RPC）：search_sessions / read_session
    Mcp(mcp::McpArgs),
    /// Replay session logs and report load time, memory, and parse statistics.
    Bench(bench::BenchArgs),
    /// Print replayed turns of one session (comparison aid).
    Dump(dump::DumpArgs),
    /// Real-fixture sha256 manifest (drift sentinel) utilities.
    Fixtures(fixtures::FixturesArgs),
}

fn main() -> anyhow::Result<()> {
    alloc_counter::reset();
    match Cli::parse().cmd {
        Cmd::Collect(args) => collect::run(args),
        Cmd::Projects(args) => projects::run(args),
        Cmd::Search(args) => search::run(args),
        Cmd::Show(args) => show::run(args),
        Cmd::View(args) => view::run(args),
        Cmd::Export(args) => export::run(args),
        Cmd::Open(args) => open::run(args),
        Cmd::Jump(args) => jump::run(args),
        Cmd::Mcp(args) => mcp::run(args),
        Cmd::Bench(args) => bench::run(args),
        Cmd::Dump(args) => dump::run(args),
        Cmd::Fixtures(args) => fixtures::run(args),
    }
}
