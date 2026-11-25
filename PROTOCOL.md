# Bits Style Scanning Protocol V0.4

本协议只负责发现文件、扫描、自动修复、基准评测和输出结果。规范分为 L0（外部工具）、L1（AST 拓扑：`AUTO-CORE` / `AUTO-PROFILE`）和 L2（品味审查：`REVIEW`）；L2 不参与静态扫描 PASS/FAIL。

## 1. 命令

```bash
bits style fix [paths...] [options]
bits style check [paths...] [options]
bits style report [paths...] [options]
bits bench [--manifest benchmarks/manifest.toml] [--corpus corpus/bad] [--offline] [--strict]
```

- `fix` 只执行确定不改变程序语义的修复，然后重新扫描。修复必须幂等。
- `check` 只读扫描；无违规返回 0，有违规返回 1。
- `report` 使用与 `check` 相同的扫描结果并输出结构化报告；报告成功生成时返回 0，扫描未完成返回 2。
- `bench` 运行正负样本基准：正样本检查 `before` 召回与 `after` 消除；负样本拉取锁定 commit 的开源标杆库，只统计 L1 结构与语义规则（排除 `BCS001/002/007/008/009` 语言格式偏好）。正样本未全过或 `--strict` 下负样本存在硬错误时返回 1。

## 2. 参数

```text
--all                 扫描指定根目录下全部支持的源文件
--changed             扫描工作区和暂存区相对 HEAD 发生变化的文件
--staged              只扫描暂存区中的文件
--diff                只报告新增/修改行相关的违规
--commit <commit>     指定 diff 的比较基线；必须搭配 --diff
--format <format>     text、json、jsonl、sarif
--output <file>       写入文件；未指定时写 stdout
--jobs <n>            并行扫描数量；默认使用 CPU 并行度（上限 8），不改变输出顺序
--config <file>       指定扫描配置
--baseline <file>     使用既有 JSON 报告作为存量违规基线
--no-ignore           不读取 .gitignore，但仍排除 .git 和扫描器自身产物
--include-passed      report 中包含通过的检查项
```

作用域参数互斥。未指定路径和作用域时，默认 `bits style check --all .`。

## 2.1 Diff-only

```bash
bits style check --diff
bits style check --diff --commit HEAD~3
bits style check --diff --commit 61a5793 src
```

- `--diff` 默认执行 `git diff HEAD`，包含工作区和暂存区相对基线的变化。
- `--commit` 接受 SHA、分支、tag 或 `HEAD~n` 等 Git revision。
- 文件仍完整解析，但只报告新增/修改行上的违规；函数级违规在函数范围与新增行相交时报告。
- patch 按文件头和 hunk 行数解析；正文不会被当作元数据。hunk 损坏、截断或行数不闭合属于扫描错误，返回 2。
- 未跟踪文件按全文件检查；纯删除且没有新增行的文件不进入扫描。
- 为检查新增依赖环，按需解析本地 import 可达的未变更文件作为只读上下文。上下文不计入 `files_scanned`、coverage 或 unsupported，其普通违规和解析错误不进入 diff 报告；只有与新增/修改行相交的依赖环诊断会被保留。
- `--changed` 和 `--staged` 保持原有的整文件扫描行为。
- **`improving` 棘轮**：对基线 commit 中已存在的同名函数，若 `BCS003` 的嵌套层数或圈复杂度仍超阈值但相对基线**严格下降**，扫描器读取 `git show <base>:./<file>` 比对后，将该诊断置为 `improving = true`、`severity = warning`、`obligation = ratchet_improving`，不阻断 `check`。新函数或指标持平/恶化仍为硬错误。

文件发现遵循 `.gitignore`，排除 `.git/`、依赖目录、构建产物、明确的生成代码和目录符号链接。同一文件规范化后只扫描一次。没有匹配文件属于成功扫描。

## 3. Profile 与覆盖

语言 Profile 决定缩进、括号、命名和格式化方式。AUTO-CORE 规则使用统一的语义定义。

