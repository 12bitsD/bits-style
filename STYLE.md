# Bits’s Coding Style V0.4

> 正确 > 清晰 > 可修改 > 一致 > 简短

本规范只约束代码本身，不包含 MVC、设计模式或评审流程。底层目标：**让代码和逻辑简单到不容易出错，并让优雅性成为可验证的工程约束。**

## 0. 品味治理框架（V0.4）

### 0.1 五条宪法价值观

1. **读优先于写，删优先于增**：优雅不是拆得碎或堆模式，而是读一遍就能信任；能删掉的结构永远优于多包一层。
2. **宁漏勿错（Precision > Recall）**：一次误报就会摧毁人与 Agent 对工具的信任。静态层宁可放过可疑代码，也不骚扰正常代码。
3. **三层各守边界**：卫生交给官方工具（L0），纯结构矛盾交给 AST（L1），语义与设计品味交给 Skill / 独立 Critic（L2）。
4. **以修复后结果论英雄**：一条规则的好坏，看 Agent 按它修完后代码是否真的更干净，而不是拆成两行碎片或堆满注释。
5. **从真实痛点生长，用顶级代码校准**：只从真实 Review 提炼规则，上线前必须通过开源标杆库的误报拷打。

### 0.2 优雅的四个维度（反例 → 代价 → 例外）

#### 1. 层级纯度：一个函数只在一个抽象高度说话
- **反例形状**：高层编排函数里突然混入底层细节或局部导入（如在 `_dispatch` 的 `Harness.load()` 之间手拼 dict、函数中间 `from .contracts import ToolCall`）。
- **维护代价**：读者必须在“流程编排”和“字段拼装”两个思维高度之间反复切换；后续修改容易在错误的层级插入日志或清理逻辑。
- **合理例外**：小型适配器的一行返回字面量（如 `return {"ok": True, "data": res}`）。
- **Anti-fix（错误修法）**：为消除提示把 `return {"ok": True}` 拆成两行“饺子皮函数”。

#### 2. 控制流平直度：主路一条直线，分支只做真实决策
- **反例形状**：箭头形深层嵌套；用长 `if/elif` 模拟查表；`argparse` 已解析命令又按字符串二次路由；用复合 key 查表后再二次解码。
- **维护代价**：读者需要在脑中维护多层上下文栈，或逆向 key 的构造与优先级才能还原真实行为。
- **合理例外**：扁平 `match/switch`、连续的 guard clauses、协议状态机的直接枚举分派。
- **Anti-fix**：把显式条件编码成 `f"{cmd}:{retry}:{empty}"` 字符串 key 塞进表里，或把连续逻辑切成互相传 `ctx` 的碎片函数。

#### 3. 信息增益：只为本层决策解析，状态都被消费
- **反例形状**：透传层镜像下游参数校验（`validate_parameters`）；猜三种下游错误 JSON 重新拼文案（`parameter_error`）；定义了只写不读的投机字段。
- **维护代价**：复制了别人拥有的知识，下游一变本层就过期；解析失败反正回退原文，说明解析并不驱动任何行为。
- **合理例外**：本层确实用该值做分支（如 `retryable` 决定重试）、安全脱敏、资源截断（如 64KiB）、保护自身不变量（如保留键）。
- **Anti-fix**：用 `bits:prove: cohesive logic` 之类的空话为镜像校验辩护，或把猜测逻辑搬进另一个 helper。

#### 4. 对称性与契约气闸：同样的事一种写法，断言只证内部不变量
- **反例形状**：互斥分支头部重复贴同一个前置调用；同类逻辑一半平铺一半嵌套；用 `assert request.token` 拦截外部输入错误，或在 `assert (x := read())` 里藏副作用。
- **维护代价**：重复副本容易只改一边导致漂移；可裁剪断言在优化构建（`-O`/`NDEBUG`）下直接改变程序行为。
- **合理例外**：内部不变量断言（`assert cursor <= len(buf)`）；外部框架强制的固定签名。
- **Anti-fix**：为满足断言数量要求添加 `assert True` 或重复下游校验。

### 0.3 三层漏斗与分级纪律（对齐 Clippy / Staticcheck）

