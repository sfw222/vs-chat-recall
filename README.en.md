# vs-chat-recall

Cross-workspace recall for your **VS Code / GitHub Copilot Chat session history**
(including empty-window sessions). Search fuzzy memory in plain words — "I think we
discussed some xx approach once" — land on the exact turn, then read it in the
terminal, a browser, or back inside VS Code.

**Fully local:** it only reads VS Code's own session files; the index and every
export stay on your machine.

[中文说明 →](README.md) · [日本語 →](README.ja.md)

## Commands

| Command | What it does |
|---|---|
| `recall collect` | Scan every local session and index it **incrementally** (SQLite; milliseconds when nothing changed) |
| `recall search "what you remember"` | Fuzzy search; ≥3 chars uses the FTS5 trigram index, shorter words fall back to LIKE; `--project` to filter |
| `recall show <id>` | Print the whole session in the terminal (`--turn N` for one turn) |
| `recall view <id> --open` | Render a readable HTML page (turn anchors, TOC rail, long-message collapse, dark theme) and open it |
| `recall export <id>` | Export Markdown with Obsidian-compatible frontmatter |
| `recall jump <id>` | Open the original session via VS Code's internal session URI (optional extension, see below) |
| `recall mcp` | Run the built-in MCP server so Copilot Chat can recall the discussion for you, with enforced privacy filters |

Typical flow:

```
recall collect
recall search "login flow"
#  1. [my-game] Login & auth refactor · 2026-10-02 · turn 3
#     user: … turn the login flow into two steps …
#     (recall view 1a2b3c4d --turn 3 --open)
recall view 1a2b3c4d --turn 3 --open     # read it in the browser (anchor jumps to the hit)
recall jump 1a2b3c4d                     # or open the original session in VS Code
```

## Getting started

Requires a Rust toolchain (developed on 1.97). Tested mainly on Windows
(reads under `%APPDATA%\Code\User`); other platforms are unverified.

```powershell
cargo build --release
.\target\release\recall.exe collect
.\target\release\recall.exe search "some words you remember"
```

Optional: put `recall` on PATH (`scripts/install-cli-shim.ps1` installs a
forwarding shim).

Read-only data sources:

- `%APPDATA%\Code\User\workspaceStorage\<hash>\chatSessions\*.jsonl`
- `%APPDATA%\Code\User\globalStorage\emptyWindowChatSessions\*.jsonl`

The index defaults to `data/recall.db` (override with `--db`); `data/` is
git-ignored.

> The session format is a VS Code implementation detail (verified on VS Code
> 1.142). A VS Code upgrade may change it: the parser is deliberately lenient
> (unknown events are counted and skipped, never fatal), but new fields may need
> follow-up adaptation.

## Language

Bundled languages: **English** (`en`), **Simplified Chinese** (`zh-CN`) and
**Japanese** (`ja`). Runtime messages follow your system locale (`zh*` →
Chinese, `ja*` → Japanese, anything else → English). Override in order of
precedence:

1. `--lang <tag>` on any command (e.g. `recall --lang ja search …`)
2. the `RECALL_LANG` environment variable
3. `"language": "zh-CN" | "ja" | "en" | "auto"` in `recall.config.json`
4. the OS locale

`--help` texts are English.

Adding a language: drop a `<tag>.json` catalog next to `src/i18n/en.json`, then
add the enum variant and the arms in `code()` / `from_tag()` / `catalog()`.
`en.json` is the source of truth — a key missing from your catalog falls back to
English instead of showing the raw key, so translations can land incrementally.
Tests enforce that en.json covers every call-site key, that no catalog invents
keys, and that each translation covers at least 80% of en.json (missing keys are
printed by `cargo test -- --nocapture`).

## Copilot Chat integration (MCP, optional)

Register it in VS Code's `mcp.json` (point `command` at your built binary):

```json
{
  "servers": {
    "recall": {
      "type": "stdio",
      "command": "C:\\path\\to\\recall.exe",
      "args": ["mcp", "--db", "C:\\path\\to\\data\\recall.db",
               "--config", "C:\\path\\to\\recall.config.json"]
    }
  }
}
```

Two tools are exposed: `search_sessions` (fuzzy recall returning snippets and
locations) and `read_session` (bounded full text of a session or one turn).

Privacy policy (`recall.config.json`):

```json
{ "mcp": { "exclude_projects": ["projectA"], "allow_projects": [] } }
```

- Case-insensitive substrings matched against the project label or workspace
  root set; `exclude_projects` wins, and a non-empty `allow_projects` is an
  allow-list;
- **Three enforcement layers**: SQL-level filtering → post-fetch guard →
  `read_session` refusal (byte-for-byte identical to "not found");
- A broken config fails closed (no access); edits take effect on the **next
  call** (hot reload).

## Jump back inside VS Code (optional extension)

`extensions/recall-jump/` is a tiny local extension:

```powershell
pwsh -File scripts/install-recall-jump.ps1   # copies into ~/.vscode/extensions and reloads the window
```

Run **`Recall: Open Past Session (Jump)`** from the command palette and enter a
session id. VS Code's session store is window-scoped, so sessions that belong to
another window are opened there via a "jump request" file (`recall jump <id>`
works the same way).

Settings: `recall.db` is required (the index built by `recall collect --db
<path>`); `recall.executable` defaults to `recall` on PATH; the jump request
file defaults to the system temp directory.

## Privacy

- The tool is **read-only** towards VS Code's session files;
- Everything (index, exports, HTML) stays on your machine, and this repository
  contains no real session content;
- `fixtures/synthetic/` holds **hand-crafted** edge cases; the real-sample
  directory is git-ignored entirely;
- The MCP entry supports per-project exclusion (above): excluded content never
  appears in any tool result.

## Development

```powershell
cargo test          # regression tests for parsing / indexing / search / MCP / HTML
```

Repository layout:

```
src/                  Rust implementation (jsonl replay, extraction, SQLite, search, export, MCP, HTML)
src/i18n/             message catalogs (en, zh-CN, ja)
tests/                integration tests (synthetic fixtures, dual-hash merge, MCP end to end)
fixtures/synthetic/   hand-crafted edge cases + expected/ assertions
extensions/recall-jump/  optional jump-back extension
scripts/              install and maintenance scripts
recall.config.json    example MCP privacy policy
```

Reference performance (measured locally, order of magnitude only): ~190 session
files / 4.4 GB of logs — first index ~25 s, subsequent no-change runs ~40 ms; the
largest single file is 492 MB (with one 157 MB line) and is handled fine.

## Notes

- Unofficial tool, not affiliated with Microsoft or GitHub.
- Early version: commands and configuration may still change.

## License

[MIT](LICENSE)
