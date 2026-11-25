# Bits’s Coding Style

[![CI](https://github.com/12bitsD/bits-style/actions/workflows/ci.yml/badge.svg)](https://github.com/12bitsD/bits-style/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Bits 是一套编码规范，外加一个用 Rust 写的命令行扫描器 `bits`。目标是让人和 AI Agent 写出简单、好读、好改的代码，并且能用工具低成本地检查出来。

## 为什么做这个

AI 生成代码很快，但默认容易多写：服务端明明会报错，本地还要再写一遍参数校验；一个简单的命令分发，也要拆出好几个转发函数和上下文对象。代码生成得越多，后面读代码、改代码的人负担越重。

官方 formatter 和编译器能管好格式和语法，但管不了多余的逻辑。如果只拿“函数不超过 48 行”卡 CI，Agent 就会靠拆碎函数、写豁免注释来过关，代码反而更难读。

Bits 把检查分成两部分：
- 能客观判断的结构问题，由 `bits` 扫描器直接检查，比如嵌套太深、在 `assert` 里赋值、声明了纯函数却调用网络请求。
- 需要结合上下文判断的设计问题，比如这层校验是不是多余、是不是在重复解析下游错误，交给配套的 [Agent Skill](skills/bits-style/SKILL.md) 按清单复核，不靠正则硬猜。

规范只约束代码本身，不涉及架构分层、设计模式或评审流程。

## 快速上手

安装需要本机有 [Rust 工具链（Cargo）](https://rustup.rs/)，被扫描的项目不限语言。只阅读规范或使用 Skill 时，不需要安装扫描器。

```bash
cargo install --git https://github.com/12bitsD/bits-style --tag v0.5.0 --locked
```

常用命令：

```bash
bits style check src                 # 检查目录，有硬错误时返回 1
bits style check --diff              # 只检查本次改动（默认对比 HEAD）
bits style report src --format json  # 输出完整 JSON 报告（含警告）
bits style fix src                   # 只修缩进、行尾空白和末尾换行
```

举个例子，下面这段代码有两个问题：三个分支开头都在重复调用 `harness.check()`；`assert` 里混入了赋值（Python 加 `-O` 运行时断言会被删掉，程序行为就变了）。

```python
async def run_command(harness, args):
    if args.cmd == "tools":
        await harness.check()
        return await harness.tools.list()
    elif args.cmd == "skills":
        await harness.check()
        return harness.skills.list()
    else:
        await harness.check()
        return None

def check_count(items):
    assert (count := len(items)) > 0
    return count
```

运行 `bits style check cli.py`，输出节选：

```text
cli.py:2:5: BCS016: mutually exclusive branches repeat the same leading call ... severity=warning
  hint: prefer refactoring; if unavoidable, register bits:prove(...) (budget: 1 per change)
cli.py:13:5: BCS006: assertion condition must not contain explicit side effects ... severity=violation
Result: FAIL
```

## 查什么，不查什么

**1. 官方工具能查的，Bits 不重复查**
- 格式与空白：交给 rustfmt、gofmt、Prettier、ClangFormat。
- 未使用变量、未使用导入、类型错误：交给编译器、Ruff、Mypy、TSC、go vet。
- Rust 的 `unwrap`、`expect`、`panic`：交给 Clippy 按项目需要开启。

**2. `bits` 扫描器只查客观结构**
- **硬错误**（返回 1，不能写注释豁免）：控制流嵌套超过 3 层、圈复杂度超过 10、嵌套三元、双重否定、空 `catch`、`assert` 条件里带 `:=`/`=`/`++`、可变全局变量、给参数重新赋值、模块依赖环，以及标了 `bits:spec(pure)` 的函数调用 IO。
- **警告**（不影响退出码，也不要求补注释）：函数超过 48 行或局部变量超过 10 个、参数超过 4 个、命名不规范、可以合并的双层 `if`、相邻分支内容完全一样、穷尽分支开头重复同一个调用、Go 的 `context.Context` 没放在第一个参数或存进了 struct。
- 同时计算 Sonar 的**认知复杂度**：嵌套越深扣分越多，扁平的 `switch/match` 扣分少。在 9 个开源库、10,658 个函数上实测，超过 15 分的函数占 1.67%，目前只作为参考警告。

**3. 设计判断交给 Agent Skill 或人复核**
只看本次改动的函数和扫描器报了警告的地方，问四个问题：
- **层级**：函数是不是一会儿在做流程编排，一会儿又在手拼底层字段？
- **控制流**：能不能用查表、提前返回或直接绑定处理函数，代替一长串 `if/else`？
- **信息**：是不是在替下游重复做校验，或者解析了根本没人用的字段？
- **对称性**：同样的事有没有两种写法？`assert` 是不是只用来检查内部不变量，而不是拦截外部输入？

完整规则见 [STYLE.md](STYLE.md)，各语言支持范围见[支持矩阵](docs/language-support.md)。

## 冲突时的取舍顺序

> **正确 > 清晰 > 可修改 > 一致 > 简短**

| 优先级 | 原则 | 发生冲突时怎么做 |
|---|---|---|
| 1. 正确 | 行为符合契约，错误路径、资源释放和并发边界明确 | 宁可多写显式处理，也不吞错、伪造成功或依赖隐式假设 |
| 2. 清晰 | 不调试就能读懂主流程、输入输出和副作用 | 不用 one-liner、双重否定或花哨技巧省行数 |
| 3. 可修改 | 状态、依赖和所有权显式，局部修改不牵动全局 | 不用可变全局和隐式上下文，不为假想需求提前抽象 |
| 4. 一致 | 遵循语言生态惯例和项目统一约定 | 格式交给 formatter，外部 API/FFI/框架要求的命名不强行改 |
| 5. 简短 | 删掉没用的状态、分支、依赖和包装层 | 排在最后，不靠拆散内聚逻辑或删必要注释凑行数 |

## 豁免与举证

豁免只认行级注释，作用于当前行或下一行，不支持按文件或目录统一关闭。

```python
# bits:allow(BCS010): FFI 要求保留原始符号名
# bits:prove(BCS003, test=test_step_atomicity): 持锁期间完成状态迁移；拆成两个函数会提前释放锁
```

- `bits:allow(RULE): 理由`：只用于外部硬约束，比如 FFI 符号名、公开 JSON 字段。
- `bits:prove(RULE[, test=测试名]): 最简理由；具体风险`：只用于警告。为了防止 Agent 写套话过关，每次 diff 最多新增 1 条；填了 `test=` 必须能对上真实的测试函数；写代码的 Agent 不能自己审自己，必须由另一个 Critic Agent 或人确认。
- 代码改完后，如果注释下面已经没有对应问题，`bits` 会报错要求删掉注释，避免留下过期说明。
- 旧版的 `bits:explain` 在全量扫描时仍兼容，但在 `--diff` 中新增会直接报错。

## 在老项目里渐进启用

接入 Bits 不需要一次性重构整个仓库：

```bash
bits style check --diff --commit HEAD~3
bits style report . --output bits-baseline.json
bits style check . --baseline bits-baseline.json
```

- `--diff` 只报告新增或修改行相关的问题，同时会按需读取关联的未改动文件来检查依赖环。
- `--baseline` 隐藏已有的存量问题；新文件和新增问题仍会被拦下。
- `--diff` 模式下，如果一个老函数的嵌套或复杂度比改动前下降了，即使仍然超标也会降级为警告（标记为 `improving`）；新函数和越改越复杂的函数仍按硬错误处理。

## 扫描与配置

当前支持：C、C++、Python、JavaScript、TypeScript、TSX、Rust（部分规则）、Go（部分规则）。遇到尚未支持的源码文件（如 `.java`、`.zig`、`.rb`），会列在报告的 `unsupported` 字段中，`check` 返回 2，不会当作检查通过。

扫描默认遵循 `.gitignore`，跳过 `.git`、依赖目录、构建目录和常见生成文件。扫描只解析语法树，不执行项目代码，也不访问网络。

```bash
bits style check . --all                     # 全量扫描
bits style check . --changed                 # 扫描工作区改动的文件（按整文件检查）
bits style check . --staged                  # 扫描已暂存的文件（按整文件检查）
bits style report src --format sarif         # 支持 text / json / jsonl / sarif
bits style report src --jobs 8 --include-passed
```

配置文件 `bits.toml`（用 `--config bits.toml` 指定）：

```toml
exclude = ["**/generated/**", "vendor/**"]
io_sources = ["Client.request", "db.query"]  # 补充项目自己的 IO 调用，供 bits:spec(pure) 检查
```

| 退出码 | 含义 |
|---|---|
| `0` | 无硬错误；或 `report` 成功生成报告 |
| `1` | 存在硬错误（`severity = violation`） |
| `2` | 参数、配置、读取或解析错误，或存在未支持的源码文件 |

性能与自动修复：
- 默认按 CPU 核数并行（上限 8），输出顺序固定，串行与并行结果一致。详见 [PERFORMANCE.md](docs/PERFORMANCE.md)。
- `bits style fix` 只处理缩进、行尾空白和文件末尾换行，不改字符串内容，也不会重命名、删代码、拆函数或改控制流。重复执行不会产生新改动。

## 规则是怎么定出来的

规则只从实际代码评审遇到的问题里提炼。一条新规则进入扫描器前，先在 `corpus/bad/` 的真实反面案例上验证（修改前命中、修改后消除），再用 `bits bench` 在 9 个锁定 commit 的开源库上检查误报：

- Python：`click`、`httpx`、`rich`
- Rust：`serde`、`ripgrep`、`tokio`
- Go：`cobra`、`zap`、`go-tools`

硬错误要求零误报；新复杂度指标先看全量分布再定阈值。误报压不下来的规则会收窄条件、降为警告或删掉，不让用户靠打豁免消化。实测数据见 [benchmarks/BASELINE.md](benchmarks/BASELINE.md)。

## 文档与开发

- [STYLE.md](STYLE.md)：编码规范与评审清单（Style V0.4）
- [PROTOCOL.md](PROTOCOL.md)：扫描输出与豁免协议（Protocol V0.4，当前扫描器 v0.5.0）
- [docs/agent-integration.md](docs/agent-integration.md)：在项目与 CI 中接入 Agent Skill
- [docs/language-support.md](docs/language-support.md)：逐语言、逐规则支持矩阵
- [docs/profiles.md](docs/profiles.md)、[profiles/go.md](profiles/go.md)、[docs/rust-support.md](docs/rust-support.md)：各语言配置与边界
- [schemas/report.schema.json](schemas/report.schema.json)：JSON 报告 Schema

本地构建与验证（当前共 127 个测试）：

```bash
cargo build --release
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

## License

[MIT](LICENSE)，版权主体为 Bits contributors。
