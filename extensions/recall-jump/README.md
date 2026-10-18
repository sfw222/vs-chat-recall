# Recall Jump — Copilot Chat History

Collect & manage your **VS Code / GitHub Copilot Chat session history** and jump back
into any past session with one command. Companion extension for the **`recall` CLI**
([download](https://github.com/sfw222/vs-chat-recall/releases) · [source](https://github.com/sfw222/vs-chat-recall)).

> ⚠️ **This extension needs the `recall` CLI.** It has no built-in indexer — it asks the CLI
> where a session lives and then opens it. Install the CLI first, run `recall collect` once,
> then install this extension.

## What it does

VS Code keeps chat sessions in a **window-scoped** store: a workspace window can only open
sessions that belong to it, and asking for a foreign session silently starts a *blank new*
session. This extension checks the ownership first and does the right thing:

- session belongs to this window → opens it as a normal **chat editor tab**
- session belongs to another window → offers **“Open in its window and jump”**, writes a jump
  request, opens/focuses the owning window, and the extension there completes the jump
- an empty-window session → opens a new empty window and jumps there

## Install

1. Install the CLI and build the index once:

   ```powershell
   recall collect
   ```

2. Install this extension (Marketplace, or **Install from VSIX…** with the `.vsix` from the
   [releases page](https://github.com/sfw222/vs-chat-recall/releases)).

3. Point it at the index — either set `recall.db` in Settings, or run
   **`Recall: Select Index File…`** from the command palette.

4. Jump:

   ```
   Recall: Open Past Session (Jump)   →   paste a session id (UUID)
   ```

   Get session ids from `recall search "…"` in the terminal.

On first activation the extension checks that the CLI and the index are present and offers
one-click fixes if they are not.

## Commands

| Command | What it does |
|---|---|
| `Recall: Open Past Session (Jump)` | Open a session id (or a full `vscode-chat-session:` URI) |
| `Recall: Select Index File…` | Pick the `recall.db` index and store it in your user settings |
| `Recall: Select recall Executable…` | Pick `recall.exe` when it is not on `PATH` |
| `Recall: Run collect (Build the Index)` | Run `recall collect --db <index>` in a terminal |
| `Recall: Check Setup` | Re-run the first-run diagnostics on demand |

## Settings

| Setting | Description |
|---|---|
| `recall.db` | Path to the index built by `recall collect --db <path>`. Required. |
| `recall.executable` | Path to the CLI. Empty = use `recall` from `PATH`. |
| `recall.pendingFile` | Jump request file. Empty = `recall-jump-request.json` in the system temp dir. |

## Privacy

- **No telemetry, no network requests.** The extension only spawns the local CLI, reads/writes
  the jump-request file, and runs VS Code commands.
- It never reads chat content itself — session lookups go through `recall show <id> --json --meta`.
- Everything stays on your machine, and it respects the CLI's own privacy policy
  (`recall.config.json`, which filters excluded projects server-side).

## Notes

- Windows-first (the CLI reads `%APPDATA%\Code\User`); requires VS Code `1.100+`.
- Unofficial tool, not affiliated with Microsoft or GitHub.
- Localized: English, Simplified Chinese, Japanese.
- How the jump works (session store is window-scoped; `vscode-chat-session://local/<base64url(id)>`
  resolves through the editor resolver) is documented in the
  [repository](https://github.com/sfw222/vs-chat-recall).

---

# Recall Jump — Copilot Chat 历史会话

搜集并管理你的 **VS Code / GitHub Copilot Chat 历史会话**，一条命令跳回任意历史会话。
本扩展是 **`recall` CLI** 的配套件（[下载](https://github.com/sfw222/vs-chat-recall/releases) ·
[源码](https://github.com/sfw222/vs-chat-recall)）。

> ⚠️ **必须先安装 `recall` CLI**：扩展本身不做索引，它只是问 CLI「这个会话在哪」然后打开。先装 CLI、
> 跑一次 `recall collect`，再装本扩展。

## 它能解决什么

VS Code 的会话存储**按窗口隔离**：有工作区的窗口只能打开属于自己的会话，硬开别人的会话会**静默新建一个空白会话**。
本扩展先判断归属，再决定怎么做：

- 属于当前窗口 → 直接打开为 Chat 编辑器标签页
- 属于其他窗口 → 提示并**「在所属窗口打开并跳转」**：写跳转请求 → 打开/聚焦所属窗口 → 那边的扩展完成跳转
- 空窗口会话 → 打开一个新的空窗口并跳转

## 安装

1. 安装 CLI 并构建索引：`recall collect`
2. 安装本扩展（Marketplace，或用 Release 里的 `.vsix` **Install from VSIX…**）
3. 指定索引：设置里填 `recall.db`，或命令面板运行 **`Recall: 选择索引文件…`**
4. 命令面板运行 **`Recall: 打开历史会话（jump）`**，粘贴会话 id（终端里 `recall search "…"` 可拿到）

首次激活时扩展会自检 CLI 与索引是否存在，缺什么就给一键修复按钮。

## 命令与设置

| 命令 | 作用 |
|---|---|
| `Recall: 打开历史会话（jump）` | 打开会话 id 或完整的 `vscode-chat-session:` URI |
| `Recall: 选择索引文件…` | 选 `recall.db` 并写入用户设置 |
| `Recall: 选择 recall 可执行文件…` | `recall.exe` 不在 PATH 时手动指定 |
| `Recall: 运行 collect（构建索引）` | 在终端里执行 `recall collect --db <索引>` |
| `Recall: 检查配置` | 手动重跑首启自检 |

设置项：`recall.db`（必填）、`recall.executable`（留空用 PATH）、`recall.pendingFile`（留空用系统临时目录）。

## 隐私

**无遥测、无网络请求。** 只做三件事：调用本机 CLI、读写跳转请求文件、执行 VS Code 命令。
扩展不直接读取聊天内容，一律经 `recall show <id> --json --meta`；数据全部留在本机，并遵守 CLI 的
`recall.config.json` 隐私策略（被排除的项目在服务端就被过滤）。

## 说明

- Windows 优先（CLI 读取 `%APPDATA%\Code\User`）；需要 VS Code `1.100+`
- 非官方工具，与 Microsoft / GitHub 无关联
- 已本地化：English / 简体中文 / 日本語

## License

[MIT](LICENSE)