| 层级 | 对应业界分级 | 职责与默认策略 |
|---|---|---|
| **L0 卫生层** | Formatter / Pyflakes / Clippy `style` | 官方工具负责语法、格式、重复导入、未使用变量、语言特有坑（如 Ruff `B006` 可变默认参数）。**只诊断，禁止自动改写语义。** |
| **L1 拓扑层（硬错误）** | Clippy `correctness` | `severity=violation, confidence=high`：零误报的确定性结构矛盾（如嵌套 >3、断言显式副作用、`bits:spec(pure)` 触 IO）。**不能靠 `bits:prove` 豁免。** |
| **L1 拓扑层（Warning）** | Clippy `suspicious` / `complexity` | `severity=warning`：确定的结构坏味道（如 `BCS016` 同体分支、可折叠 `if`），用于圈定 L2 热区；**不阻断 CI，不要求补注释。** |
| **L2 品味层** | Clippy `pedantic` + Ousterhout | 主观但有高价值的品味审查，由独立 Critic 只聚焦 Diff 改动函数与 L1 Warning 热区，无问题保持沉默。 |
| **Restriction / Opt-in** | Clippy `restriction` | `bits:spec(pure)`、`bits.toml` 的 `io_sources`、Go Profile 等按项目政策显式开启，绝不整组强推。 |

**L0/L1 去重契约**：跨语言结构规则（可折叠 `if`、同体分支、冗余 `else`、无效重抛、布尔陷阱）**统一由 Bits L1 拥有**；各语言 L0 配置模板（如 `profiles/python-ruff.toml`）不得重复开启对应规则，避免同一行代码双重告警。

### 0.4 新规则准入与退出（Eval-Driven）

任何新 L1 规则上线前必须同时通过：
1. **正样本召回**：`corpus/bad/` 中真实反面案例 `before` 命中、`after` 消除。
2. **开源盲测**：在锁定 commit 的基准库上，硬错误 **0 误报**，Warning 误报函数占比 **< 1%**（`bits bench` 按 `functions_with_warnings / functions_checked` 统计）。新复杂度指标先看 **P50/P90/P95/P99 分布**再定阈值，不拍脑袋设硬门禁。
3. **Agent 对抗测试**：Agent 修复后代码更接近 `after`，而非拆碎函数、编造 `bits:prove` 或改写成晦涩查表。
4. **退出机制（Staticcheck 纪律）**：实测出现真实误报的规则，必须收窄、降级或退出默认集；扫描器的误报修扫描器，不靠让用户打 `bits:allow` 消化。

## 1. 规则类型与适用范围

- `[AUTO-CORE]`：跨语言、可机器判定的语义规则，影响扫描退出码。
- `[AUTO-PROFILE]`：可机器判定，但具体形式由语言 Profile 定义，影响扫描退出码。
- `[REVIEW]`：需要设计判断，不参与自动 PASS/FAIL。

语言 Profile 定义缩进、括号、命名、续行、资源清理和工具配置。编译器、ABI、FFI、框架重写方法及回调强制要求的签名和命名优先于本规范，但必须使用第 5 节的行级豁免。

当前 Profile：C/C++、Python、JavaScript/TypeScript/TSX、Rust、Go。Rust 与 Go 均为 partial 支持，扫描器支持状态以 `docs/language-support.md` 为准。

基础语法和语言官方工具已经覆盖的问题不在 Profile 中重复检查。Bits 的底层目标是让代码和逻辑简单到不容易出错，优先约束错误路径、状态、资源、并发和抽象成本。

## 2. 规则

### BCS001　缩进与空白 `[AUTO-PROFILE]`

- 文件必须以一个换行符结束，禁止行尾空白，禁止混用 Tab 和 Space 缩进。
- C/C++：Tab 缩进，`TabWidth = 8`、`IndentWidth = 8`。
- Python、Rust：4 个 Space 缩进。
- JavaScript/TypeScript/TSX：2 个 Space 缩进。
- Go：缩进与行宽完全由 `gofmt` 决定，Bits 不重复检查。
- 禁止源码内 editor modeline。

### BCS002　行宽与续行 `[AUTO-PROFILE]`

- 默认每行不超过 80 列，Profile 可以调整行宽。
- 续行缩进以及运算符放行首或行尾由 Profile 和 formatter 统一决定。
- 不得为了满足行宽拆分用户可见字符串、多行字符串/模板/docstring、日志或错误信息、URL；这些场景自动豁免。

### BCS003　函数规模与模块体量 `[AUTO-CORE]`

每个函数同时检查四项指标（测试代码与业务代码执行同样严格的规模约束）：

