# Language Profiles

Profile 只决定代码形式和工具映射，不削弱 AUTO-CORE 语义规则。基础语法和语言官方工具已经覆盖的问题不重复检查；核心目标是让代码和逻辑简单到不容易出错。

| Profile | Formatter | 缩进 | 函数/变量命名 | 大括号 | 资源与错误处理 |
|---|---|---|---|---|---|
| C/C++ | clang-format | Tab 8 | `lower_snake_case` | 函数 `{` 下一行，控制块同行 | C 使用集中清理；C++ RAII，禁止 goto；非 void 返回值检查或显式丢弃 |
| Python | ruff format | 4 Space | `lower_snake_case` | 不适用 | `with`、`raise`；显式忽略使用 `_ = call()` |
| JavaScript/TypeScript/TSX | 项目既有 formatter + EditorConfig | 2 Space | `lowerCamelCase` | `{` 同行 | `try/finally`；显式忽略使用 `void call()` |
| Rust | rustfmt | 4 Space | `lower_snake_case`，枚举成员 `PascalCase` | rustfmt | `Drop`、`?`；显式忽略使用 `let _ = call()`；unwrap/expect 为 warning |
| Go | gofmt/goimports | gofmt 决定 | 导出 `PascalCase`，未导出 `lowerCamelCase` | gofmt | `defer`、`context`、显式 error 路径；禁止 goto |

## 配置

仓库提供最小配置示例：

- `profiles/cpp.clang-format`
- `profiles/python-ruff.toml`
- `profiles/javascript.editorconfig`
- `profiles/rust-rustfmt.toml`

Go 使用独立的语义规范 [`profiles/go.md`](../profiles/go.md)，不增加重复格式配置。

这些文件覆盖格式底线与 L0 卫生检查。语义与结构检查由 `bits`（L1）和各语言编译器/linter 共同完成，不能仅用 formatter 宣称全部 BCS 通过。

### L0 / L1 去重契约（以 Python 为例）

`profiles/python-ruff.toml` 只开启 Python 特有的卫生规则：`F`（Pyflakes）、`B006`（可变默认参数）、`PLR1714`（重复相等比较）、`PIE790`（多余 `pass`），并设置 `unfixable = ["ALL"]` 禁止自动改写语义。

跨语言结构规则——`SIM102`（可折叠 `if`）、`SIM114`（同体分支）、`RET505`（冗余 `else`）、`TRY201/TRY203`（无效重抛）、`FBT`（布尔陷阱）——**统一由 Bits L1 拥有**（`BCS004/005/013/016`），不在 Ruff 模板中重复开启，保证所有语言共享同一份 JSON 报告与 `bits:prove` 协议，且同一行代码不会双重告警。

## Python 数据契约

- `Annotated`、`Literal`、`Callable` 和其他明确类型表达式按类型别名处理，不计为可变全局状态。
- 带类型注解的普通变量不自动视为类型别名；类型别名使用 `PascalCase`，私有前缀允许下划线。
- Pydantic/JSON 数据字段保留外部契约命名；属性更新不算参数重赋值。
- 模板插值中的标识符计为真实引用，`__all__` 中的 re-export 不算未使用导入。

## React / TSX

- 返回 JSX 的函数组件自动识别为 React 组件，允许 `PascalCase`，无需逐行豁免。
- 展示型布尔 props 由组件 Profile 接受，不与普通业务流程布尔参数混同。
- BCS003 的行数和局部变量是 warning，真实嵌套和圈复杂度是 violation；优先按职责和阶段拆分，不为了 48 行拆散内聚组件。
- JSX 属性、表达式箭头函数、多行签名和 type-only import 按 Tree-sitter 节点处理；TSX 的保守格式提示为低置信 warning。

## 续行

续行由对应 formatter 统一处理，不手工追求跨语言一致的对齐方式。运算符位置、参数折行和字符串处理必须在同一 Profile 内保持确定性。

## 外部签名与未使用参数

- C/C++：`[[maybe_unused]]`，或按项目既有 ABI 约定显式标记。
- Python：`_` 或 `_name`。
- JavaScript/TypeScript：`_name`。
- Rust：`_` 或 `_name`。

外部强制命名使用 `bits:allow(BCS010): 理由`，不得为规避命名规则批量关闭文件检查。
