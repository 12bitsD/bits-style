# Rust Support

Rust 通过 Tree-sitter 接入，当前为 partial 支持，不代表完整 Rust 语义分析。

## 已覆盖

- `function_item`、`closure_expression`、`function_signature_item` 的函数规模、参数和命名。
- `if_expression`、`match_expression`、循环和 match arm 的结构检查。
- `let_declaration`、`static_item`、`const_item` 的命名和可变全局检查。
- `use_declaration` 的绑定、通配符导入和 crate/self/super 相对依赖；通配符导入放行 `pub`/`pub(...)` 再导出、`::prelude::*`、枚举变体（大写所有者）、`#[cfg(...test...)]` / `#![cfg(test)]`、`tests.rs`/`mocks.rs` 等测试辅助文件及同文件内联子模块的 `use super::*`。
- 隐式环境输入仅限 `env::var` / `env::args`（作为边界 warning），不再误伤 `Instant::now()`。
- `unsafe { expr };` 的尾分号不算同行多语句；`///` / `//!` 文档注释不算注释掉的旧代码。

## 委托给官方工具链（L0）

- 未使用导入交由 `rustc`（`unused_imports`），避免 trait 方法解析误报。
- `unwrap`、`expect`、`panic!`、`todo!`、`unimplemented!` 交由 Clippy（`restriction` 组），Bits 不做文本/子字符串级告警。

## 边界

- 不宣称完整生命周期、借用、类型推断、宏展开、trait resolution 或跨 crate 语义分析。
- `static mut` 是明确的全局可变状态；普通 `const` 和不可变 `static` 不误报。
- `match_arm` 的缩进交给 rustfmt，不使用简单的列偏移规则，避免 `let ... = match ...` 误报。
- 已知待修项（V0.4 自检发现）：元组解构 `let (a, b) = ...` 会被 `BCS009` 误判为函数名与括号分离；多行闭包/let-else 的大括号位置由 rustfmt 决定，`BCS007` 在 Rust 上仍会误报。
- 未适配语言仍进入 `unsupported`，不会与 Rust 的 partial coverage 混淆。
