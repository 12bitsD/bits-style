# Language Support

本矩阵是 V0.6 扫描器能力的公开边界。`STYLE.md` 中的规则定义不因此缩减；L2 品味审查（四个优雅维度）由 Agent Skill 执行，不在静态扫描覆盖范围内。

- **supported**：对应语法有通过和失败测试，实现该规则定义的确定性检查。
- **partial**：只实现列出的保守子集，未实现部分不得被理解为通过。
- **not_applicable**：语言没有对应语法。
- **not_implemented**：尚未实现；报告不会将其列为已执行。

## 总表

| 规则 | C / C++ | Python | JS / TS / TSX | Rust | Go | 当前判定边界 |
|---|---|---|---|---|---|---|
| `BCS001` | partial | partial | partial | partial | partial | 禁止混用及错误缩进字符，处理行尾空白和末尾换行；Go 缩进宽度交 `gofmt` |
| `BCS002` | partial | partial | partial | partial | not_applicable | 默认 80 列（硬错误）；自动豁免多行字符串/模板/docstring 与 URL；Go 行宽交 `gofmt` 与项目约定 |
| `BCS003` | partial | partial | partial | partial | partial | 函数行数（>48）、全部局部变量（>10）与非测试文件净行数（>300）为 warning（测试与业务一致执行）；嵌套（>3）和圈复杂度（>10）为 violation 且附带 Top `hotspots`；`cognitive_v1 > 15` 输出 shadow warning |
| `BCS004` | partial | partial | partial | partial | partial | 参数数量为 warning（Python 自动剔除 `self`/`cls` 与裸 `*`，keyword-only 放宽至 6、DI `__init__` 至 8；Go 展开 `(a, b int)`）；仅无默认值的位置布尔参数判为行为开关；返回 `None`/`void` 且原地修改 ≥2 个参数容器的多出参函数报 warning；Go 检查 `context.Context` 首参（warning） |
| `BCS005` | partial | partial | partial | partial | partial | 冗余 else；C 限制 goto，C++/Go 禁止 goto；无 `elif/else` 的双层嵌套 `if` 报可折叠 warning |
| `BCS006` | partial | partial | partial | partial | partial | 可变容器或调用结果的链式赋值为硬错误，纯标量链式赋值为 warning；嵌套三元（含括号包裹）和双重否定为硬错误；Python/C/JS 断言条件含 `:=`/`=`/`++` 等显式副作用报硬错误；Rust `unsafe { x };` 尾分号不误报 |
| `BCS007` | supported | not_applicable | supported | partial | not_applicable | 所有控制块必须加大括号；C/C++ 函数 `{` 下一行，JS/TS 同行；Rust 多行闭包/let-else 交 rustfmt；Go 强制大括号由语法保证 |
| `BCS008` | partial | not_applicable | partial | partial | not_applicable | C/C++ case 对齐 switch；JS/TS 使用一级缩进；Rust match 交 rustfmt；Go case 缩进由 gofmt 强制 |
| `BCS009` | partial | partial | TSX 低置信 warning；其他 partial | partial | not_applicable | 运算符空白按 AST 左右节点判断；多行 closing ) 不报 padding；JSX 与 Go 交给各自 formatter |
| `BCS010` | partial | partial | partial | partial | partial | 命名为 warning；Go 接受 MixedCase 及 `Test*`/`Benchmark*`/`Example*`/`Fuzz*`；Pydantic/JSON 字段、HTTP 回调和 React/memo 组件自动豁免 |
| `BCS011` | partial | partial | partial | partial | partial | 可变全局、真实参数重赋值与 Python `nonlocal` 为硬错误（`global` 为 warning）；环境变量读取降为边界 warning（自动放行 `main`/`run_cli`/`__main__` 及已注入 `env` 参数的回退）；Go 跳过参数重赋值，检查 struct 禁存 `context.Context`（warning，需真实 `"context"` 导入） |
| `BCS012` | partial | not_applicable | not_applicable | not_applicable | not_applicable | C/C++ 宏、inline 和函数内条件编译；宏的全部调用方语义不完整 |
| `BCS013` | C++ partial；C 异常语法 N/A | partial | partial | 委托 Clippy | 委托 L0/L2 | 空 catch 与宽泛 `except`/`except Exception`/`catch (...)` 仅写裸 `return`/`continue`/`break` 为硬错误；伪造成功（`true`/`0`/`{"ok":true}`/隐式空 `return`）、边界转换、冗余重抛为 warning；解析/探针返回 `false`/`null`/`None` 不误报；Rust `unwrap/expect/panic` 交 Clippy `restriction`，Go 显式 `error` 路径交编译器/L2 |
| `BCS014` | partial | partial | partial | partial | partial | 运行时依赖环、不可达代码为硬错误；Python 未跨文件导入的私有 `_fn` 与 JS/TS ES Module 未导出的孤儿函数/`const` 报死代码 warning；Rust/Go unused import 交 `rustc`/Go 编译器；Rust 通配符放行 `pub(...)` 导出、prelude、枚举变体、测试模块与同文件内联子模块；Go 非测试 dot import 报 warning；多行说明块与 godoc 示例不算旧代码 |
| `BCS015` | supported | supported | supported | supported | supported | TODO 格式、`allow`/`prove` 格式、墓碑注释硬错误、`test=X` AST 唯一性校验、diff `prove` 配额（≤1）与 diff 新增 `explain` 禁令、editor modeline |
| `BCS016` | partial | partial | partial | partial | partial | 穷尽分支头部重复同一前置调用报 warning；相邻同体分支报 warning（跳过 `typeof`/`isinstance`/`:=` 收窄） |
| `BCS017` | partial | partial | partial | partial | partial | `bits:spec(pure)` 触碰内置（含 Go `fmt.Print*`/`http.*`/`os/exec` 等）或 `io_sources` 配置的 IO 源报硬错误；无法静态确认接收者的方法调用默认不报 |

