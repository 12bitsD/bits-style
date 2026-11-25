# Bits Bench 基线体检（V0.4 MVP）

运行命令：

```bash
bits bench            # 首次拉取锁定 commit 的负样本库到 target/bench-cache/
bits bench --offline  # 使用本地缓存复跑，保证可复现
```

评测范围：正样本检查 `before` 召回与 `after` 消除；负样本只统计 L1 结构与语义规则，排除 `BCS001/002/007/008/009` 格式偏好（例如 `click`/`httpx`/`rich` 使用 88 列 formatter）。

## 正样本（`corpus/bad/`）

| 样本 | 维度 | 期望规则 | before | after | 结果 |
|---|---|---|---|---|---|
| `branch_head_setup` | `symmetry` | `BCS016` | `BCS016: 1` | 无 | OK |
| `mirrored_parameter_validation` | `state-gain` | `BCS003`, `BCS015` | `BCS003: 2`, `BCS015: 1` | 无 | OK |
| `nested_control_flow` | `control-flow` | `BCS003`, `BCS006` | `BCS003: 1`, `BCS006: 1` | 无 | OK |

`mirrored_parameter_validation` 同时验证了：**删除镜像下游校验后，函数从 56 行/圈复杂度 35 降到 7 行/圈复杂度 3**，无需拆碎函数即可消除硬错误。

## 负样本校准前后对比（硬错误数）

| 基准库（锁定 commit） | 文件数 | 校准前 | 校准后 | `BCS016` 命中 |
|---|---:|---:|---:|---:|
| `click` (`08b803cf`) | 17 | 459 | 288 | **0** |
| `httpx` (`7cff5f79`) | 23 | 197 | 101 | **0** |
| `rich` (`3a109ccd`) | 100 | 877 | 564 | **0** |
| **合计** | **140** | **1,533** | **953（-38%）** | **0** |

## 本轮已落地的校准

1. **修复 `BCS007` Python 循环误报（真实 Bug）**：`analyze_loop` 未排除 Python，在循环体内的 f-string `{key}` 插值中误找 `{` 报“左大括号位置错误”，曾迫使业务代码添加无意义的 `# bits:allow(BCS007)`。
2. **`BCS004` 布尔参数收窄**：只拦截无默认值的位置布尔参数，放过 `follow_redirects: bool = False` 等具名配置项。三库 `BCS004` 硬错误从 **600 降至 44（-93%）**。
3. **`BCS011` Python 类型与日志惯例**：识别 `typing.Callable[...]` 等带模块前缀的类型别名，允许模块级 `logger = logging.getLogger(...)`。
4. **`BCS014` 包公开导出**：允许 `__init__.py` 中 `from ._mod import *` 相对路径公开导出（`httpx` 中消除 19 处硬误报）。
5. **`BCS006` Python 双重否定**：支持 `not_operator` 与带括号的 `not (not x)`。

## 下一轮 Taste Loop 待治理项（按剩余噪声排序）

- **`BCS011`（剩余 571 次硬错误）**：主要为认证/请求流中的参数重赋值（如 `_auth.py` 中 `request = ...`）及模块级可变缓存。需抽检区分“真正隐藏状态”与“局部流式封装”，评估是否将部分子项降为 warning 或收窄定义。
- **`BCS014`（剩余 157 次硬错误）**：需抽检 `rich` 中的动态/条件导入（如 `TYPE_CHECKING` 或可选依赖）是否被误判为未使用。
- **`BCS003`（剩余 113 次硬错误）**：协议/渲染状态机与大型 dispatch 的圈复杂度，需确认阈值在声明式代码上的适用性。
- **`BCS013`（剩余 26 次硬错误）**：清理路径中的 `except: pass`（如流关闭、上下文管理器回滚）与显式忽略写法的区分。

---

# V0.4.1 抽检校准与机制落地

## 负样本演进对比（L1 硬错误）

| 基准库 | 扫描文件数 | 初测（v0.4.0 前） | v0.4.0 | v0.4.1 | 累计降幅 |
|---|---:|---:|---:|---:|---:|
| `click` (`08b803cf`) | 17 | 459 | 288 | **272** | -41% |
| `httpx` (`7cff5f79`) | 23 | 197 | 101 | **81** | -59% |
| `rich` (`3a109ccd`) | 77（排除生成的 `_unicode_data/`） | 877* | 564* | **315** | -64%* |
| **合计** | **117** | **1,533*** | **953*** | **668** | **-56%** |

*\*初测与 v0.4.0 的 `rich` 含 `_unicode_data/` 生成代码；v0.4.1 按规范第 5 节将生成数据表排除在基准外。*

正样本新增 `pure_io_leak`（`BCS017`）：**4 / 4 全部通过**（`before` 命中，`after` 消除）。

## `BCS011` / `BCS014` 抽检结论与修复

抽检发现“扫描器边界缺陷”与“Bits 有意严格的风格约束”并存，本轮只修前者，不为了压数字放松后者：

