# Bits’s Coding Style

[![CI](https://github.com/12bitsD/bits-style/actions/workflows/ci.yml/badge.svg)](https://github.com/12bitsD/bits-style/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

一套面向人与 AI Agent 的编码规范、静态扫描器与品味审查协议。

大多数风格工具要么纠结空格和行宽（官方 formatter 早已解决），要么给函数设一个行数上限。后者一旦变成硬门禁，就会逼着开发者（尤其是 Agent）把内聚的逻辑拆成一堆两行碎函数，或者写一句套话注释蒙混过关——指标变绿了，代码却更难读懂和维护。

Bits 的出发点是：**优雅不是把代码切得更碎，而是让不必要的结构消失，并且让这种简单能被低成本验证。** 规范只约束代码本身，不涉及架构分层、设计模式或评审流程；能由编译器和 formatter 判定的绝不重复检查，需要上下文判断的设计品味不靠正则硬猜。

> 规范版本：**Style V0.4 / Protocol V0.4** · 扫描器版本：**v0.5.0**
>
> 文档导航：[编码规范（BCS001–BCS017）](STYLE.md) · [扫描协议](PROTOCOL.md) · [Agent 接入与 Skill](docs/agent-integration.md) · [语言支持矩阵](docs/language-support.md) · [基准体检报告](benchmarks/BASELINE.md) · [Profiles](docs/profiles.md) · [Report Schema](schemas/report.schema.json)

## 取舍顺序

> **正确 > 清晰 > 可修改 > 一致 > 简短**

所有规则冲突时，都按这个顺序裁决：

| 优先级 | 原则 | 发生冲突时的取舍 |
|---|---|---|
| 1. 正确 | 行为符合契约，错误路径、资源释放和并发边界明确 | 宁可多写显式处理，也不吞错、伪造成功或依赖隐式假设 |
| 2. 清晰 | 不调试就能读懂主流程、输入输出和副作用 | 不用 one-liner、双重否定或聪明技巧压缩阅读成本 |
| 3. 可修改 | 状态、依赖和所有权显式，局部修改不牵动全局 | 拒绝可变全局、隐式上下文和为假想需求提前抽象 |
| 4. 一致 | 同一语言遵循生态惯例和项目统一约定 | 格式交给官方 formatter，外部 API/FFI/框架契约不被强行改名 |
| 5. 简短 | 删除无用状态、分支、依赖和包装层 | 永远排在最后，不靠拆散内聚逻辑或删除必要注释凑行数 |

## 五条设计原理

1. **读优先于写，删优先于增**
   代码的主要成本在读和改，不在写。判断一段结构该不该保留，先问“删掉它会损失什么决策信息”，而不是“它有多少行”。优先让数据结构、零值和表结构表达约束，让特殊情况自然消失；深模块、小接口永远优于碎函数、上下文包装对象和纯转发层。

2. **宁漏勿错（Precision > Recall）**
   静态工具的信任极其脆弱：几次把正常代码判错的硬告警，就足以让开发者和 Agent 开始批量打豁免，工具随之失效。因此只有确定性结构矛盾才做硬门禁；拿不准的一律降为 warning 或交给审查层。扫描器的误报由扫描器自己修，绝不让用户靠打 `bits:allow` 消化。

3. **三层各守边界，不让 AST 猜语义**
   不同层次的问题需要不同的判定能力，混在一层必然导致误报或漏判：
   - **L0 卫生层（官方工具链）**：语法、格式、未使用变量和语言特有坑，交给 formatter、编译器、Ruff、Clippy、go vet 等。Bits 不重复造轮子，也不自动改写语义。
   - **L1 拓扑层（`bits` AST）**：只抓不依赖业务含义的客观结构事实，如真实嵌套深度、断言显式副作用、互斥分支头部重复前置调用、`bits:spec(pure)` 触碰 IO。硬错误不能用注释豁免；warning 只负责圈定热区，不要求补注释。
   - **L2 品味层（Skill + 独立 Critic）**：用**层级纯度、控制流平直度、信息增益、对称性**四个维度，只审查 Diff 改动函数与 L1 warning 热区，识别镜像下游校验、过度防御解析和层级混杂。无问题保持沉默。

4. **以“修复后结果”论英雄（反刷分）**
   凡是允许靠写注释通关的规则，Agent 永远会选阻力最小的路：写一句无懈可击的套话。因此 Bits 把外部硬契约豁免（`bits:allow`）与稀缺技术债举证（`bits:prove`）严格分开：举证必须给出具体风险与真实测试锚点、限制单次 diff 配额，且作者不能自审，必须交由独立 Critic 或人核验。一条规则好不好，不看抓出多少告警，而看修完之后代码是不是真的更干净。

5. **从真实痛点生长，用顶级代码校准**
   不拍脑袋造教条，只从真实 Review 踩坑中提炼规则。新规则进入 `AUTO-CORE` 前必须过三关：真实反面案例 `before/after` 验证、三语言 9 个锁定 commit 开源标杆库盲测（`bits bench`）、Agent 修复对抗测试；新复杂度指标先看全量分位数分布再定阈值。实测噪声超线的规则，果断收窄、降级或删除。

## 演进方向

- **先增量，再全库**：通过 `--diff --commit`、baseline 与 `improving` 棘轮（复杂度严格下降即放行），让存量仓库不必一次性重写，也能建立只进不退的门禁。
- **扫描可信度**：持续收敛 TSX/JSX、Python 类型、Rust trait/macro 与跨文件引用的误报，保持与主流 formatter 格式化结果零冲突。
- **语言适配深度**：Python、Rust、Go 均已接入 partial 扫描；在保持 L0/L1 去重的前提下，稳步推进 Rust 常用语义与 Go 资源/并发生命周期的确定性检查。
- **Agent 原生闭环**：持续打磨 [`bits-style` Skill](skills/bits-style/SKILL.md) 与项目模板，让 Agent 按 `report → 分类 → 最小修复 → 真实测试 → 重扫` 闭环工作。

## Warning、举证与 Unsupported

- **Warning 不要求补注释**。处理顺序固定为：删冗余层 → 换成表/Schema/handler 绑定 → 按职责拆分 → 最后才登记稀缺举证。
- `bits:allow(RULE): 理由` 仅用于外部硬契约；`bits:prove(RULE[, test=name]): 最简理由；具体风险` 仅用于 warning，**每次 diff 最多新增 1 条**，不能豁免硬错误，`test=name` 必须经 AST 唯一解析到真实测试，且必须由独立 Critic/人核验。未命中告警的 `allow`/`prove` 及 diff 新增的 legacy `bits:explain` 直接报 `BCS015` 硬错误。
- **L1 结构品味**：穷尽分支头部重复前置调用与相邻同体分支（`BCS016`）、可折叠双层 `if`（`BCS005`）、无效重抛（`BCS013`）、断言显式副作用硬错误（`BCS006`）、`bits:spec(pure)` IO 污点传播（`BCS017`）。
- **Sonar 认知复杂度 `cognitive_v1`**：在 Python（`click`/`httpx`/`rich`）、Rust（`serde`/`ripgrep`/`tokio`）、Go（`cobra`/`zap`/`go-tools`）9 库共 10,658 个函数上实测 P50=0、P95=7、P99=20；`> 15` 仅占 1.67%，继续作为 shadow warning。圈复杂度 `> 10` 的 161 个函数中 77.6% 认知复杂度也 `> 15`，仅 2.5% 为低认知扁平分发，暂维持现有硬门禁与 shadow 分工。
- **Python L0/L1 去重**：`profiles/python-ruff.toml` 仅开 `F`、`B006`、`PLR1714`、`PIE790` 且禁止自动改写；结构规则统一归 Bits L1。
- **`improving` 棘轮**：`--diff` 下按 `(函数名, 同名序号)` 匹配基线函数，嵌套、圈复杂度或认知复杂度严格下降时标记 `improving = true` 并降为 warning 放行；新函数和恶化改动仍硬拦截。
- React 组件自动接受 PascalCase 和展示型布尔 props；JSX 区域交给 formatter，TSX 保守提示不阻断 CI。
- `severity = violation` 才返回 1；warning 只进入报告。`report` 展示全部结果。
- Rust 与 Go 均为 partial 支持，格式、unused import 与 `unwrap/panic` 等交由 `rustfmt`/`rustc`/Clippy 与 `gofmt`/Go 工具链；Java、Zig、Ruby 等未适配源码进入 `unsupported` 列表，`check` 返回 2，避免“未覆盖却 passed”。

## 实现

`bits` 是 Rust 编写的独立 CLI，使用 Tree-sitter 解析源码，不执行被扫描项目代码，扫描过程不访问网络。

当前扫描适配：C、C++、Python、JavaScript、TypeScript、TSX、Rust、Go。Rust 与 Go 均为 partial 支持；Java、Kotlin、Swift、Ruby、PHP、C#、Scala、Lua、Zig、Elixir、Haskell、OCaml、Shell 等未适配源码会进入 `unsupported` 列表，而不是形成“无文件通过”。

**能解析一种语言不等于完整实现全部规则。** 函数接口、状态、错误处理和部分语义检查只支持文档列出的保守子集。逐规则状态见[支持矩阵](docs/language-support.md)。

## 安装

运行扫描器需要 [Rust 工具链（含 Cargo）](https://rustup.rs/)；Cargo 是 Rust 的构建与包管理器。被扫描的项目不需要使用 Rust；只阅读规范或按 Agent Skill 约束编码时，也不必安装扫描器。

```bash
cargo install --git https://github.com/12bitsD/bits-style --tag v0.5.0 --locked
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
bits bench --offline
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
io_sources = ["Client.request", "db.query"]
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

127 个测试覆盖规则边界、Profile（含 Rust/Go 校准）、diff-only、指定 commit、跨文件依赖上下文、suppression、baseline、协议输出、Schema、退出码、并行确定性和修复幂等，其中集成测试直接驱动真实 CLI；`bits bench` 支持三语言 9 个锁定 commit 开源标杆库的误报与复杂度分布评测。

## License

[MIT](LICENSE)，版权主体为 Bits contributors。
