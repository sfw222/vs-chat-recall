# recall-jump —— 本地侧载扩展（P5 跳回）

把 recall 索引中的历史会话**直接打开为 VS Code 的 Chat 编辑器标签页**（作用域感知，支持跨窗口）。

## 安装（无需打包）

```powershell
pwsh -File scripts/install-recall-jump.ps1
```

然后 **重载窗口**（命令面板 → `Developer: Reload Window`）。

## 使用

- 命令面板 → **`Recall: 打开历史会话（jump）`** → 输入会话 id（UUID）。
- 或在终端：`recall jump <id>`（写跳转请求 + 打开/聚焦所属窗口，目标窗口的扩展自动跳转）。

## 作用域规则（2026-10-18 实测 + 上游源码）

- 本地会话存储**按窗口作用域**：有工作区的窗口只读自己的
  `workspaceStorage/<hash>/chatSessions`；只有空窗口读 `globalStorage/emptyWindowChatSessions`。
- 若要打开的会话不属于当前窗口，`ChatEditorInput` 会**静默新建一个空白会话**（上游 fallback）。
  因此扩展先用 `recall show <id> --json --meta` 判断归属：
  - 同窗口 → 直接 `vscode.open`；
  - 跨窗口 → 提示并提供「在所属窗口打开并跳转」→ 写 `data/jump-request.json` + 打开目标窗口，
    目标窗口的扩展（`onStartupFinished` / 窗口获得焦点时）消费请求并打开会话。

## 原理（上游源码核对）

- URI：`vscode-chat-session://local/<base64url(sessionId)>`（无 padding）
  （`chatUri.ts#LocalChatSessionUri.forSession`；scheme 常量名 `vscodeLocalChatSession`，**值**为 `'vscode-chat-session'`）。
- 该 scheme 注册在**编辑器解析器**上（`chat.shared.contribution.ts#ChatResolverContribution`）→ `vscode.open` 即进入 `ChatEditorInput`。

## 配置（settings）

- `recall.db`（**必填**）：`recall collect --db <path>` 生成的索引库路径；
- `recall.executable`：留空时使用 PATH 中的 `recall`（可用 `scripts/install-cli-shim.ps1` 安装）；
- `recall.pendingFile`：留空时使用系统临时目录下的 `recall-jump-request.json`（与 `recall jump` 的默认值一致）。

## 卸载

删除 `%USERPROFILE%\.vscode\extensions\local.recall-jump-0.0.2\` 并重载窗口。

## 兜底

- `recall view <id> --open`：离线 HTML 页面
- `recall open <id>`：打开所属工作区窗口

