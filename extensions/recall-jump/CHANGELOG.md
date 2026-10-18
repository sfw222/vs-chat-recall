# Changelog

## 1.0.0 — 2026-10-18

首个公开版本（配合 `recall` CLI v0.1.0 发布到 VS Code Marketplace）。

- 打开历史会话：命令面板 → **Recall: 打开历史会话（jump）**，输入会话 id 即把该会话作为 Chat 标签页打开
- 作用域感知：先判断会话属于当前窗口还是别的窗口；跨窗口时写跳转请求并打开/聚焦所属窗口，由那边的扩展接手
- **首启检测**：启动后发现缺少 `recall` CLI 或索引库时弹出提示，可直接「选择可执行文件」「选择索引文件」「运行 recall collect」「打开下载页」
- **新增命令**：选择索引文件 / 选择 recall 可执行文件 / 运行 collect（构建索引）/ 检查配置
- 配置写入使用 **全局（User）作用域**，多个窗口共用
- 本地化：命令标题与提示支持 English / 简体中文 / 日本語
- 发布元数据：publisher `sfw222`、MIT、仓库与 issue 链接、关键词

## 0.0.2 — 2026-10-18

- 跨窗口跳转（跳转请求文件 + 打开/聚焦所属窗口）
- 侧载安装脚本 `scripts/install-recall-jump.ps1`
- 本地化清单（package.nls）+ 运行时文案（l10n bundle）

## 0.0.1 — 2026-10-18

- 首个可用版本：`vscode-chat-session://local/<base64url(id)>` 直接打开历史会话