- 非空、非注释代码行不超过 48 行。纯字符串内容和多行类型声明不计入函数实现行数，只产生 warning。
- 控制流嵌套不超过 3 层，属于确定的 violation，并附带 Top 热点行号归因（`hotspots`）。
- 局部变量不超过 10 个；`int a, b;` 计两个，参数不计，只产生 warning。
- 圈复杂度不超过 10，属于确定的 violation，并附带 Top 热点行号归因（`hotspots`）。扁平 `switch/match` 的 `case` 不增加圈复杂度，但分支内的其他判断仍然计数。
- **模块体量（非测试文件）**：单文件非空、非注释、非导入净代码行超过 300 行时产生 `warning`（`category = level-purity`），遏制上帝文件膨胀。
- **单调用瘦包装（Thin Wrapper）**：文件内仅被调用 1 次的私有 helper 函数，若函数体仅 1 条 `return callee(原样全部参数)`，产生 `warning`（`category = level-purity`）并给出内联处方。

- **认知复杂度 `cognitive_v1`（Sonar 白皮书口径，观测中）**：普通 `if`/循环/`catch`/三元/整个 `match/switch` 记 `1 + 当前嵌套`；`elif`/`else if`/`else` 只记 `+1`；逻辑运算按连续序列计分，`goto`/直接自递归/`match guard` 各记 `+1`。当前 `> 15` 输出高置信 `warning`（shadow 观测，附带 `hotspots`），硬阈值待跨语言基准 P95/P99 分布校准后再定。`else if` 与 Python `elif` 已统一嵌套深度。

行数、文件体量和局部变量数超过阈值产生 `warning`，不直接阻断 CI。真实嵌套和圈复杂度超过阈值仍属于 violation。优先删除冗余层或按职责拆分，不按行数机械切段，也不为了达标拆散内聚逻辑；确需保留时使用第 4.2 节的 `bits:prove`。

### BCS004　函数接口 `[AUTO-CORE]`

- 参数不超过 4 个，不包含语言隐含的 `this/self/cls` 以及 Python 裸 `*` 分隔符，超过产生 warning。Python 中对含 `*` 具名 keyword-only 参数的函数：位置参数仍须 `<= 4`，普通函数总业务参数上限为 `6`，`__init__`/`__new__` 依赖注入构造器总业务参数上限为 `8`。对象字段和响应数据中的布尔值不属于行为开关。
- 禁止无默认值的位置布尔参数切换行为（如 `run(enabled: boolean)`）；带默认值的具名配置项或 keyword-only 选项（如 `follow_redirects: bool = False`）不属于布尔陷阱。React 组件的展示型布尔 props 由 React Profile 自动识别。
- 分离的函数声明必须包含参数名。
- 外部框架、接口、回调和 FFI 强制的签名可以通过行级豁免保留。
- 返回 `None`/`void` 且原地修改 ≥ 2 个参数容器（如 `.append`/`.add`/`.update`/`.setdefault`/`.push`/`p[k] = ...`）的多出参函数产生 `warning`（`category = state-gain`），并附带返回结构化结果的处方。
- **设计力学检测（全部附带可执行 `prescription` 处方，输出为 `warning`）**：
  - **Data Clump（数据团块）**：同文件 ≥ 3 个不同名函数反复共享同一组 ≥ 3 个业务参数时，提示合并为不可变值对象（`dataclass`/`struct`）。
  - **Tramp Data（透传胶水）**：≥ 5 参数函数把一半以上（且 ≥ 4 个）参数原样直传给同一个下游调用（排除 `super()`）时，提示改为直接传对象或由调用方构造。
  - **Adjacent Same-Type Positional Params（相邻同类型位置参数）**：Python/TS/TSX 函数位置参数 ≥ 3 且出现相邻同基础类型非对称参数（如 `after_seq: int, max_events: int`）时，提示加 `*` 强制 keyword-only 或收成具名选项对象。
- 不得使用无约束的 `map/dict/options` 打包互不相关的参数来规避限制 `[REVIEW]`。

### BCS005　控制流 `[AUTO-CORE]`

- 异常路径使用提前 `return/throw`，主流程保持靠左。
- `return/throw` 之后不得保留无意义的 `else`。
- C++、Rust 及其他以 RAII、defer、with 管理资源的语言禁止 `goto`。
- C 中 `goto` 只能向前跳转，标签必须匹配 `out_*` 或 `err_*`，禁止 `err1/err2`。
- **可折叠双层 `if`**：外层 `if` 无 `elif/else`（非 `else if`、非条件声明/初始化语法），且执行体只有一条同样无 `elif/else` 的内层 `if` 时，报 `warning`（`category = control-flow`），提示合并为短路条件；不自动改写。
- 是否应通过查表替代分支 `[REVIEW]`。

