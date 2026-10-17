# vs-chat-recall

[![CI](https://github.com/sfw222/vs-chat-recall/actions/workflows/ci.yml/badge.svg)](https://github.com/sfw222/vs-chat-recall/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/sfw222/vs-chat-recall?display_name=tag&sort=semver)](https://github.com/sfw222/vs-chat-recall/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.97-orange.svg)](https://www.rust-lang.org/)
![Platform](https://img.shields.io/badge/platform-Windows-0078D6.svg)

**VS Code / GitHub Copilot Chat のセッション履歴**（空のウィンドウのセッションも含む）を
ワークスペースをまたいで横断検索できるようにするツールです。「どこかで似た方針を
相談した気がする」という程度の曖昧な記憶でも、思いついた言葉のまま検索して該当の
turn を特定し、ターミナル・ブラウザー・VS Code のいずれでも全文を読めます。

**完全にローカル:** VS Code 自身のセッションファイルを読むだけで、インデックスも
エクスポートもすべて手元に残ります。

[English →](README.en.md) · [中文说明 →](README.md)

## コマンド

| コマンド | 内容 |
|---|---|
| `recall collect` | ローカルの全セッションを走査して**増分**でインデックス化（SQLite。変更がなければミリ秒） |
| `recall search "覚えている語句"` | 曖昧検索。3 文字以上は FTS5 trigram インデックス、短い語は LIKE にフォールバック。`--project` で絞り込み |
| `recall show <id>` | セッション全文をターミナルに表示（`--turn N` で 1 ターンだけ） |
| `recall view <id> --open` | 読みやすい HTML ページを生成して開く（turn アンカー、左側の目次、長文の折りたたみ、ダークテーマ） |
| `recall export <id>` | Obsidian 互換 frontmatter 付きの Markdown を書き出す |
| `recall jump <id>` | VS Code 内部のセッション URI で元のセッションを開く（任意の拡張。下記参照） |
| `recall mcp` | 内蔵 MCP サーバーとして Copilot Chat に接続し、モデルが「あのときの議論」を探し出せるようにする（強制プライバシーフィルター付き） |

典型的な流れ:

```
recall collect
recall search "ログインの流れ"
#  1. [my-game] Login & auth refactor · 2026-10-02 · turn 3
#     user: … ログインの流れを 2 ステップに変えて …
#     (recall view 1a2b3c4d --turn 3 --open)
recall view 1a2b3c4d --turn 3 --open     # ブラウザーで読む（アンカーがヒット位置に飛ぶ）
recall jump 1a2b3c4d                     # あるいは VS Code で元のセッションを開く
```

## はじめ方

Rust ツールチェーンが必要です（開発は 1.97）。主に Windows で検証済み
（`%APPDATA%\Code\User` 以下を読みます）。他プラットフォームは未検証です。

```powershell
cargo build --release
.\target\release\recall.exe collect
.\target\release\recall.exe search "覚えている語句"
```

任意: `recall` を PATH に置く（`scripts/install-cli-shim.ps1` が転送用 shim を入れます）。

読み取り専用のデータソース:

- `%APPDATA%\Code\User\workspaceStorage\<hash>\chatSessions\*.jsonl`
- `%APPDATA%\Code\User\globalStorage\emptyWindowChatSessions\*.jsonl`

インデックスの既定は `data/recall.db`（`--db` で変更）。`data/` は git 管理外です。

> セッションの形式は VS Code の実装詳細です（VS Code 1.142 で検証）。VS Code の
> 更新で変わる可能性があります。パーサーは意図的に寛容で、未知のイベントは数えて
> 読み飛ばすだけで中断しませんが、新しいフィールドには追従が必要になることがあります。

## 言語

同梱の言語は **英語**（`en`）、**簡体中国語**（`zh-CN`）、**日本語**（`ja`）。実行時の
メッセージはシステムのロケールに従います（`zh*` → 中国語、`ja*` → 日本語、
それ以外 → 英語）。次の優先順位で上書きできます:

1. 任意のコマンドに `--lang <tag>`（例: `recall --lang ja search …`）
2. 環境変数 `RECALL_LANG`
3. `recall.config.json` の `"language": "zh-CN" | "ja" | "en" | "auto"`
4. システムのロケール

`--help` の文言は常に英語です。

言語を追加するには、`src/i18n/en.json` の隣に `<tag>.json` を置き、`Lang` の
バリアントと `code()` / `from_tag()` / `catalog()` の各分岐を足すだけです。
`en.json` が基準（source of truth）です: あなたのカタログに無いキーは英語にフォールバックし、
生のキーが画面に出ることはないため、翻訳は段階的に入れられます。テストは
「en.json が全呼び出し箇所のキーを持つこと」「他カタログが余計なキーを持たないこと」
「各翻訳が en.json の 80% 以上をカバーすること」を強制します（不足分は
`cargo test -- --nocapture` で表示）。

## Copilot Chat 連携（MCP・任意）

VS Code の `mcp.json` に登録します（`command` はビルドした実行ファイルのパス）:

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

提供されるツールは 2 つ: `search_sessions`（抜粋と位置を返す曖昧検索）と
`read_session`（セッション全体または 1 ターンの本文を上限付きで返す）。

プライバシーポリシー（`recall.config.json`）:

```json
{ "mcp": { "exclude_projects": ["projectA"], "allow_projects": [] } }
```

- プロジェクトラベルまたはワークスペースルート集合に対する大文字小文字を
  区別しない部分一致。`exclude_projects` が優先で、`allow_projects` が非空なら
  それが許可リストになります。
- **3 段階の強制**: SQL レベルでの絞り込み → 取得後の再確認 → `read_session` の拒否
  （「存在しない」場合とバイト単位で完全に同一の応答）。
- 設定が壊れている場合は fail-closed（アクセス拒否）。編集は**次の呼び出し**から
  有効（ホットリロード）。

## VS Code 内にジャンプする（任意の拡張）

`extensions/recall-jump/` は小さなローカル拡張です:

```powershell
pwsh -File scripts/install-recall-jump.ps1   # ~/.vscode/extensions にコピーしてウィンドウを再読込
```

コマンドパレットで **`Recall: 過去のセッションを開く（jump）`** を実行し、
セッション id を入力します。VS Code のセッションストアはウィンドウ単位なので、
別ウィンドウに属するセッションは「ジャンプ要求」ファイルを経由して、
そのウィンドウ側で開かれます（`recall jump <id>` も同じ仕組みです）。

設定: `recall.db` は必須（`recall collect --db <path>` で作ったインデックス）。
`recall.executable` は既定で PATH 上の `recall`。ジャンプ要求ファイルは既定で
システムの一時ディレクトリです。

## プライバシー

- ツールは VS Code のセッションファイルに対して**読み取り専用**です。
- すべて（インデックス、エクスポート、HTML）は手元に残り、このリポジトリには
  実セッションの内容を一切含みません。
- `fixtures/synthetic/` は**手で作った**境界ケースで、実サンプルのディレクトリは
  まるごと git 管理外です。
- MCP はプロジェクト単位の除外に対応（上記）。除外された内容はどのツール結果にも
  現れません。

## 開発

```powershell
cargo test          # パース / インデックス / 検索 / MCP / HTML の回帰テスト
```

リポジトリ構成:

```
src/                  Rust 実装（jsonl リプレイ、抽出、SQLite、検索、エクスポート、MCP、HTML）
src/i18n/             メッセージカタログ（zh-CN, ja, en）
tests/                統合テスト（合成フィクスチャ、二重ハッシュの併合、MCP エンドツーエンド）
fixtures/synthetic/   手作りの境界ケース + expected/ の期待値
extensions/recall-jump/  任意のジャンプ用拡張
scripts/              インストールと保守のスクリプト
recall.config.json    MCP プライバシーポリシーの例
```

参考性能（ローカル実測、桁の参考値）: 約 190 セッションファイル / 4.4 GB のログで、
初回インデックスが約 25 秒、変更がなければ再実行は約 40 ミリ秒。最大の単一ファイルは
492 MB（1 行 157 MB を含む）で、問題なく処理できます。

## 注意

- 非公式ツールです。Microsoft / GitHub とは関係ありません。
- 早期バージョンのため、コマンドと設定は変更される可能性があります。

## License

[MIT](LICENSE)
