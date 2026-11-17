# Bits’s Coding Style

[![CI](https://github.com/12bitsD/bits-style/actions/workflows/ci.yml/badge.svg)](https://github.com/12bitsD/bits-style/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Bits’s Coding Style V0.1 的公开规范与参考扫描器。规范只约束代码本身，不包含 MVC、设计模式、评审流程或独立语言 Profile。

- [Coding Style：BCS001–BCS015](STYLE.md)
- [Scanning Protocol V0.1](PROTOCOL.md)
- [逐语言支持矩阵](docs/language-support.md)
- [JSON Report Schema](schemas/report.schema.json)

## 实现

`bits` 是 Rust 编写的独立 CLI，使用 Tree-sitter 解析源码，不执行被扫描项目代码，扫描过程不访问网络。

当前适配：

- C、C++
- Python
- JavaScript、TypeScript、TSX

**能解析一种语言不等于完整实现全部规则。** 函数接口、状态、错误处理和部分语义检查只支持文档列出的保守子集。逐规则状态见[支持矩阵](docs/language-support.md)，机器报告中的 `coverage` 列出实际执行和部分覆盖的规则。

## 安装

当前版本从源码安装：

```bash
cargo install --git https://github.com/12bitsD/bits-style --locked
```

也可以本地构建：

```bash
git clone https://github.com/12bitsD/bits-style.git
cd bits-style
cargo build --release
./target/release/bits --help
```

## 使用

```bash
# 只读检查；发现违规返回 1
bits style check src

# 输出 JSON 报告；报告成功生成时返回 0
bits style report src --format json --output bits-report.json

# 只修复确定性空白问题，然后完整重扫
bits style fix src
```

### 扫描范围

```bash
bits style check . --all
bits style check . --changed
bits style check . --staged
```

`--changed` 使用工作区和暂存区相对 `HEAD` 的变化，`--staged` 只使用暂存区。未指定路径时默认扫描当前目录。

### 输出

```bash
bits style check src --format text
bits style report src --format json
bits style report src --format jsonl
bits style report src --format sarif
bits style report src --include-passed
bits style report src --jobs 8
```

文本输出示例：

```text
src/parser.ts:81:9: BCS003: control-flow nesting exceeds three levels actual=4 expected=<=3 fixable=no
```

### 配置

通过 `--config` 指定排除规则：

```toml
# bits.toml
exclude = ["**/generated/**", "vendor/**"]
```

```bash
bits style check . --config bits.toml
```

扫描默认遵循 `.gitignore`，并排除 `.git`、依赖目录、构建目录和常见生成文件。`--no-ignore` 禁用 ignore 文件，但不会重新包含 `.git` 和扫描器构建产物。

## 自动修复边界

`fix` 只处理：

- Tab 与缩进。
- 行尾空白和文件末尾换行。
- 不改变字符串内容的空白修复。

它不会重命名、删除代码、拆分函数、修改依赖或改变控制流。修复后必须重新扫描；连续执行两次不得产生新的 diff。

## 退出码

| 退出码 | 含义 |
|---|---|
| `0` | 无违规；或 `report` 成功生成报告 |
| `1` | 扫描完成，存在违规 |
| `2` | 参数、配置、读取或解析错误 |

## 开发与验证

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

测试直接驱动真实 CLI，覆盖 C/C++、Python、JavaScript/TypeScript/TSX、协议输出、Schema、退出码、并行确定性和修复幂等。

## License

[MIT](LICENSE)，版权主体为 Bits contributors。
