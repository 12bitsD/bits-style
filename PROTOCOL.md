# Bits Style Scanning Protocol V0.1

本协议只负责发现文件、扫描、自动修复和输出结果。

## 1. 命令

```bash
bits style fix [paths...] [options]
bits style check [paths...] [options]
bits style report [paths...] [options]
```

- `fix` 只执行确定不改变程序语义的修复，然后重新扫描。修复必须幂等。
- `check` 只读扫描；无违规返回 0，有违规返回 1。
- `report` 使用与 `check` 相同的扫描结果并输出结构化报告；报告成功生成时返回 0，扫描未完成返回 2。

## 2. 参数

```text
--all                 扫描指定根目录下全部支持的源文件
--changed             扫描工作区和暂存区相对 HEAD 发生变化的文件
--staged              只扫描暂存区中的文件
--format <format>     text、json、jsonl、sarif
--output <file>       写入文件；未指定时写 stdout
--jobs <n>            并行扫描数量，不改变输出顺序
--config <file>       指定扫描配置
--no-ignore           不读取 .gitignore，但仍排除 .git 和扫描器自身产物
--include-passed      report 中包含通过的检查项
```

作用域参数互斥。未指定路径和作用域时，默认 `bits style check --all .`。

文件发现遵循 `.gitignore`，排除 `.git/`、依赖目录、构建产物、明确的生成代码和目录符号链接。同一文件规范化后只扫描一次。没有匹配文件属于成功扫描。

## 3. 扫描流程

```text
发现文件
  → 路径规范化与去重
  → 词法扫描
  → 格式化检查
  → AST 检查
  → 跨文件依赖检查
  → 汇总违规
  → 按固定顺序输出
```

单文件规则不得依赖扫描顺序；串行与并行结果必须一致。扫描不得访问网络或执行被扫描项目代码。解析失败属于扫描错误，不得静默跳过。LLM 不参与规则判定。

输出按 `file → start.line → start.column → rule_id` 排序。

## 4. 文本输出

```text
<file>:<line>:<column>: <rule_id>: <message> actual=<actual> expected=<expected> fixable=<yes|no>
```

摘要包含扫描文件数、违规数、可修复数、扫描错误数和最终结果。机器格式下，人类可读日志只能写入 stderr。

## 5. JSON 报告

```json
{
  "protocol": "bits.style.scan/v1",
  "tool": { "name": "bits", "version": "0.1.0" },
  "style": { "name": "Bits’s Coding Style", "version": "0.1" },
  "status": "violation",
  "coverage": {
    "executed": ["BCS001", "BCS002"],
    "not_implemented": ["BCS011"]
  },
  "results": [
    {
      "rule_id": "BCS003",
      "file": "src/parser.ts",
      "range": {
        "start": { "line": 81, "column": 9 },
        "end": { "line": 81, "column": 11 }
      },
      "message": "control-flow nesting exceeds limit",
      "actual": 4,
      "expected": "<=3",
      "fixable": false
    }
  ],
  "scan_errors": [],
  "summary": {
    "files_discovered": 42,
    "files_scanned": 42,
    "violations": 1,
    "fixable_violations": 0,
    "scan_errors": 0,
    "files_fixed": 0,
    "violations_fixed": 0
  }
}
```

必填字段包括 `protocol`、`status`、`rule_id`、`file`、`range`、`actual`、`expected`、`fixable`、`scan_errors` 和 `coverage`。路径为相对扫描根目录的 POSIX 风格路径，行列号从 1 开始。

`coverage.executed` 列出至少对一种输入语言实际执行的规则；`coverage.not_implemented` 防止将可解析语言误解为完整规则覆盖。

## 6. JSONL

第一行：

```json
{"type":"run","protocol":"bits.style.scan/v1","style_version":"0.1"}
```

随后每条违规或扫描错误占一行：

```json
{"type":"violation","rule_id":"BCS003","file":"src/parser.ts","line":81,"column":9,"actual":4,"expected":"<=3","fixable":false}
{"type":"scan_error","file":"src/broken.ts","error":"parse_error","message":"unexpected token at line 19"}
```

最后一行：

```json
{"type":"summary","status":"violation","files_scanned":42,"violations":1,"scan_errors":0}
```

## 7. 修复

```text
原始文件 → 安全修复 → 写入 → 完整重扫 → 输出剩余违规
```

允许自动修复 Tab、缩进、行尾空白、文件末尾换行、不改变字符串内容的换行、大括号位置、case 缩进和空格。

禁止自动修复拆分函数、减少参数、降低圈复杂度、重命名、删除代码或依赖、修改 goto、条件编译、宏、控制流或错误处理。

`fix` 的结果包含修复文件数、修复违规数及重扫后的剩余违规。

## 8. 退出码

| 退出码 | 含义 |
|---|---|
| 0 | 扫描完成且无违规；或 report 成功生成报告 |
| 1 | 扫描完成，存在至少一条违规 |
| 2 | 扫描未完成，包括参数、配置、文件读取或解析错误 |

`check` 和 `report` 必须使用同一扫描结果；`fix` 必须以修复后的重新扫描结果作为最终输出。
