# Language Profiles

Profile 只决定代码形式和工具映射，不削弱 AUTO-CORE 语义规则。基础语法和语言官方工具已经覆盖的问题不重复检查；核心目标是让代码和逻辑简单到不容易出错。

| Profile | Formatter | 缩进 | 函数/变量命名 | 大括号 | 资源与错误处理 |
|---|---|---|---|---|---|
| C/C++ | clang-format | Tab 8 | `lower_snake_case` | 函数 `{` 下一行，控制块同行 | C 使用集中清理；C++ RAII，禁止 goto；非 void 返回值检查或显式丢弃 |
| Python | ruff format | 4 Space | `lower_snake_case` | 不适用 | `with`、`raise`；显式忽略使用 `_ = call()` |
| JavaScript/TypeScript/TSX | 项目既有 formatter + EditorConfig | 2 Space | `lowerCamelCase` | `{` 同行 | `try/finally`；显式忽略使用 `void call()` |
| Rust | rustfmt | 4 Space | `lower_snake_case`，枚举成员 `PascalCase` | rustfmt | `Drop`、`?`；显式忽略使用 `let _ = call()` |
| Go | gofmt/goimports | gofmt 决定 | 导出 `PascalCase`，未导出 `lowerCamelCase` | gofmt | `defer`、`context`、显式 error 路径；禁止 goto |

## 配置

仓库提供最小配置示例：

- `profiles/cpp.clang-format`
- `profiles/python-ruff.toml`
- `profiles/javascript.editorconfig`
- `profiles/rust-rustfmt.toml`

Go 使用独立的语义规范 [`profiles/go.md`](../profiles/go.md)，不增加重复格式配置。

这些文件只覆盖格式底线。语义检查由 `bits` 和各语言编译器/linter 共同完成，不能仅用 formatter 宣称全部 BCS 通过。

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