1. **Python 绝对导入误撞标准库（已修）**：`rich/abc.py` 中 `from abc import ABC` 曾被错误解析到自身 `abc.py` 形成假自环。现沿 `__init__.py` 向上定位顶层包名与包根，绝对导入按包根解析并忽略自环边。
2. **`TYPE_CHECKING` 运行时假环（已修）**：`if TYPE_CHECKING:` / `import type` 导入标记为 `type_only`，不进入运行时依赖环图；函数内延迟运行时 `import` 仍计为真实环（如 `httpx/_utils.py` ↔ `_urls.py`）。
3. **字符串前向类型注解误报 unused（已修）**：`x: "Console"`、`Union["URL", str]` 类型别名及 `cast("T", ...)` 中的字符串计为真实类型引用，普通字符串（如 `VALUE = "os"`）仍不误算；支持 `# noqa: F401` 显式再导出标记。
4. **Python 全局状态误判（已修）**：
   - `UNSET: t.Literal[...] = ...` 改用 AST `left` 取变量名，不再把类型注解混入常量名；
   - `if __name__ == "__main__":` 入口块变量不再算共享可变全局（消除 `rich/__main__.py` 约 200 次误报）；
   - `except ImportError: brotli = None` 可选依赖回退不再算可变全局；
   - `import os as o` 模块别名现在按真实模块名 `os` 匹配（同时供 `BCS017` IO 判定）。
5. **有意保留的严格约束（非误报）**：
   - `BCS011` 参数重赋值（如 `encoding = encoding or "utf-8"`）：Bits 有意禁止复用参数绑定，存量项目通过 `baseline` 与 `improving` 棘轮渐进治理。
   - `BCS014` 真实运行时环（如 `rich` 中 `console.py` 与渲染组件的 72 处运行时/延迟环）：属于 Bits 有意禁止的架构耦合。
   - `BCS011` 大写可变字面量（如 `EMOJI = {...}`）：纯数据表可按规范用 `bits:allow` 说明，或由 `Final` / 不可变结构替代。

## 新增机制

- **`BCS017` 纯度污点传播**：`bits:spec(pure)` + 内置/配置 `io_sources`，沿同文件与真实 import 调用图传播 IO 污点，触污报硬错误；在三库 117 个文件上 **0 误报**。
- **`improving` 棘轮**：`--diff` 下已存在函数的 `BCS003` 嵌套/圈复杂度严格低于基线 commit 时标记 `improving = true` 并降级为 warning，鼓励存量代码持续做减法；新函数与恶化改动仍硬拦截。

---

# V0.4.2 业界标准吸收与认知复杂度分布实测（Python 打通）

## `cognitive_v1` 认知复杂度分布（三库共 1,942 个函数）

按 SonarSource 白皮书原教旨实现（结构节点 `1 + 嵌套`、混合节点 `+1`、逻辑序列/标签跳转/直接自递归/match guard `+1`、整个 `match/switch` 只计一次），实测分布如下：

| 分位数 / 阈值 | 数值 / 占比 |
|---|---:|
| P50 | **1** |
| P90 | **8** |
| P95 | **13** |
| P99 | **34** |
| Max | **119** |
| `> 10` 的函数占比 | 7.16% |
| `> 15` 的函数占比 | **3.86%** |
| `> 20` 的函数占比 | 2.37% |
| `> 25` 的函数占比 | 1.75% |

**阈值决策**：遵循“先用分布反推阈值，不拍脑袋设硬门禁”的纪律。若按 11 起步报警会波及约 7% 的正常函数，噪声过大；**15 位于 P95（13）与 P99（34）之间**，因此本轮将 `cognitive_v1 > 15` 设为高置信 **shadow warning**（不阻断 CI），保留旧圈复杂度 >10 与嵌套 >3 硬错误以兼容存量 baseline，待补齐 Rust/Go 跨语言基准后再评估是否晋升硬门禁。

## 本轮落地的改进

1. **封堵协议后门（P0）**：
   - `--diff` 新增 `bits:explain` 直接报 `BCS015` 硬错误（全量扫描存量 `explain` 仍兼容）；
   - `bits:prove(..., test=X)` 通过 AST 唯一解析真实测试定义（支持 Python `test_*`/`Test*.test_*`、TS `test()/it()`、Rust `#[test]`，diff 模式自动索引未变更测试文件）。
2. **跨语言计分与 Rust 边界修正**：
   - 统一 JS/C++/Rust `else if` 与 Python `elif` 的嵌套深度，补齐 Rust 控制节点；
   - 棘轮函数身份升级为 `(函数名, 同名序号)`，避免同名方法碰撞且不受行号移动影响；
   - 放行 Rust `pub use ...::*` 与 `#[cfg(test)]` 模块内 `use super::*`。
3. **新增 4 项 L1 检查（三库 0 硬误报）**：
   - 可折叠双层 `if`（`BCS005` warning，三库共 9 处）；
   - 相邻同体分支（`BCS016` warning，跳过 `typeof`/`isinstance` 收窄，三库共 2 处）；
   - 纯重抛 `try` 与 `raise var` 冗余重抛（`BCS013` warning）；
   - 断言条件显式副作用 `:=`/`=`/`++`（`BCS006` 硬错误，三库 **0 误报**，不误伤 `assert len(x) > 0`）。
4. **Python L0/L1 去重**：`profiles/python-ruff.toml` 仅开 `F`、`B006`、`PLR1714`、`PIE790` 且 `unfixable = ["ALL"]`，结构规则统一归 Bits L1。
5. **文档体例**：`STYLE.md` 与 L2 Skill 全面改写为「反例形状 → 维护代价 → 合理例外 + Anti-fix」，纳入 Clippy 分级纪律、Ousterhout 接口税测试与 TigerStyle 契约气闸。
