# Language Support

本矩阵是 V0.4 扫描器能力的公开边界。`STYLE.md` 中的规则定义不因此缩减；L2 品味审查（四个优雅维度）由 Agent Skill 执行，不在静态扫描覆盖范围内。

- **supported**：对应语法有通过和失败测试，实现该规则定义的确定性检查。
- **partial**：只实现列出的保守子集，未实现部分不得被理解为通过。
- **not_applicable**：语言没有对应语法。
- **not_implemented**：尚未实现；报告不会将其列为已执行。

## 总表

| 规则 | C / C++ | Python | JavaScript / TypeScript / TSX | 当前判定边界 |
|---|---|---|---|---|
| `BCS001` | partial | partial | partial | 禁止混用及错误缩进字符，处理行尾空白和末尾换行；续行与连续块宽度未完整判定 |
| `BCS002` | partial | partial | partial | 默认 80 列；字符串字面量保守识别用户文本、日志和 URL；续行由 Profile formatter 决定 |
| `BCS003` | partial | partial | partial | 行数和局部变量为 warning；嵌套（已统一 `else if`/`elif` 深度）和圈复杂度为 violation；`cognitive_v1 > 15` 输出 shadow warning；存量 `bits:explain` 兼容，diff 新增禁用 |
| `BCS004` | partial | partial | partial | 参数数量为 warning；仅无默认值的位置布尔参数判为行为开关，带默认值配置项与 keyword-only 选项跳过；签名豁免覆盖整个函数签名 |
| `BCS005` | partial | partial | partial | 冗余 else；C 限制 goto，C++ 禁止 goto；无 `elif/else` 的双层嵌套 `if` 报可折叠 warning；查表属于 REVIEW |
| `BCS006` | partial | partial | partial | 链式赋值按 target 判断；嵌套三元和双重否定为硬错误；Python/C/JS 断言条件含 `:=`/`=`/`++` 等显式副作用报硬错误（普通调用不报） |
| `BCS007` | supported | not_applicable | supported | 所有控制块必须加大括号；C/C++ 函数 `{` 下一行，JS/TS 同行 |
| `BCS008` | partial | not_applicable | partial | C/C++ case 对齐 switch；JS/TS 使用一级缩进；fallthrough 采用保守检查 |
| `BCS009` | partial | partial | TSX 为低置信 warning；其他为 partial | 运算符空白按 AST 左右节点判断；多行 closing ) 不报 padding；JSX 区域交给 formatter |
| `BCS010` | partial | partial | partial | 命名为 warning；模块级 const 同时接受两种命名；Pydantic/JSON 字段、HTTP 回调和 React/memo 组件自动豁免 |
| `BCS011` | partial | partial | partial | 可变全局、直接环境读取和真实参数重赋值；类型别名（含 `typing.*`）、带注解大写常量、模块级 `logger`、`__main__` 入口变量、`except ImportError` 回退、对象属性更新不误报 |
| `BCS012` | partial | not_applicable | not_applicable | C/C++ 宏、inline 和函数内条件编译；宏的全部调用方语义不完整 |
| `BCS013` | C++ partial；C 异常语法 N/A、非 void 返回值检查未实现 | partial | partial | 空 catch 为硬错误；预期 false/null 失败、边界转换、纯重抛 `try` 与 `raise var`/`throw var` 冗余重抛为 warning |
| `BCS014` | partial | partial | partial | 运行时依赖环、不可达代码为硬错误；`TYPE_CHECKING`/type-only import 不进运行时环图，Python 绝对导入按包根解析；字符串前向类型注解、`noqa: F401`、`__init__.py` 相对导出、Rust `pub use *` 与 `#[cfg(test)] use super::*` 合法 |
| `BCS015` | supported | supported | supported | TODO 格式、`allow`/`prove` 格式、墓碑注释硬错误、`test=X` AST 唯一性校验、diff `prove` 配额（≤1）与 diff 新增 `explain` 禁令、editor modeline |
| `BCS016` | partial | partial | partial | 穷尽分支头部重复同一前置调用报 warning；相邻 `if/elif` 执行体 AST 完全一致报 warning（跳过 `typeof`/`isinstance`/`:=` 收窄）；Python `match` 暂未覆盖 |
| `BCS017` | partial | partial | partial | `bits:spec(pure)` 函数直接或经同文件/真实 import 调用链触碰内置或 `io_sources` 配置的 IO 源报硬错误；无法静态确认接收者的方法调用默认不报 |

## 文件扩展名

| Language | Extensions |
|---|---|
| C | `.c`, `.h` |
| C++ | `.cc`, `.cpp`, `.cxx`, `.hh`, `.hpp`, `.hxx` |
| Python | `.py`, `.pyi` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| TypeScript | `.ts`, `.mts`, `.cts` |
| TSX | `.tsx` |

Rust 为 partial 支持，已接入 Tree-sitter，支持函数、参数、命名、match、可变全局、use/依赖、未使用导入和 Result/unwrap 的保守检查。Go 目前只有规范 Profile，没有扫描适配；这些文件会进入报告的 `unsupported` 列表：`report` 明确展示，`check` 返回 2，避免形成“没有覆盖却 passed”。其他语言尚未声明支持。

## TypeScript 动态 import 兼容

当前 Tree-sitter 对 `importOriginal<typeof import(...)>(...)` 和 `import('...').SomeType` 两类合法 TypeScript 形式存在解析差异。扫描器在内存中等长屏蔽泛型参数、以 `any` 替换动态类型引用，不修改磁盘源码，不影响违规行列号。运行时动态 import 不做该处理。该兼容路径有真实项目回归测试，仍属于 partial 支持。

## 依赖图边界

- JavaScript/TypeScript：解析相对路径 import，并尝试常见扩展名和 index 文件。
- Python：只对可解析到已发现文件的相对模块路径建立边。
- C/C++：相对头文件 include 可形成依赖边。
- 外部包、构建条件、宏生成 include 和动态 import 不参与完整依赖证明。

## 报告覆盖

`coverage.executed` 表示规则至少在一个成功解析的语言适配器中执行；`coverage.partial` 表示这里列出的保守子集；`coverage.not_applicable` 表示不适用语法；`coverage.not_implemented` 表示没有任何已执行实现。

因此，`status = passed` 的准确含义是：**当前语言适配器实际执行的检查均通过**，不是对 REVIEW 规则或所有无法静态判定的语义作出证明。
