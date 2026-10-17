# fixtures/synthetic —— 合成边界夹具（进 git）

全部为**手工构造**的最小文件，不包含任何真实会话内容，覆盖解析器必须通过的全部边界。
P1 用它们做回归测试（与 `fixtures/real` 的"保真"用途互补）；断言在 `tests/synthetic.rs`（8 个测试全绿）。

## 夹具清单（P1 已完成）

- [x] `initial-only.jsonl`：只有 1 条 kind:0 的空会话（`requests: []`）
- [x] `set-nested.jsonl`：kind:1 深路径赋值（数组内对象字段）+ 整对象替换 + 错误路径计数
- [x] `push-append.jsonl`：kind:2 无 `i` 追加（含 `message.parts` 兜底、thinking/tool 部件）
- [x] `push-insert.jsonl`：kind:2 带 `i`——**上游语义 = 先截断到 i 再追加 v**（`arr.length=i; arr.push(...v)`），
      不是"插入到中间"；文件名沿用原清单，语义以本条注释为准
- [x] `push-truncate.jsonl`：kind:2 带 `i` 且无 `v` 的截断；另覆盖"push 自动创建缺失数组"
- [x] `delete-restore.jsonl`：kind:3 删除后 kind:1 重建；删除缺失父路径计入 applyErr
- [x] `compact-rewrite.jsonl`：中途 kind:0 = 压缩重写（**整体替换 state**，非合并）
- [x] `huge-line.jsonl`：含 1MiB 字符串的两条超长行（~2MB 文件，由 `scripts/gen-synthetic-large.ps1`
      生成后入库）；覆盖 `--max-line-bytes` 跳过与 `--max-part-chars` 截断
- [x] `partial-tail.jsonl`：尾行 JSON 不完整（**末行无换行符**，并发写中断容错）
- [x] `unknown-kind.jsonl`：未知 kind（数字 9 / 字符串）、多余字段（宽进策略 + unparsed 计数）
- [x] `legacy-v3.json`：legacy 单文件 `.json` 样例。**形态为推断**（本机 0 个真实旧格式文件），
      仅用于防回归的"兼容性下限"；未来出现真实旧文件应先补真实样本再修断言

期望结果在 `expected/<name>.json`（turns 文本、异常计数、sessionId/customTitle），与夹具一一对应。

