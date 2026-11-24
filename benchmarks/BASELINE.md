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
