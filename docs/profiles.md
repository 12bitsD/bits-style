# Language Profiles

Profile 只决定代码形式和工具映射，不削弱 AUTO-CORE 语义规则。

| Profile | Formatter | 缩进 | 函数/变量命名 | 大括号 | 资源与错误处理 |
|---|---|---|---|---|---|
| C/C++ | clang-format | Tab 8 | `lower_snake_case` | 函数 `{` 下一行，控制块同行 | C 使用集中清理；C++ RAII，禁止 goto；非 void 返回值检查或显式丢弃 |
| Python | ruff format | 4 Space | `lower_snake_case` | 不适用 | `with`、`raise`；显式忽略使用 `_ = call()` |
| JavaScript/TypeScript/TSX | 项目既有 formatter + EditorConfig | 2 Space | `lowerCamelCase` | `{` 同行 | `try/finally`；显式忽略使用 `void call()` |
| Rust | rustfmt | 4 Space | `lower_snake_case`，枚举成员 `PascalCase` | rustfmt | `Drop`、`?`；显式忽略使用 `let _ = call()` |

## 配置

仓库提供最小配置示例：

- `profiles/cpp.clang-format`
- `profiles/python-ruff.toml`
- `profiles/javascript.editorconfig`
- `profiles/rust-rustfmt.toml`

这些文件只覆盖格式底线。语义检查由 `bits` 和各语言编译器/linter 共同完成，不能仅用 formatter 宣称全部 BCS 通过。

## 续行

续行由对应 formatter 统一处理，不手工追求跨语言一致的对齐方式。运算符位置、参数折行和字符串处理必须在同一 Profile 内保持确定性。

## 外部签名与未使用参数

- C/C++：`[[maybe_unused]]`，或按项目既有 ABI 约定显式标记。
- Python：`_` 或 `_name`。
- JavaScript/TypeScript：`_name`。
- Rust：`_` 或 `_name`。

外部强制命名使用 `bits:allow(BCS010): 理由`，不得为规避命名规则批量关闭文件检查。
