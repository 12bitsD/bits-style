# Bits’s Coding Style V0.3

> 正确 > 清晰 > 可修改 > 一致 > 简短

本规范只约束代码本身，不包含 MVC、设计模式或评审流程。

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

确实必须保持内聚时，在函数前添加解释，说明职责、不可拆分的原因以及拆分会破坏的约束：

```text
bits:explain(BCS003): <职责>；<必须保持内聚的原因>；<拆分风险>
```

报告中的 `explanation` 字段保存该说明。解释只能用于最后手段，不能替代本可进行的重构。纯数据表和声明式配置仍可使用 `bits:allow(BCS003)` 豁免。

### BCS004　函数接口 `[AUTO-CORE]`

- 参数不超过 4 个，不包含语言隐含的 `this/self`，超过产生 warning。对象字段和响应数据中的布尔值不属于行为开关。
- 禁止布尔参数；React 组件的展示型布尔 props 由 React Profile 自动识别，不需要逐行豁免。
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
- 类型别名不是可变全局状态；`Annotated/Literal/Callable` 等按类型表达式处理。
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

- 模块依赖必须无环。
- 禁止未使用的导入、依赖、变量、函数和不可达代码。
- 禁止通配符导入，禁止保留注释掉的旧实现。
- 接口或回调的未使用参数必须使用 Profile 规定的 `_`、`_name` 或 `[[maybe_unused]]` 等显式标记。

### BCS015　注释与源码元数据 `[AUTO-CORE]`

- TODO 必须使用 `TODO(owner): 明确的完成条件` 格式；条件是否足够明确 `[REVIEW]`。
- 禁止 editor modeline 和通过源码注释覆盖团队统一格式。
- 对齐的数据表格可以使用受控 formatter-off 区域，但必须包含规则号和理由，并在表格结束后立即恢复。
- 注释应解释原因、约束和例外，不复述代码 `[REVIEW]`。

## 3. 抽象约束 `[REVIEW]`

- 优先直接调用。重复模式稳定后再抽象，通常第三次重复才值得认真考虑。
- 只有一个实现的接口不一定是问题：外部系统边界、插件、ABI 和测试替身可以例外；没有边界目的时应删除。
- 禁止没有新增语义、错误处理、资源管理或组合价值的纯转发包装层。
- 继承深度默认不超过 2；组合优先于继承。
- 数据结构、默认值和类型约束能否消除特殊分支，优先于继续添加判断。

本节不参与自动 PASS/FAIL。

## 4. 行级豁免

统一格式：

```text
bits:allow(BCS004): 框架回调签名固定
bits:allow(BCS010): FFI 要求保留原始符号名
```

- 必须包含规则号和理由，只作用于当前行或下一行。
- 禁止文件级、目录级 blanket suppression。
- 生成代码和第三方代码通过扫描配置排除，不使用豁免注释。
- formatter-off 区域也必须使用规则号和理由，并立即恢复。

## 5. 生成代码、测试与存量代码

- 生成代码和第三方代码默认排除。
- 测试代码适用同样的语义规则。
- 存量代码使用 baseline：不得新增违规，新文件必须全量通过。
- 使用 `bits style report --output bits-baseline.json` 生成基线，再使用 `bits style check --baseline bits-baseline.json`。
- baseline 只隐藏与既有规则、文件、消息和实际值完全匹配的违规；移动代码行号不会自动产生新违规。
