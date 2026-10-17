//! recall — cross-workspace VS Code / Copilot Chat session recall.
//!
//! Runtime output is localized (zh-CN / en); CLI help stays English.

use clap::{Parser, Subcommand};
use vs_chat_recall::{
    alloc_counter, bench, collect, dump, export, fixtures, i18n, jump, mcp, open, privacy, projects,
    search, show, view,
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
    /// Output language: en, zh-CN or ja (default: follow the OS locale)
    #[arg(long, global = true, value_name = "zh-CN|en")]
    lang: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Scan local sessions and incrementally build the SQLite index
    Collect(collect::CollectArgs),
    /// Fuzzy recall search over collected sessions
    Search(search::SearchArgs),
    /// Print the full text of one session
    Show(show::ShowArgs),
    /// Generate a readable HTML page (turn anchors; --open to launch the browser)
    View(view::ViewArgs),
    /// Export markdown (Obsidian-compatible frontmatter)
    Export(export::ExportArgs),
    /// List merged projects (folder / multi-root hashes merged by root paths)
    Projects(projects::ProjectsArgs),
    /// Open the VS Code window that owns a session
    Open(open::OpenArgs),
    /// Jump back: queue a jump request and open/focus the owning window
    Jump(jump::JumpArgs),
    /// Run as an MCP server over stdio (search_sessions / read_session)
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
    let cli = Cli::parse();
    let lang = i18n::Lang::resolve(cli.lang.as_deref(), &privacy::PrivacyPolicy::default_path());
    i18n::set_lang(lang);
    match cli.cmd {
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