### BCS006　语句与表达式 `[AUTO-CORE]`

- 每行最多一条语句，禁止一行多个赋值；可变容器字面量或调用结果的链式赋值（如 `a = b = []`、`a = b = make()`）为硬错误，纯标量链式赋值（如 `lo = hi = 0`）为 warning。
- 禁止在条件表达式内赋值；语言提供的结构化条件初始化语法由 Profile 决定。
- 链式赋值按真实 assignment target 判断，函数调用的关键字参数不属于链式赋值。
- 禁止使用逗号表达式代替大括号。
- 禁止嵌套三元表达式（含用括号包裹的嵌套三元）和双重否定条件。
- 混合不同优先级的运算符时必须加括号。
- **断言条件禁止显式副作用（硬错误）**：Python `assert` 或 C/JS `assert(...)` 的条件子树中禁止出现 `:=`、赋值 `=`、`+=`、`++/--` 等语法级变异（`-O`/`NDEBUG` 裁剪会改变程序行为）；普通谓词调用如 `assert len(x) > 0`、`assert is_valid(x)` 不报警。
- 是否构成难以理解的高密度表达式 `[REVIEW]`。

### BCS007　大括号 `[AUTO-PROFILE]`

- 所有 `if/else`、循环和 `switch/match` 控制块必须加大括号，语言语法不允许时除外。
- C/C++：非函数块的 `{` 与语句同行；函数定义的 `{` 独占下一行；`} else {` 同行。
- JavaScript/TypeScript/TSX：`{` 与语句或函数声明同行，`} else {` 同行。
- Python 不使用大括号；Rust 按 rustfmt 执行。

### BCS008　分支标签 `[AUTO-PROFILE]`

- C/C++：`case/default` 与所属 `switch` 缩进相同，禁止 double indent。
- JavaScript/TypeScript/TSX：`case/default` 使用 Profile 定义的一级缩进。
- 多个 `case` 共用一段逻辑时必须连续排列。
- 有意贯穿的 `case` 必须显式标记 `fallthrough` 或等价说明。

### BCS009　空格 `[AUTO-PROFILE]`

- 空格、括号、运算符和函数调用的格式由 Profile 的 formatter 决定。
- formatter 无法覆盖的约束必须写成具体、可机检的规则，不使用“合理”“美观”等判定词。
- TSX/JSX 的保守格式提示只产生低置信 `warning`，JSX 区域交给 Profile formatter。

### BCS010　命名 `[AUTO-PROFILE]`

- C/C++、Python、Rust：函数和变量使用 `lower_snake_case`。
- JavaScript/TypeScript/TSX：函数和变量使用 `lowerCamelCase`。
- 类型和类型别名使用 `PascalCase`；模块级 const 同时接受 `UPPER_SNAKE_CASE` 和 Profile 普通变量名，Rust 枚举成员使用 `PascalCase`。
- Pydantic/JSON 数据字段、HTTP 框架回调、React 组件和 memo 包装组件保留外部命名契约，不强制改成 `lower_snake_case`。
- 禁止匈牙利命名。
- 编译器、ABI、FFI、框架重写方法和回调的强制命名优先，但必须行级豁免。
- 名字长度应与作用域匹配，带单位的值使用 `timeout_ms` 等后缀，布尔名称优先使用 `is_/has_` 等肯定式 `[REVIEW]`。

### BCS011　状态与可变性 `[AUTO-CORE]`

- 禁止新增可变全局变量、Singleton 和隐式上下文输入。Python `nonlocal` 跨闭包改写外层状态报硬错误，`global` 报 warning。环境变量读取应在入口边界完成并显式传参；Composition Root（`main`、`run_cli`、`cli_main`、`if __name__ == "__main__":`）及已接受显式 `env`/`environ`/`env_override` 注入参数的回退读取自动放行。
- 默认使用 `const/final` 或语言等价机制 `[AUTO-PROFILE]`。
- 禁止给函数参数重新赋值；对象属性更新与参数绑定替换必须区分。
- 类型别名不是可变全局状态；`Annotated/Literal/Callable`（含 `typing.` 前缀）按类型表达式处理。
- Python 中带类型注解的 `UPPER_SNAKE` 常量、模块级 `logger`、`if __name__ == "__main__":` 入口块内变量、`except ImportError: mod = None` 可选依赖回退不属于可变全局状态。
- 禁止变量遮蔽，包括局部变量遮蔽参数。
- 变量是否处于足够小的作用域和生命周期 `[REVIEW]`。

