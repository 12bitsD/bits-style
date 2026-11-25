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

### 0.2 优雅的四个可计算维度

| 维度 | 优雅定义 | 不优雅信号 | L2 Critic 追问 |
|---|---|---|---|
| **层级纯度** | 一个函数只在一个抽象高度说话 | 高层编排与底层拼装/局部 import 混杂 | 有没有哪几行突然比周围低了一个层级？ |
| **控制流平直度** | 主路一条直线，分支只做真实决策 | 箭头嵌套、分支模拟查表、框架已分发又二次分发 | 这些分支是在做业务决策，还是在重复表/框架的工作？ |
| **信息增益** | 只为本层决策解析，状态都被消费 | 镜像下游校验、带原文兜底的猜测解析、只写不读字段 | 解析/校验出的值驱动了哪个分支？删掉会出什么具体事故？ |
| **对称性** | 同样的事只用一种方式表达 | 互斥分支重复贴前置调用、同类逻辑两套写法、运行时 `assert` 糊弄类型 | 同一个前置操作能否无副作用地合并为一处？ |

### 0.3 三层漏斗

- **L0 卫生层（外部工具）**：`ruff` / `mypy` / `clippy` / `tsc` / formatter 负责语法、格式、重复导入、未使用变量和 `assert`。Bits 不重复实现。
- **L1 拓扑层（`bits` AST）**：只抓不依赖变量名、不猜业务语义的结构事实，负责硬门禁与圈定嫌疑函数（`BCS001`–`BCS016` 的确定性子集）。
- **L2 品味层（Bits Skill + 独立 Critic）**：只聚焦**本次 Diff 中改动较大/新增分支的函数**与 **L1 报出 Warning 的函数**，用四个维度追问；无问题保持沉默，不输出表扬与空话。

### 0.4 新规则准入（Eval-Driven）

任何新 L1 规则上线前必须同时通过：
1. **正样本召回**：`corpus/bad/` 中真实反面案例 `before` 命中、`after` 消除。
2. **开源盲测**：在锁定 commit 的 `click` / `httpx` / `rich` 等基准库上，硬错误 **0 误报**，Warning 误报率 **< 1%**（`bits bench`）。
3. **Agent 对抗测试**：Agent 修复后代码更接近 `after`，而非拆碎函数或编造 `bits:prove`。

## 1. 规则类型与适用范围

- `[AUTO-CORE]`：跨语言、可机器判定的语义规则，影响扫描退出码。
- `[AUTO-PROFILE]`：可机器判定，但具体形式由语言 Profile 定义，影响扫描退出码。
- `[REVIEW]`：需要设计判断，不参与自动 PASS/FAIL。

语言 Profile 定义缩进、括号、命名、续行、资源清理和工具配置。编译器、ABI、FFI、框架重写方法及回调强制要求的签名和命名优先于本规范，但必须使用第 5 节的行级豁免。

当前 Profile：C/C++、Python、JavaScript/TypeScript/TSX、Rust、Go。Rust 为 partial 支持，Go 目前只有规范 Profile，扫描器支持状态以 `docs/language-support.md` 为准。

基础语法和语言官方工具已经覆盖的问题不在 Profile 中重复检查。Bits 的底层目标是让代码和逻辑简单到不容易出错，优先约束错误路径、状态、资源、并发和抽象成本。

## 2. 规则

### BCS001　缩进与空白 `[AUTO-PROFILE]`

- 文件必须以一个换行符结束，禁止行尾空白，禁止混用 Tab 和 Space 缩进。
- C/C++：Tab 缩进，`TabWidth = 8`、`IndentWidth = 8`。
- Python、Rust：4 个 Space 缩进。
- JavaScript/TypeScript/TSX：2 个 Space 缩进。
- 禁止源码内 editor modeline。

### BCS002　行宽与续行 `[AUTO-PROFILE]`

- 默认每行不超过 80 列，Profile 可以调整行宽。
- 续行缩进以及运算符放行首或行尾由 Profile 和 formatter 统一决定。
- 不得为了满足行宽拆分用户可见字符串、日志或错误信息、URL。
- 上述不可拆分文本是唯一的超宽例外。

### BCS003　函数规模 `[AUTO-CORE]`

每个函数同时检查四项指标：

- 非空、非注释代码行不超过 48 行。纯字符串内容和多行类型声明不计入函数实现行数，只产生 warning。
- 控制流嵌套不超过 3 层，属于确定的 violation。
- 局部变量不超过 10 个；`int a, b;` 计两个，参数不计，只产生 warning。
- 圈复杂度不超过 10，属于确定的 violation。扁平 `switch/match` 的 `case` 不增加圈复杂度，但分支内的其他判断仍然计数。

