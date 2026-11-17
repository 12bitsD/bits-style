# Bits’s Coding Style

[![CI](https://github.com/12bitsD/bits-style/actions/workflows/ci.yml/badge.svg)](https://github.com/12bitsD/bits-style/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Bits’s Coding Style V0.2 的公开规范与参考扫描器。规范只约束代码本身，不包含 MVC、设计模式或评审流程。

- [Coding Style V0.2：BCS001–BCS015](STYLE.md)
- [Language Profiles](docs/profiles.md)
- [Scanning Protocol V0.2](PROTOCOL.md)
- [逐语言支持矩阵](docs/language-support.md)
- [JSON Report Schema](schemas/report.schema.json)

## 核心结构

- `AUTO-CORE`：跨语言、可机检的语义规则。
- `AUTO-PROFILE`：由语言 Profile 决定的可机检格式和写法。
- `REVIEW`：需要设计判断，不参与自动 PASS/FAIL。

C/C++ 保留 Tab 8 和函数换行大括号；Python/Rust 使用 4 空格；JavaScript/TypeScript/TSX 使用 2 空格、lowerCamelCase 和同行大括号。编译器、ABI、FFI 和框架强制签名可以行级豁免。

## 实现

`bits` 是 Rust 编写的独立 CLI，使用 Tree-sitter 解析源码，不执行被扫描项目代码，扫描过程不访问网络。

当前扫描适配：C、C++、Python、JavaScript、TypeScript、TSX。Rust 目前只有规范 Profile，没有扫描适配。

**能解析一种语言不等于完整实现全部规则。** 函数接口、状态、错误处理和部分语义检查只支持文档列出的保守子集。逐规则状态见[支持矩阵](docs/language-support.md)。

## 安装

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
bits style report src --format jsonl
bits style report src --format sarif
bits style report src --jobs 8 --include-passed
```

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

## 自动修复边界

`fix` 只处理 Profile 缩进、行尾空白和文件末尾换行，不改变字符串内容。它不会重命名、删除代码、拆分函数、修改依赖或改变控制流。修复后必须重新扫描，连续执行两次不得产生新的 diff。

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

34 个测试直接驱动真实 CLI，覆盖规则边界、Profile、suppression、baseline、协议输出、Schema、退出码、并行确定性和修复幂等。

## License

[MIT](LICENSE)，版权主体为 Bits contributors。
