# Contributing

## 开发要求

使用当前稳定版 Rust。提交前运行：

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

## 新增规则或语言支持

1. 在 `STYLE.md` 中保留唯一、明确的规则定义。
2. 在 `docs/language-support.md` 中准确声明覆盖范围。
3. 为 supported 的每个语言/规则组合添加通过和失败夹具。
4. 不得将仅能解析源码描述为支持全部规则。
5. 自动修复必须保持语义并通过幂等测试。