行数和局部变量数超过阈值产生 `warning`，不直接阻断 CI。真实嵌套和圈复杂度超过阈值仍属于 violation。优先按阶段拆分新增或正在修改的函数，不按行数机械切段，也不为了达标拆散内聚逻辑。

确实必须保持内聚时，在函数前添加解释，说明具体机制、必须一起维护的共享约束，以及拆分会破坏的不变量：

```text
bits:explain(BCS003): <具体机制>；<共享状态/资源/顺序约束>；<具体拆分风险>
```

报告中的 `explanation` 字段保存该说明；文本报告在缺少说明时给出补充提示。`bits:explain` 只用于行数和局部变量 warning 的最后手段说明，**不能豁免嵌套或圈复杂度硬错误**，也不能替代本可进行的重构：

- 禁止空泛套话（如 `single table`、`too complex to split`、`split scatters rules`），必须写出具体的状态、资源或顺序约束。
- 如果分支链实际可以用声明式 Schema、数据表或删除下游镜像校验消除，应直接重构或删除，不使用解释注释保留。纯数据表和声明式配置可使用 `bits:allow(BCS003): 理由` 豁免。

### BCS004　函数接口 `[AUTO-CORE]`

- 参数不超过 4 个，不包含语言隐含的 `this/self`，超过产生 warning。对象字段和响应数据中的布尔值不属于行为开关。
- 禁止无默认值的位置布尔参数切换行为（如 `run(enabled: boolean)`）；带默认值的具名配置项或 keyword-only 选项（如 `follow_redirects: bool = False`）不属于布尔陷阱。React 组件的展示型布尔 props 由 React Profile 自动识别。
- 分离的函数声明必须包含参数名。
- 外部框架、接口、回调和 FFI 强制的签名可以通过行级豁免保留。
- 不得使用无约束的 `map/dict/options` 打包互不相关的参数来规避限制 `[REVIEW]`。

### BCS005　控制流 `[AUTO-CORE]`

- 异常路径使用提前 `return/throw`，主流程保持靠左。
- `return/throw` 之后不得保留无意义的 `else`。
- C++、Rust 及其他以 RAII、defer、with 管理资源的语言禁止 `goto`。
- C 中 `goto` 只能向前跳转，标签必须匹配 `out_*` 或 `err_*`，禁止 `err1/err2`。
- 是否应通过查表替代分支 `[REVIEW]`。

### BCS006　语句与表达式 `[AUTO-CORE]`

- 每行最多一条语句，禁止一行多个赋值和链式赋值。
- 禁止在条件表达式内赋值；语言提供的结构化条件初始化语法由 Profile 决定。
- 链式赋值按真实 assignment target 判断，函数调用的关键字参数不属于链式赋值。
- 禁止使用逗号表达式代替大括号。
- 禁止嵌套三元表达式和双重否定条件。
- 混合不同优先级的运算符时必须加括号。
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

- 禁止新增可变全局变量、Singleton 和隐式上下文输入。
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

- 禁止空异常处理和静默吞错；解析、守卫等场景的预期 `false/null` 失败结果只产生低置信 warning，不与伪造成功混为硬错误。
- 必须忽略清理路径错误时，使用 Profile 规定的显式丢弃写法，禁止空 `catch/except`。
- catch-all 只能继续抛出，或在系统边界转换为明确的错误类型；无法静态确认边界的转换只产生低置信 `warning`。
- 非 void、非单元返回值必须检查或显式丢弃 `[AUTO-PROFILE]`。
- 是否构成“伪造的成功结果”的复杂业务语义 `[REVIEW]`。

### BCS014　依赖与无用代码 `[AUTO-CORE]`

- 模块运行时依赖必须无环；`TYPE_CHECKING` / `import type` 仅类型边不进入运行时环图，自环边忽略。Python 绝对导入按 `__init__.py` 包根解析，不将标准库同名模块误判为包内模块。
- 禁止未使用的导入、依赖、变量、函数和不可达代码；Python 类型注解与类型别名中的字符串前向引用（如 `x: "Console"`、`Union["URL", str]`）及 `# noqa: F401` 显式导出计为真实引用，普通字符串（如 `VALUE = "os"`）不计。
- 禁止通配符导入（`__init__.py` 中 `from .mod import *` 相对公开导出除外），禁止保留注释掉的旧实现。
- 接口或回调的未使用参数必须使用 Profile 规定的 `_`、`_name` 或 `[[maybe_unused]]` 等显式标记。

### BCS015　注释与源码元数据 `[AUTO-CORE]`