### BCS012　宏、Inline 与条件编译 `[AUTO-PROFILE]`

仅适用于具有对应语法的 Profile：

- 多语句宏必须使用语言提供的安全语句块包装。
- 宏及元编程不得隐藏 `return/goto/break/continue`，不得依赖调用方作用域中的固定变量名。
- C/C++ 显式 `inline` 函数体不得超过 3 行；仅调用一次的局部函数禁止手工添加 `inline`。
- 条件编译只能包围完整声明或定义，禁止切入函数体内部。

### BCS013　错误处理 `[AUTO-CORE]`

- 禁止空异常处理和静默吞错（硬错误）；宽泛 `except:` / `except Exception:` / `catch (...)` 中仅写裸 `return`/`continue`/`break` 静默丢弃异常同样为硬错误；`catch/except` 返回伪造成功值（`return true/True`、`return 0`、`return {"ok": True}`、隐式空 `return`）产生 warning；解析/探针场景返回显式失败或缺失哨兵（`return false/False`、`return null/None/undefined`）不误报。
- 必须忽略清理路径错误时，使用 Profile 规定的显式丢弃写法，禁止空 `catch/except`。
- catch-all 只能继续抛出，或在系统边界转换为明确的错误类型；无法静态确认边界的转换只产生低置信 `warning`。
- 非 void、非单元返回值必须检查或显式丢弃 `[AUTO-PROFILE]`。
- **无效果异常包装（warning）**：`try` 的所有 handler 仅含裸 `raise`/`throw;` 且无 `else/finally` 时，提示删除整个 `try`；handler 唯一语句为 `raise caught_var` / `throw caught_var;` 时，提示改为裸重抛或删除 handler（C++ 按值 `throw e` 还会导致切片）。
- `try` 块是否最小化、异常转换是否保留 `cause/from err` `[REVIEW]`。

### BCS014　依赖与无用代码 `[AUTO-CORE]`

- 模块运行时依赖必须无环；`TYPE_CHECKING` / `import type` 仅类型边不进入运行时环图，自环边忽略。Python 绝对导入按 `__init__.py` 包根解析，不将标准库同名模块误判为包内模块。
- 禁止未使用的导入、依赖、变量、函数和不可达代码；Python 私有 `_fn`（未被同仓其他模块导入或 `__all__` 导出）及 JS/TS ES Module 中未导出的顶层函数/`const` 仅出现一次时产生死代码 `warning`；Python 类型注解与类型别名中的字符串前向引用（如 `x: "Console"`、`Union["URL", str]`）及 `# noqa: F401` 显式导出计为真实引用，普通字符串（如 `VALUE = "os"`）不计。
- 禁止通配符导入（Python `__init__.py` 相对公开导出；Rust `pub`/`pub(...)` 再导出、prelude、枚举变体、`#[cfg(...test...)]`、测试/模拟文件及同文件内联子模块的 `use super::*` 除外）；Go 非测试文件的 dot import 报 warning。禁止保留注释掉的旧实现（多行说明块、文档示例与映射说明句不算旧实现）。Rust/Go 的未使用导入交由 `rustc` / Go 编译器判定。
- 接口或回调的未使用参数必须使用 Profile 规定的 `_`、`_name` 或 `[[maybe_unused]]` 等显式标记。

### BCS015　注释与源码元数据 `[AUTO-CORE]`

- TODO 必须使用 `TODO(owner): 明确的完成条件` 格式；条件是否足够明确 `[REVIEW]`。
- 禁止 editor modeline 和通过源码注释覆盖团队统一格式。
- 对齐的数据表格可以使用受控 formatter-off 区域，但必须包含规则号和理由，并在表格结束后立即恢复。
- 注释应解释原因、约束和例外，不复述代码 `[REVIEW]`。
- 未命中任何告警的 `bits:allow` / `bits:prove` 属于过期墓碑注释，必须删除（硬错误）。
- `bits:prove` 不能豁免硬错误，且在 `--diff` 模式下每次变更最多新增 1 条（硬错误）。
- `bits:prove(..., test=X)` 中的 `X` 必须唯一解析到扫描范围（含 diff 只读测试上下文）中的真实测试定义，否则报硬错误。
- `--diff` 模式下禁止新增 legacy `bits:explain`（硬错误）。