`coverage.executed` 列出至少对一种输入语言实际执行的规则；`coverage.partial` 列出只支持保守语法子集的规则；`coverage.not_applicable` 和 `coverage.not_implemented` 防止将可解析语言误解为完整覆盖。

`status = passed` 只表示当前适配器实际执行的检查通过，不证明 REVIEW 规则或未实现的语义。

## 3.1 Violation、Warning 与 Unsupported

- `severity = violation`：确定违规，`check` 返回 1。真实嵌套、圈复杂度、空 catch、真实参数重赋值、依赖环和不可达代码属于此类。
- `severity = warning`：需要人工判断的提示，不阻断 CI。BCS003 的行数/局部变量、TSX 保守格式提示和无法确认边界的 catch-all 转换属于此类。
- `confidence = high|low`：描述判定置信度，不代替 severity。升级为 `violation` 的函数级硬错误必须同时标记 `confidence = high`。
- `category`：可选，优雅性维度分类：`level-purity`、`control-flow`、`state-gain`、`symmetry`、`role`。
- `obligation`：可选，处理义务：`refactor`（必须重构）、`prove_minimalism`（建议重构或登记稀缺举证）、`critic_review`（已附 `bits:prove`，待独立 Critic 核验）、`ratchet_improving`（diff 棘轮命中，指标较基线下降）。
- `improving`：可选布尔值，为 `true` 时表示该诊断命中了 diff `improving` 棘轮。
- `explanation`：可选的说明文本，来自 `bits:prove` 或兼容的 `bits:explain(BCS003)`；warning 缺失说明时文本报告输出温和提示，不强制要求补注释。
- `unsupported`：未实现适配的文件列表。`report` 返回 0 并展示 `status = unsupported`；`check` 返回 2，防止将未覆盖误认为通过。
- `summary.warnings`、`summary.unsupported_files`、`summary.proves` 和 `summary.improving` 分别统计 warning、unsupported 文件、处于 `critic_review` 的举证数和命中棘轮的诊断数。

## 3.2 配置文件

```toml
# bits.toml
exclude = ["**/generated/**", "vendor/**"]
io_sources = ["Client.request", "db.query", "my_io::*"]
```

`io_sources` 为追加的 IO 调用 glob 模式，在内置 IO 源头（`open`、`os.*`、`subprocess.*`、`httpx.*`、`fetch`、`std::fs::*` 等）之上生效，供 `BCS017`（`bits:spec(pure)` 污点传播）判定直接 IO 源。

## 4. 扫描流程

```text
发现文件
  → 路径规范化与去重
  → 词法扫描
  → 格式化检查
  → AST 检查
  → 跨文件依赖检查（diff 按需补充只读上下文）
  → 行级豁免
  → diff 新增行过滤
  → baseline 过滤
  → 汇总违规
  → 按固定顺序输出
```

单文件规则不得依赖扫描顺序；串行与并行结果必须一致。扫描不得访问网络或执行被扫描项目代码。解析失败属于扫描错误，不得静默跳过，错误信息包含可用的行列位置。TypeScript 动态 import 泛型和动态类型引用使用文档说明的内存兼容处理，不修改源文件。LLM 不参与规则判定。

输出按 `file → start.line → start.column → rule_id` 排序。

## 5. 行级豁免与稀缺举证

```text
bits:allow(BCS004): 框架回调签名固定
bits:allow(BCS010): FFI 要求保留原始符号名
bits:prove(BCS003, test=test_lock): cursor 与 checksum 共享锁；拆分导致提前解锁
```

- `bits:allow` 仅用于外部硬契约，必须包含规则号和理由，只作用于当前行或下一行（`BCS004` 覆盖完整签名）。
- `bits:prove` 必须使用 `;` / `；` 分隔的两段式说明，可附 `test=<name>`；只作用于 warning 并标记 `obligation = critic_review`，**不能豁免硬错误**。
- 以下情况均产生 `BCS015` 硬错误：
  1. 缺少规则号、理由或两段式结构的注释；
  2. **未压中任何告警的 `bits:allow`（墓碑注释）**；
  3. **未命中活跃 warning 的 `bits:prove`（墓碑注释）**；
  4. 使用 `bits:prove` 试图豁免 `severity = violation` 的硬错误；
  5. `--diff` 模式下新增 `bits:prove` 超过 **1** 条配额。