- TODO 必须使用 `TODO(owner): 明确的完成条件` 格式；条件是否足够明确 `[REVIEW]`。
- 禁止 editor modeline 和通过源码注释覆盖团队统一格式。
- 对齐的数据表格可以使用受控 formatter-off 区域，但必须包含规则号和理由，并在表格结束后立即恢复。
- 注释应解释原因、约束和例外，不复述代码 `[REVIEW]`。
- 未命中任何告警的 `bits:allow` / `bits:prove` 属于过期墓碑注释，必须删除（硬错误）。
- `bits:prove` 不能豁免硬错误，且在 `--diff` 模式下每次变更最多新增 1 条（硬错误）。

### BCS016　互斥分支头部公共前置重复 `[AUTO-CORE]`

- 带兜底 `else` 的穷尽 `if/elif/else` 链，且所有分支条件无副作用时，若每个分支的首条语句都是完全相同的独立调用（如 `await harness.check()`），产生 `warning`（`category = symmetry`，`obligation = prove_minimalism`）。
- 将该调用提升到分支之前语义等价；若各分支实际在做命令路由，优先改为注册表或 `set_defaults(handler=...)` 直接绑定处理函数。
- 无 `else` 的非穷尽分支、含副作用条件（如条件中调用函数）或首条调用不一致时不报警。

### BCS017　纯度契约与污点传播 `[AUTO-CORE]`

- 显式标记 `bits:spec(pure)` 的函数不得直接或经调用链间接调用 IO 污点代码，违者报**硬错误**（`category = role`，`obligation = refactor`）。
- 内置常见 IO 调用与模块（`open`、`print`、`fetch`、`os.*`、`subprocess.*`、`httpx.*`、`std::fs::*`、`*.read_text` 等，含 `import os as o` 别名与 `from os import getenv` 绑定）；项目可在 `bits.toml` 用 `io_sources = ["Client.request", "db.*"]` 追加 glob 源头。
- 宁漏勿错：不自动推断 `pure`；跨文件只沿真实 import 绑定传播；无法静态确认接收者类型的方法调用（如 `client.get()`）默认不误报，需通过 `io_sources` 显式声明。

## 3. 抽象约束 `[REVIEW]`

- 优先直接调用。重复模式稳定后再抽象，通常第三次重复才值得认真考虑。
- 只有一个实现的接口不一定是问题：外部系统边界、插件、ABI 和测试替身可以例外；没有边界目的时应删除。
- 禁止没有新增语义、错误处理、资源管理或组合价值的纯转发包装层。
- 继承深度默认不超过 2；组合优先于继承。
- 数据结构、默认值和类型约束能否消除特殊分支，优先于继续添加判断。
- 透传层只校验自身拥有的事实和不变量（如保留字段、权限、结构边界）。下游 SDK、模型或服务已有参数契约时，不用 `if/elif` 镜像其取值范围和互斥规则；交由下游校验，并在系统边界透传或包装原始错误。
- 确实需要在本地提前失败或归一化时，使用声明式 Schema（如 Pydantic、Zod）或查表，不用同一变量对多个字符串字面量做 `if/elif` 分派。

本节不参与自动 PASS/FAIL。

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
- 高风险边界举证必须附 `test=<test_name>`，指向验证该失效场景的测试。
- **不能豁免任何硬错误**；作用于 warning 时，将其标记为 `obligation = critic_review`，交由**独立 Critic Agent 或人**核验，写代码的 Agent 不能自审通过。
- **配额**：`--diff` 模式下每次变更最多新增 **1** 条 `bits:prove`，超出直接报 `BCS015` 硬错误。
- **过期即删**：未命中任何活跃 warning 的 `bits:prove` 直接报 `BCS015` 硬错误。
- `bits:explain(BCS003)` 作为 V0.3 兼容字段保留，新代码统一使用 `bits:prove`。

生成代码和第三方代码通过扫描配置排除，不使用豁免注释；formatter-off 区域也必须使用规则号和理由，并立即恢复。

## 5. 生成代码、测试与存量代码

- 生成代码和第三方代码默认排除。
- 测试代码适用同样的语义规则。
- 存量代码使用 baseline：不得新增违规，新文件必须全量通过。
- 使用 `bits style report --output bits-baseline.json` 生成基线，再使用 `bits style check --baseline bits-baseline.json`。
- baseline 只隐藏与既有规则、文件、消息和实际值完全匹配的违规；移动代码行号不会自动产生新违规。
- **`improving` 棘轮**：在 `--diff` 模式下，若已存在函数仍超出 `BCS003` 嵌套或圈复杂度阈值，但同名函数相对基线 commit 的对应指标**严格下降**，则该条诊断标记为 `improving = true`、`obligation = ratchet_improving`，降级为 warning 不阻断增量提交。复杂度持平/上升、或新增函数不享受棘轮，仍为硬错误。
