# Language Support

本矩阵是 V0.2 扫描器能力的公开边界。`STYLE.md` 中的规则定义不因此缩减。

- **supported**：对应语法有通过和失败测试，实现该规则定义的确定性检查。
- **partial**：只实现列出的保守子集，未实现部分不得被理解为通过。
- **not_applicable**：语言没有对应语法。
- **not_implemented**：尚未实现；报告不会将其列为已执行。

## 总表

| 规则 | C / C++ | Python | JavaScript / TypeScript / TSX | 当前判定边界 |
|---|---|---|---|---|
| `BCS001` | partial | partial | partial | 禁止混用及错误缩进字符，处理行尾空白和末尾换行；续行与连续块宽度未完整判定 |
| `BCS002` | partial | partial | partial | 默认 80 列；字符串字面量保守识别用户文本、日志和 URL；续行由 Profile formatter 决定 |
| `BCS003` | partial | partial | partial | 行数和局部变量为 warning；真实嵌套和圈复杂度为 violation；多行类型声明不计行数；支持 `bits:explain` |
| `BCS004` | partial | partial | partial | 参数数量为 warning；直接布尔行为参数保留检查，对象布尔字段跳过；签名豁免覆盖整个函数签名 |
| `BCS005` | partial | partial | partial | 冗余 else；C 限制 goto，C++ 禁止 goto；查表属于 REVIEW |
| `BCS006` | partial | partial | partial | 按 assignment target 判断链式赋值；关键字参数不误报；嵌套三元和双重否定保留检查 |
| `BCS007` | supported | not_applicable | supported | 所有控制块必须加大括号；C/C++ 函数 `{` 下一行，JS/TS 同行 |
| `BCS008` | partial | not_applicable | partial | C/C++ case 对齐 switch；JS/TS 使用一级缩进；fallthrough 采用保守检查 |
| `BCS009` | partial | partial | TSX 为低置信 warning；其他为 partial | 运算符空白按 AST 左右节点判断；多行 closing ) 不报 padding；JSX 区域交给 formatter |
| `BCS010` | partial | partial | partial | 命名为 warning；模块级 const 同时接受两种命名；Pydantic/JSON 字段、HTTP 回调和 React/memo 组件自动豁免 |
| `BCS011` | partial | partial | partial | 可变全局、直接环境读取和真实参数重赋值；类型别名、对象属性更新和普通类型注解变量不误报 |
| `BCS012` | partial | not_applicable | not_applicable | C/C++ 宏、inline 和函数内条件编译；宏的全部调用方语义不完整 |
| `BCS013` | C++ partial；C 异常语法 N/A、非 void 返回值检查未实现 | partial | partial | 空 catch 为硬错误；预期 false/null 失败结果和边界转换为 warning；复杂业务错误语义属于 REVIEW |
| `BCS014` | partial | partial | partial | 依赖环、不可达代码为硬错误；type-only import、模板插值和 `__all__` re-export 计为真实引用；注释旧代码为 warning |
| `BCS015` | supported | supported | supported | TODO 格式、行级豁免格式、editor modeline；注释内容属于 REVIEW |

## 文件扩展名

| Language | Extensions |
|---|---|
| C | `.c`, `.h` |
| C++ | `.cc`, `.cpp`, `.cxx`, `.hh`, `.hpp`, `.hxx` |
| Python | `.py`, `.pyi` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| TypeScript | `.ts`, `.mts`, `.cts` |
| TSX | `.tsx` |

Rust 目前只有规范 Profile，没有扫描适配。Go 已有独立的[语义规范](../profiles/go.md)，但同样没有扫描适配。这些文件会进入报告的 `unsupported` 列表：`report` 明确展示，`check` 返回 2，避免形成“没有覆盖却 passed”。其他语言尚未声明支持。

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
