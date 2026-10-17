# vs-chat-recall

[![CI](https://github.com/sfw222/vs-chat-recall/actions/workflows/ci.yml/badge.svg)](https://github.com/sfw222/vs-chat-recall/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/sfw222/vs-chat-recall?display_name=tag&sort=semver)](https://github.com/sfw222/vs-chat-recall/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.97-orange.svg)](https://www.rust-lang.org/)
![Platform](https://img.shields.io/badge/platform-Windows-0078D6.svg)

跨工作区检索你的 **VS Code / GitHub Copilot Chat 历史会话**（含空窗口会话），
支持**中文模糊回忆检索**：只记得"我好像讨论过某个 xx 方案"，用一句中文就能搜回去，
定位到具体一次问答，并在终端、浏览器或 VS Code 里查看完整内容。

**数据完全本地**：只读 VS Code 的会话文件，索引与导出都保存在本机，不上传任何内容。

[English →](README.en.md) · [日本語 →](README.ja.md)

## 功能

| 命令 | 作用 |
|---|---|
| `recall collect` | 扫描本机全部会话并**增量**入库（SQLite；无变化时毫秒级完成） |
| `recall search "记得的片段"` | 中文模糊检索；≥3 字走 FTS5 trigram 索引，短词自动 LIKE 兜底；可 `--project` 过滤 |
| `recall show <id>` | 终端打印完整会话（`--turn N` 只看一轮） |
| `recall view <id> --open` | 生成可读 HTML 页面（turn 锚点、左侧目录、长消息折叠、深色主题）并在浏览器打开 |
| `recall export <id>` | 导出 Markdown（带 Obsidian 兼容的 frontmatter） |
| `recall jump <id>` | 用 VS Code 内部会话 URI 直接打开原会话（需可选扩展，见下文） |
| `recall mcp` | 以内置 MCP server 接入 Copilot Chat，让模型替你"找回当时的讨论"，带强制隐私过滤 |

工作流示例：

```
recall collect
recall search "登录流程"
#  1. [my-game] 登录与鉴权重构 · 2026-10-02 · turn 3
#     user: ……把登录流程改成两步……
#     (recall view 1a2b3c4d --turn 3 --open)
recall view 1a2b3c4d --turn 3 --open     # 浏览器里读全文（锚点直达命中轮）
recall jump 1a2b3c4d                     # 或在 VS Code 里打开原会话
```

## 快速开始

需要 Rust 工具链（开发使用 1.97）。主要在 Windows 上实测（读取 `%APPDATA%\Code\User`），其他平台未验证。

```powershell
cargo build --release
.\target\release\recall.exe collect
.\target\release\recall.exe search "模糊回忆的中文词"
```

可选：把 `recall` 放进 PATH（`scripts/install-cli-shim.ps1` 会安装一个转发 shim）。

数据源（只读）：

- `%APPDATA%\Code\User\workspaceStorage\<hash>\chatSessions\*.jsonl`
- `%APPDATA%\Code\User\globalStorage\emptyWindowChatSessions\*.jsonl`

索引默认写到 `data/recall.db`（`--db` 可改），`data/` 已被 git 忽略。

> 会话格式为 VS Code 内部实现（已在 VS Code 1.142 实测）。升级 VS Code 后格式可能变化：
> 解析器采用"宽进"策略（未知事件计数跳过，不会中断），但新字段可能需要后续适配。

## 语言

内置语言：**英文**（`en`）、**简体中文**（`zh-CN`）、**日文**（`ja`）。运行时输出跟随系统区域（`zh*` → 中文，`ja*` → 日文，其余 → 英文）。按以下优先级覆盖：

1. 任意命令加 `--lang <tag>`（如 `recall --lang ja search …`）
2. 环境变量 `RECALL_LANG`
3. `recall.config.json` 的 `"language": "zh-CN" | "ja" | "en" | "auto"`
4. 系统区域

`--help` 文案恒为英文。

新增语言：在 `src/i18n/en.json` 旁放一份 `<tag>.json`，再加枚举分支及 `code()` / `from_tag()` / `catalog()` 里的对应项。`en.json` 是唯一权威：你的目录缺哪个键就回退到英文（不会把原始 key 显示给用户），因此翻译可以分批落地。测试会强制：en.json 覆盖全部调用点 key、其他目录不得出现多佘 key、每份翻译至少覆盖 en.json 的 80%（缺失清单可用 `cargo test -- --nocapture` 查看）。

## 接入 Copilot Chat（MCP，可选）

在 VS Code 的 `mcp.json` 中注册（`command` 换成你构建出的可执行文件路径）：

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

提供两个工具：`search_sessions`（模糊检索，返回片段与位置）与 `read_session`（读取会话/单轮全文，有界）。

隐私策略（`recall.config.json`）：

```json
{ "mcp": { "exclude_projects": ["项目A"], "allow_projects": [] } }
```

- 大小写不敏感的子串，匹配项目名或工作区根路径集合；`exclude_projects` 优先，`allow_projects` 非空即白名单；
- **三层过滤**：查询 SQL 层过滤 → 返回前复核 → `read_session` 拒绝（与"不存在"逐字一致）；
- 配置损坏时 fail-closed（拒绝访问）；修改后**下一次调用即生效**（热加载）。

## 在 VS Code 中跳回原会话（可选扩展）

`extensions/recall-jump/` 是一个很小的本地扩展：

```powershell
pwsh -File scripts/install-recall-jump.ps1   # 复制到 ~/.vscode/extensions 并重载窗口
```

命令面板运行 **`Recall: 打开历史会话（jump）`** 并输入会话 id。VS Code 的会话存储按窗口隔离，
因此跨窗口的会话会经由一个"跳转请求"在所属窗口里打开（`recall jump <id>` 同样如此）。

设置：必须指定 `recall.db`（索引路径，`recall collect --db <path>` 生成的库）；
`recall.executable` 留空时使用 PATH 中的 `recall`；跳转请求文件默认写到系统临时目录。

## 隐私

- 工具**只读** VS Code 的会话文件，不修改、不删除；
- 所有数据（索引、导出、HTML）保存在本机，仓库不含任何真实会话内容；
- 仓库中的 `fixtures/synthetic/` 为**手工构造**的边界样例；真实样本目录整体被 git 忽略；
- MCP 入口支持按项目排除（见上），被排除的内容不会出现在任何工具结果里。

## 开发

```powershell
cargo test          # 解析/索引/检索/MCP/HTML 的回归测试
```

仓库结构：

```
src/                  Rust 实现（.jsonl 重放、抽取、SQLite、检索、导出、MCP、HTML）
src/i18n/             多语言目录（en、zh-CN、ja）
tests/                集成测试（合成夹具、双 hash 归并、MCP 端到端）
fixtures/synthetic/   手工构造的边界夹具 + expected/ 断言
extensions/recall-jump/  可选的跳回扩展
scripts/              安装与维护脚本
recall.config.json    MCP 隐私策略示例
```

参考性能（本机实测，仅作量级参考）：约 190 个会话文件 / 4.4 GB 日志，首次入库 ~25s，
之后无变化增量 ~40ms；最大单文件 492 MB（含 157 MB 单行）可正常处理。

## 说明

- 非官方工具，与 Microsoft / GitHub 无关联。
- 目前为早期版本，命令与配置可能调整。

## License

[MIT](LICENSE)
