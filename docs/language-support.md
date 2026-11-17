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
| `BCS003` | partial | partial | partial | 非空非注释代码行、嵌套、局部绑定和圈复杂度；扁平 case 豁免；复杂声明可能只覆盖部分绑定 |
| `BCS004` | partial | partial | partial | 参数数量、布尔参数和 C/C++ 参数名；外部签名使用行级豁免 |
| `BCS005` | partial | partial | partial | 冗余 else；C 限制 goto，C++ 禁止 goto；查表属于 REVIEW |
| `BCS006` | partial | partial | partial | 每行语句、条件/链式赋值、逗号表达式、嵌套三元、双重否定；混合运算符覆盖仍不完整 |
| `BCS007` | supported | not_applicable | supported | 所有控制块必须加大括号；C/C++ 函数 `{` 下一行，JS/TS 同行 |
| `BCS008` | partial | not_applicable | partial | C/C++ case 对齐 switch；JS/TS 使用一级缩进；fallthrough 采用保守检查 |
| `BCS009` | partial | partial | partial | 常见关键字、调用、括号和二元运算符空格；其余交给 Profile formatter |
| `BCS010` | partial | partial | partial | C/Python 使用 lower_snake_case；JS/TS 使用 lowerCamelCase；类型 PascalCase、常量 UPPER_SNAKE_CASE |
| `BCS011` | partial | partial | partial | 常见可变全局及容器常量、直接环境读取、直接参数重赋值和部分参数遮蔽；完整作用域与全部隐藏输入不自动判定 |
| `BCS012` | partial | not_applicable | not_applicable | C/C++ 宏、inline 和函数内条件编译；宏的全部调用方语义不完整 |
| `BCS013` | C++ partial；C 异常语法 N/A、非 void 返回值检查未实现 | partial | partial | 空 catch/except、常见伪造成功、catch-all 重抛或边界转换；复杂业务错误语义属于 REVIEW |
| `BCS014` | partial | partial | partial | 通配符/简单未使用导入、相对依赖环、不可达代码和疑似注释旧代码；不是完整语义可达性分析 |
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

Rust 目前只有规范 Profile，没有扫描适配。Go 已有独立的[语义规范](../profiles/go.md)，但同样没有扫描适配；Go 文件不会进入当前扫描结果，不应被描述为已经通过 Bits 的自动检查。其他语言尚未声明支持。

## 依赖图边界

- JavaScript/TypeScript：解析相对路径 import，并尝试常见扩展名和 index 文件。
- Python：只对可解析到已发现文件的相对模块路径建立边。
- C/C++：相对头文件 include 可形成依赖边。
- 外部包、构建条件、宏生成 include 和动态 import 不参与完整依赖证明。

## 报告覆盖

`coverage.executed` 表示规则至少在一个成功解析的语言适配器中执行；`coverage.partial` 表示这里列出的保守子集；`coverage.not_applicable` 表示不适用语法；`coverage.not_implemented` 表示没有任何已执行实现。

因此，`status = passed` 的准确含义是：**当前语言适配器实际执行的检查均通过**，不是对 REVIEW 规则或所有无法静态判定的语义作出证明。
