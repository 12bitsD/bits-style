# Bits’s Coding Style

[![CI](https://github.com/12bitsD/bits-style/actions/workflows/ci.yml/badge.svg)](https://github.com/12bitsD/bits-style/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Bits’s Coding Style V0.3 的公开规范与参考扫描器。规范只约束代码本身，不包含 MVC、设计模式或评审流程。底层目标是让代码和逻辑简单到不容易出错；基础语法和官方 lint 已覆盖的问题不重复检查。

- [Coding Style V0.3：BCS001–BCS015](STYLE.md)
- [Language Profiles](docs/profiles.md)
- [Go Profile：错误、资源、并发与抽象](profiles/go.md)
- [Rust 扫描支持边界](docs/rust-support.md)
- [Agent 集成与完整 Skill](docs/agent-integration.md)
- [Scanning Protocol V0.3](PROTOCOL.md)
- [逐语言支持矩阵](docs/language-support.md)
- [JSON Report Schema](schemas/report.schema.json)

## 核心结构

- `AUTO-CORE`：跨语言、可机检的语义规则。
- `AUTO-PROFILE`：由语言 Profile 决定的可机检格式和写法。
- `REVIEW`：需要设计判断，不参与自动 PASS/FAIL。

C/C++ 保留 Tab 8 和函数换行大括号；Python/Rust 使用 4 空格；JavaScript/TypeScript/TSX 使用 2 空格、lowerCamelCase 和同行大括号。编译器、ABI、FFI 和框架强制签名可以行级豁免。

## Warning、React 与 Unsupported

- BCS003 的行数和局部变量输出 warning，真实嵌套和圈复杂度仍为硬错误；优先按阶段拆分，只有必须保持内聚时才使用 `bits:explain(BCS003): 职责；内聚原因；拆分风险`。
- React 组件自动接受 PascalCase 和展示型布尔 props；JSX 区域交给 formatter，TSX 保守提示不阻断 CI。
- `severity = violation` 才返回 1；warning 只进入报告。`report` 展示全部结果。
- Rust/Go 等未适配文件进入 `unsupported` 列表，`check` 返回 2，不再形成“未覆盖却 passed”。

## 实现

`bits` 是 Rust 编写的独立 CLI，使用 Tree-sitter 解析源码，不执行被扫描项目代码，扫描过程不访问网络。

当前扫描适配：C、C++、Python、JavaScript、TypeScript、TSX、Rust。Rust 为 partial 支持，Go 目前只有规范 Profile，没有扫描适配；未适配文件会进入 `unsupported` 列表，而不是形成“无文件通过”。

**能解析一种语言不等于完整实现全部规则。** 函数接口、状态、错误处理和部分语义检查只支持文档列出的保守子集。逐规则状态见[支持矩阵](docs/language-support.md)。

## 安装

```bash
cargo install --git https://github.com/12bitsD/bits-style --locked
```

本地构建：

```bash
git clone https://github.com/12bitsD/bits-style.git
cd bits-style
cargo build --release
./target/release/bits --help
```

## 使用

```bash
bits style check src
bits style report src --format json --output bits-report.json
bits style fix src
```

### 扫描范围与输出

```bash
bits style check . --all
bits style check . --changed
bits style check . --staged
bits style check --diff
bits style check --diff --commit HEAD~3
bits style check --diff --commit 61a5793 src
bits style report src --format jsonl
bits style report src --format sarif
bits style report src --jobs 8 --include-passed
```

`--changed` 和 `--staged` 扫描整个文件。`--diff` 只报告新增/修改行相关的违规，默认相对 `HEAD`；`--commit` 可指定 SHA、分支、tag 或 `HEAD~n`。未跟踪文件按全文件检查，纯删除且没有新增行的文件不进入扫描。patch 无法可靠解析时返回 2，不会降级为通过。

`--diff` 会按需读取本地 import 可达的未变更文件，用于发现新增依赖环。这些上下文文件不计入扫描数量，其存量违规和解析错误也不会进入 diff 报告；这不代表全仓检查通过。

### 行级豁免

```python
# bits:allow(BCS010): FFI 要求保留原始符号名
```

豁免必须包含规则号和理由，只作用于当前行或下一行，禁止文件级或目录级 blanket suppression。

### 存量 baseline

```bash
bits style report . --output bits-baseline.json
bits style check . --baseline bits-baseline.json
```

baseline 只隐藏规则号、文件、消息和实际值完全匹配的存量违规。新文件不参与 baseline；新增违规仍然返回 1。

### 配置

```toml
# bits.toml
exclude = ["**/generated/**", "vendor/**"]
```

```bash
bits style check . --config bits.toml
```

扫描默认遵循 `.gitignore`，并排除 `.git`、依赖目录、构建目录和常见生成文件。测试代码适用同样的语义规则。

## 性能

默认 `--jobs` 使用 CPU 并行度（上限 8），输出顺序与串行一致。函数嵌套和圈复杂度合并为一次 AST 遍历，标识符计数不再为每个 import 编译正则，字符串和类型范围使用排序索引。详见 [PERFORMANCE.md](docs/PERFORMANCE.md)。

## 自动修复边界

`fix` 只处理 Profile 缩进、行尾空白和文件末尾换行，不改变字符串内容。它不会重命名、删除代码、拆分函数、修改依赖或改变控制流。diff 模式只修改新增行；文件末尾换行也只有最后一行属于新增行时才修复。修复后必须重新扫描，连续执行两次不得产生新的 diff。

## 退出码

| 退出码 | 含义 |
|---|---|
| `0` | 无违规；或 `report` 成功生成报告 |
| `1` | 扫描完成，存在违规 |
| `2` | 参数、配置、baseline、读取或解析错误 |

## 开发与验证

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

101 个测试覆盖规则边界、Profile、diff-only、指定 commit、跨文件依赖上下文、suppression、baseline、协议输出、Schema、退出码、并行确定性和修复幂等，其中集成测试直接驱动真实 CLI。

## License

[MIT](LICENSE)，版权主体为 Bits contributors。
