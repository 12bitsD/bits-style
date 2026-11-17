# Agent Integration

完整 Skill 位于 [`skills/bits-style/SKILL.md`](../skills/bits-style/SKILL.md)，包含采用流程、增量门禁、CI 模板和初始化脚本。

## 初始化

```bash
./skills/bits-style/scripts/bootstrap.sh /path/to/project origin/main
```

脚本只在文件不存在时创建：

- `bits.toml`
- `AGENTS.md`
- `.github/workflows/bits.yml`
- `bits-baseline.json`

已有指令、CI 和 baseline 不会被覆盖。没有安装 `bits` 时会先提示安装：

```bash
cargo install --git https://github.com/12bitsD/bits-style --tag v0.3.4 --locked
```

## 日常工作流

```text
bits style report
→ 分类违规、warning、unsupported 和外部契约
→ 修复高置信语义问题
→ 运行项目 formatter、类型检查和测试
→ 重新扫描
→ 报告剩余 warning 和 unsupported
```

增量修改使用：

```bash
bits style fix --diff --commit origin/main
bits style check --diff --commit origin/main
```

只有确定违规才应阻断 CI。函数规模、命名、格式和模糊业务语义继续作为 review 输入，不应为了消零而拆函数或增加状态包装。

## 接入模板

- `templates/AGENTS.md`：项目指令，不是完整规范替代品。
- `templates/bits.toml`：生成代码、依赖目录和构建目录排除项。
- `templates/ci.yml`：固定版本的增量 CI 示例。

初始化脚本的真实临时项目测试覆盖非覆盖和幂等行为。