## 文件扩展名

| Language | Extensions |
|---|---|
| C | `.c`, `.h` |
| C++ | `.cc`, `.cpp`, `.cxx`, `.hh`, `.hpp`, `.hxx` |
| Python | `.py`, `.pyi` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| TypeScript | `.ts`, `.mts`, `.cts` |
| TSX | `.tsx` |
| Rust | `.rs` |
| Go | `.go` |

Rust 与 Go 均为 partial 支持，已接入 Tree-sitter，详见 [`rust-support.md`](rust-support.md) 与 [`profiles/go.md`](../profiles/go.md)。未适配源码（`.java`、`.kt`、`.swift`、`.rb`、`.php`、`.cs`、`.scala`、`.lua`、`.zig`、`.ex`/`.exs`、`.hs`、`.ml`、`.sh`/`.bash`/`.zsh`）会如实记录在报告的 `unsupported` 列表：目录扫描和 `--diff` 默认跳过（`check` 退出码 0），传入 `--strict-unsupported` 或命令行直接点名单个未支持文件时 `check` 返回 2。其他文件类型（如 Markdown、JSON、图片）不参与扫描。

## TypeScript 动态 import 兼容

当前 Tree-sitter 对 `importOriginal<typeof import(...)>(...)` 和 `import('...').SomeType` 两类合法 TypeScript 形式存在解析差异。扫描器在内存中等长屏蔽泛型参数、以 `any` 替换动态类型引用，不修改磁盘源码，不影响违规行列号。运行时动态 import 不做该处理。该兼容路径有真实项目回归测试，仍属于 partial 支持。

## 依赖图边界

- JavaScript/TypeScript：解析相对路径 import，并尝试常见扩展名和 index 文件。
- Python：只对可解析到已发现文件的相对模块路径建立边。
- C/C++：相对头文件 include 可形成依赖边。
- 外部包、构建条件、宏生成 include 和动态 import 不参与完整依赖证明。

## 报告覆盖

`coverage.executed` 表示规则至少在一个成功解析的语言适配器中执行；`coverage.partial` 表示这里列出的保守子集；`coverage.not_applicable` 表示不适用语法；`coverage.not_implemented` 表示没有任何已执行实现。

因此，`status = passed` 的准确含义是：**当前语言适配器实际执行的检查均通过**，不是对 REVIEW 规则或所有无法静态判定的语义作出证明。