### BCS016　分支对称性与重复形状 `[AUTO-CORE]`

- 带兜底 `else` 的穷尽 `if/elif/else` 链，且所有分支条件无副作用时，若每个分支的首条语句都是完全相同的独立调用（如 `await harness.check()`），产生 `warning`（`category = symmetry`，`obligation = prove_minimalism`）。
- 将该调用提升到分支之前语义等价；若各分支实际在做命令路由，优先改为注册表或 `set_defaults(handler=...)` 直接绑定处理函数。
- 无 `else` 的非穷尽分支、含副作用条件（如条件中调用函数）或首条调用不一致时不报警。
- **相邻同体分支**：相邻 `if/elif` 分支的归一化 AST 执行体完全相同时，报 `warning`（`category = symmetry`），提示用 `or`/`||` 合并条件；跨 `else` 不重排，含 `typeof`/`isinstance`/`:=` 等类型收窄或绑定条件时跳过。
- **重复字符串分发（Repeated Dispatch）**：同文件内出现 ≥ 2 处长 `if/elif` 字符串等值链（≥ 4 分支且共享 ≥ 3 个相同字符串 key）时，报 `warning`（`category = symmetry`）并提示收口为单一注册表或策略表。

### BCS017　纯度契约、污点传播与模块分层 `[AUTO-CORE]`

- 显式标记 `bits:spec(pure)` 的函数，或匹配 `bits.toml` 中 `[[layers]]` 且 `pure = true` 层的模块内所有函数，不得直接或经调用链间接调用 IO 污点代码，违者报**硬错误**（`category = role`，`obligation = refactor`）。
- 可在 `bits.toml` 中通过 `[[layers]]` 声明 `name`、`paths`、`pure`、`forbid_imports`（禁止导入的层名或模块 glob）和 `allow_imports`（白名单层名）；违反跨层导入边界直接报 `BCS017` **硬错误**。
- 内置常见 IO 调用与模块（`open`、`print`、`fetch`、`os.*`、`subprocess.*`、`httpx.*`、`std::fs::*`、`*.read_text` 等，含 `import os as o` 别名与 `from os import getenv` 绑定）；项目可在 `bits.toml` 用 `io_sources = ["Client.request", "db.*"]` 追加 glob 源头。
- 宁漏勿错：不自动推断 `pure`；跨文件只沿真实 import 绑定传播；无法静态确认接收者类型的方法调用（如 `client.get()`）默认不误报，需通过 `io_sources` 显式声明。

## 3. 抽象与边界约束 `[REVIEW]`（三段式）

### 3.1 接口税测试：新抽象必须减少调用者知识
- **反例形状**：`def parse(req, ctx): return _parse(req, ctx.schema, ctx.seen, ctx.out)` —— 抽出的单调用 helper 签名几乎复述原函数，靠搬运 `ctx` 或 out-param 维持连续决策。
- **维护代价**：读者要同时理解两套签名、共享状态和调用顺序，复杂性只是被转移而非被封藏。
- **合理例外**：公共稳定 API、ABI/FFI、框架强制签名、真实多实现接缝、独立可命名的纯领域运算。**判据**：把 helper 内联回唯一调用者后，调用者具体新增了哪项知识或不变量？答不出就内联。
- **Anti-fix**：把 helper 改名为 `impl/adapter` 或补一段复述代码的 docstring 来证明抽象价值。

### 3.2 契约气闸与最小 `try`：区分运行时错误与内部不变量
- **反例形状**：`assert request.token`（把外部输入缺失当程序员 Bug）；`try` 块包裹了大段不会抛目标异常的日志/序列化逻辑；`while True:` 无限重试且没有耗尽错误。
- **维护代价**：外部可预期失败变成含混崩溃；大 `try` 误吞无关异常；永久故障变成悬挂请求。
- **合理例外**：测试断言；内部已证明的不变量；服务器事件循环以 EOF/取消为终点。透传层只校验自身不变量（保留键、权限、结构边界），下游参数契约交下游报错并在边界透传/包装原始错误（保留 `cause`/`source`）。
- **Anti-fix**：为过关批量加 `from None` 隐藏根因，或机械补 `assert True`。