- 禁止文件级、目录级 blanket suppression。

## 6. 存量 baseline

```bash
bits style report . --output bits-baseline.json
bits style check . --baseline bits-baseline.json
```

baseline 只隐藏规则号、文件、消息和实际值完全匹配的违规，行列变化不会单独产生新违规。每条基线记录只消费一次，重复违规按记录数量匹配，新增的同指纹违规不会被已有的一条记录吞掉。新文件不参与 baseline，必须全量通过。`summary.suppressed_violations` 记录被隐藏的存量违规数量。

## 7. 文本输出

```text
<file>:<line>:<column>: <rule_id>: <message> actual=<actual> expected=<expected> fixable=<yes|no>
```

摘要包含扫描文件数、违规数、可修复数、baseline 抑制数、扫描错误数和最终结果。机器格式下，人类可读日志只能写入 stderr。

## 8. JSON 报告

```json
{
  "protocol": "bits.style.scan/v2",
  "tool": { "name": "bits", "version": "0.4.1" },
  "style": { "name": "Bits’s Coding Style", "version": "0.4" },
  "status": "warning",
  "coverage": {
    "executed": ["BCS001", "BCS002"],
    "not_implemented": [],
    "partial": ["BCS011"],
    "not_applicable": []
  },
  "results": [
    {
      "rule_id": "BCS003",
      "file": "src/parser.ts",
      "range": {
        "start": { "line": 81, "column": 9 },
        "end": { "line": 81, "column": 11 }
      },
      "message": "mutually exclusive branches repeat the same leading call",
      "actual": "await harness.check()",
      "expected": "hoist_shared_setup_or_bind_handler",
      "fixable": false,
      "severity": "warning",
      "confidence": "high",
      "category": "symmetry",
      "obligation": "critic_review",
      "explanation": "check depends on branch timing; hoisting changes init order [test=test_init_order]"
    }
  ],
  "scan_errors": [],
  "unsupported": [],
  "summary": {
    "files_discovered": 42,
    "files_scanned": 42,
    "violations": 0,
    "warnings": 1,
    "fixable_violations": 0,
    "scan_errors": 0,
    "files_fixed": 0,
    "violations_fixed": 0,
    "suppressed_violations": 0,
    "unsupported_files": 0,
    "proves": 1,
    "improving": 0
  }
}
```

必填字段包括 `protocol`、`status`、`rule_id`、`file`、`range`、`actual`、`expected`、`fixable`、`scan_errors` 和 `coverage`。路径为相对扫描根目录的 POSIX 风格路径，行列号从 1 开始。

## 9. JSONL

第一行输出 run 元数据和 coverage，随后每条违规或扫描错误占一行，最后一行输出包含 `suppressed_violations` 的 summary：

```json
{"type":"summary","status":"warning","files_scanned":42,"violations":0,"warnings":1,"scan_errors":0,"suppressed_violations":0,"unsupported_files":0}
```

## 10. 修复

```text
原始文件 → 安全修复 → 写入 → 完整重扫 → 输出剩余违规
```

允许自动修复 Profile 缩进、行尾空白和文件末尾换行。不得重命名、删除代码、拆分函数、修改依赖或改变控制流。字符串内容不得改变。diff 模式只修改新增行；只有最后一行属于新增行时才允许补文件末尾换行。

`fix` 的结果包含修复文件数、修复违规数及重扫后的剩余违规。

## 11. 退出码

| 退出码 | 含义 |
|---|---|
| 0 | 扫描完成且无违规；或 report 成功生成报告 |
| 1 | 扫描完成，存在至少一条违规 |
| 2 | 扫描未完成，包括参数、配置、baseline、文件读取、解析错误或存在 unsupported 文件 |

`check` 和 `report` 必须使用同一扫描结果；`fix` 必须以修复后的重新扫描结果作为最终输出。
