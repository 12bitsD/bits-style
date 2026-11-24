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

## 宗旨与原则

> **正确 > 清晰 > 可修改 > 一致 > 简短**

Bits 的底层需求是：**写出简单到不容易出错的代码和逻辑，并让这种简单能够被低成本验证。**

| 优先级 | 原则 | 发生冲突时的取舍 |
|---|---|---|
| 1. 正确 | 行为符合契约，错误路径、资源释放和并发边界明确 | 宁可多写显式处理，也不吞错、伪造成功或依赖隐式假设 |
| 2. 清晰 | 不调试就能读懂主流程、输入输出和副作用 | 不用 one-liner、双重否定或聪明技巧压缩阅读成本 |
| 3. 可修改 | 状态、依赖和所有权显式，局部修改不牵动全局 | 拒绝可变全局、隐式上下文和为假想需求提前抽象 |
| 4. 一致 | 同一语言遵循生态惯例和项目统一约定 | 格式交给官方 formatter，外部 API/FFI/框架契约不被强行改名 |
| 5. 简短 | 删除无用状态、分支、依赖和包装层 | 永远排在最后，不靠拆散内聚逻辑或删除必要注释凑行数 |

围绕这个优先级，Bits 坚持四条底线：

- **数据结构优先于分支修补**：先让类型、状态、默认值和表结构表达约束，让特殊情况消失，而不是继续叠加 `if`。
- **内聚优先于函数行数**：按职责和阶段拆分，不按行数机械切段。深模块、小接口优于碎函数、上下文包装对象和纯转发层。
- **工具揭示复杂性，不制造复杂性**：基础语法、格式和编译问题交给语言官方工具链与真实测试；Bits 只补充它们难以表达的复杂逻辑与结构风险，绝不要求为了清零报告而重构。
- **验证成本低，边界诚实**：能确定性判断的才做硬门禁；不确定项输出 `warning` 或留给 `REVIEW`；`partial` 和 `unsupported` 必须显式展示，绝不当作全量通过。

## 迭代原则与方向

### 迭代原则

1. **先可信，再全面**：消除误报和 formatter 冲突优先于新增规则。真实项目反馈必须沉淀为正反例回归测试。
2. **先增量，再全库**：优先完善 `--diff --commit`、baseline 和跨文件只读上下文，让存量项目不必全仓重写也能建立门禁。
3. **语义重于格式**：新检查聚焦错误处理、状态与所有权、资源生命周期、并发、依赖环和真实控制流复杂度；格式只保留最小兜底。
4. **跨语言同目标，各语言各习惯**：共享 `AUTO-CORE` 语义，各语言保留官方 formatter、命名惯例和外部数据契约。

### 当前方向

- **扫描可信度**：持续收敛 TSX/JSX、Python 类型与 Pydantic、Rust trait/macro 以及跨文件引用的误报，保证主流 formatter 格式化后的合法代码零冲突。
- **增量门禁与性能**：增强 diff 与指定 commit 的变更关联、跨文件依赖上下文和并行扫描，在保持输出确定性的前提下降低大仓扫描成本。
- **语言适配深度**：将 Rust 从 partial 向常用语义稳步推进；落地 Go 扫描适配，重点覆盖 `error`、`defer`、`context`/goroutine 生命周期和共享状态，并与 `gofmt`、`go vet`、`staticcheck`、`go test -race` 明确分工。
- **Agent 原生接入**：持续打磨 [`bits-style` Skill](skills/bits-style/SKILL.md) 与项目模板，让 Agent 按 `report → 分类 → 最小修复 → 项目测试 → 重扫` 闭环工作。

新规则进入 `AUTO-CORE` 前必须同时满足：可确定性判定、误报率低、有真实项目正反例、严重级别明确，且不会诱导开发者拆散内聚逻辑或增加状态包装。做不到的规则只进入 `AUTO-PROFILE`、`warning` 或 `REVIEW`。

## 核心结构

- `AUTO-CORE`：跨语言、可机检的语义规则。
- `AUTO-PROFILE`：由语言 Profile 决定的可机检格式和写法。
- `REVIEW`：需要设计判断，不参与自动 PASS/FAIL。

C/C++ 保留 Tab 8 和函数换行大括号；Python/Rust 使用 4 空格；JavaScript/TypeScript/TSX 使用 2 空格、lowerCamelCase 和同行大括号。编译器、ABI、FFI 和框架强制签名可以行级豁免。

## Warning、React 与 Unsupported

- BCS003 的行数和局部变量输出 warning，真实嵌套和圈复杂度仍为硬错误；优先按阶段拆分，只有必须保持内聚时才使用 `bits:explain(BCS003): 具体机制；共享约束；拆分风险`（不豁免硬错误，不接受空话套话）。
- React 组件自动接受 PascalCase 和展示型布尔 props；JSX 区域交给 formatter，TSX 保守提示不阻断 CI。
- `severity = violation` 才返回 1；warning 只进入报告。`report` 展示全部结果。
- Go 等未适配文件进入 `unsupported` 列表，`check` 返回 2；Rust 已适配的部分按 `partial` 明确报告，不再形成“未覆盖却 passed”。

## 实现

`bits` 是 Rust 编写的独立 CLI，使用 Tree-sitter 解析源码，不执行被扫描项目代码，扫描过程不访问网络。

当前扫描适配：C、C++、Python、JavaScript、TypeScript、TSX、Rust。Rust 为 partial 支持，Go 目前只有规范 Profile，没有扫描适配；未适配文件会进入 `unsupported` 列表，而不是形成“无文件通过”。

**能解析一种语言不等于完整实现全部规则。** 函数接口、状态、错误处理和部分语义检查只支持文档列出的保守子集。逐规则状态见[支持矩阵](docs/language-support.md)。

## 安装

运行扫描器需要 [Rust 工具链（含 Cargo）](https://rustup.rs/)；Cargo 是 Rust 的构建与包管理器。被扫描的项目不需要使用 Rust；只阅读规范或按 Agent Skill 约束编码时，也不必安装扫描器。

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

103 个测试覆盖规则边界、Profile、diff-only、指定 commit、跨文件依赖上下文、suppression、baseline、协议输出、Schema、退出码、并行确定性和修复幂等，其中集成测试直接驱动真实 CLI。

## License

[MIT](LICENSE)，版权主体为 Bits contributors。
