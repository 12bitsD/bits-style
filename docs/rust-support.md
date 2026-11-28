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
- `static mut` 是明确的全局可变状态；普通 `const` 和不可变 `static`（含函数内 `static REGEX: OnceLock<...>`）均允许 `UPPER_SNAKE_CASE` 且不误报。
- 大括号风格（`BCS007`）与 `match_arm`/case 缩进（`BCS008`）由 `rustfmt` 与 Rust 语法强制保障，在 Rust 上标为 `not_applicable`。
- 元组解构与 Rust 关键字（`let (a, b)`、`pub (crate)`、`const`、`static`、`fn`、`move`、`where`、`loop`、`unsafe`、`extern`、`ref`、`mut`、`type`）在 `BCS009` 中均已白名单放行。
- Rust 行级注释（`// bits:allow(...)`、`// bits:prove(...)`、`// bits:spec(pure)`）和块注释（`/* ... */`）均正常识别。
- `bits-style` 自身源码库（`src/`）已在 `bits.toml`（含 `[[layers]]` 分层契约）下达成 `0 violations, 0 warnings`。
- 未适配语言在目录扫描和 `--diff` 下默认静默跳过并计入 `Skipped (unsupported)`，仅在 `--strict-unsupported` 或显式传单文件时失败。