### 3.3 单源分发与最小能力签名：不二次路由，不索取多余所有权
- **反例形状**：`argparse` 注册了 `command="run"` 又写 `if args.command == "run"`；只读数据却索取所有权再内部 `clone()`；往 `out` 参数里塞本应返回的新集合。
- **维护代价**：同一事实维护两份必然漂移；伪所有权强迫调用方复制；隐藏 out-param 让数据流向和失败路径难以推导。
- **合理例外**：框架不支持绑定时用单一注册表；调用方缓冲区复用（如 `Read::read`）、真实原地修改、FFI/trait 固定签名。优先直接调用，重复到第三次且模式稳定再抽象；继承默认不超过 2 层且必须满足 is-a。
- **Anti-fix**：为躲参数数量硬套 Builder/functional options，或为躲 `clone` 提示套一层无收益的 `Cow`/泛型。

本节不参与自动 PASS/FAIL，由 L2 独立 Critic 聚焦 Diff 热区审查。

## 4. 外部豁免与稀缺举证（`bits:allow` vs `bits:prove`）

**Warning 不要求补注释。** 只有当确实无法消除形状时，才允许登记举证，且作者不能自己给自己盖章。

### 4.1 `bits:allow`：仅限外部硬契约

```text
bits:allow(BCS004): 框架回调签名固定
bits:allow(BCS010): FFI 要求保留原始符号名
```

- 必须包含规则号和理由，只作用于当前行或下一行（`BCS004` 覆盖完整函数签名）。
- 仅用于 ABI、FFI、框架强制签名/命名、纯数据表或受 `__all__` 管理的公开导出等外部契约。
- 禁止文件级、目录级 blanket suppression。**未压中任何告警的 `bits:allow` 直接报 `BCS015` 硬错误要求删除。**

### 4.2 `bits:prove`：稀缺技术债登记

```text
bits:prove(BCS003, test=test_lock): cursor 与 checksum 共享一把锁；拆分会中途释放锁
bits:prove(BCS016): check 内部依赖分支求值顺序；提升会改变初始化时序
```

- 必须用 `;` 或 `；` 分隔为**两段**：为什么这是最简形式；删掉/改简单后的具体风险。
- 高风险边界举证必须附 `test=<fn_or_Class_method>`。扫描器通过 AST 索引真实测试定义（Python `def test_*` / `Test*.test_*`、TS `test()/it()`、Rust `#[test] fn`、Go `func Test*` / `Benchmark*`，含 diff 下未变更的测试文件）；**不存在或有歧义直接报 `BCS015` 硬错误**。
- **不能豁免任何硬错误**；作用于 warning 时，将其标记为 `obligation = critic_review`，交由**独立 Critic Agent 或人**核验，写代码的 Agent 不能自审通过。
- **配额**：`--diff` 模式下每次变更最多新增 **1** 条 `bits:prove`，超出直接报 `BCS015` 硬错误。
- **过期即删**：未命中任何活跃 warning 的 `bits:prove` 直接报 `BCS015` 硬错误。
- **封死后门**：`bits:explain(BCS003)` 仅用于存量代码全量扫描兼容；**`--diff` 模式下新增 `bits:explain` 直接报 `BCS015` 硬错误**，必须使用 `bits:prove`。

生成代码和第三方代码通过扫描配置排除，不使用豁免注释；formatter-off 区域也必须使用规则号和理由，并立即恢复。

## 5. 生成代码、测试与存量代码

- 生成代码和第三方代码默认排除。
- 测试代码适用同样的语义规则。
- 存量代码使用 baseline：不得新增违规，新文件必须全量通过。
- 使用 `bits style report --output bits-baseline.json` 生成基线，再使用 `bits style check --baseline bits-baseline.json`。
- baseline 只隐藏与既有规则、文件、消息和实际值完全匹配的违规；移动代码行号不会自动产生新违规。
- **`improving` 棘轮**：在 `--diff` 模式下，按 `(函数名, 同名出现序号)` 匹配基线 commit 中的已存在函数（不受行号移动影响，区分同名方法）；若仍超出 `BCS003` 嵌套、圈复杂度或认知复杂度观测阈值，但对应指标**严格下降**，则标记 `improving = true`、`obligation = ratchet_improving`，降级为 warning 不阻断增量提交。指标持平/上升、改名或新增函数不享受棘轮，仍为硬错误。
