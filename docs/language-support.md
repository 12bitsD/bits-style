# Language Support

本矩阵是扫描器能力的公开边界。`STYLE.md` 中的规则定义不因此缩减。

状态含义：

- **supported**：对应语法有通过和失败测试，实现该规则定义的确定性检查。
- **partial**：只实现列出的保守子集，未实现部分不得被理解为通过。
- **not_applicable**：语言没有对应语法。
- **not_implemented**：尚未实现；报告不会将其列为已执行。

## 总表

| 规则 | C / C++ | Python | JavaScript / TypeScript / TSX | 当前判定边界 |
|---|---|---|---|---|
| `BCS001` | supported | supported | supported | Tab、缩进、行尾空白、末尾换行；自动修复避开多行字符串 |
| `BCS002` | partial | partial | partial | 80 列；字符串字面量用于保守识别不可拆分的用户可见字符串 |
| `BCS003` | partial | partial | partial | 函数行数、嵌套、局部绑定和圈复杂度；复杂声明或解构可能只覆盖部分绑定 |
| `BCS004` | partial | partial | partial | 参数数量、布尔参数和 C/C++ 参数名；未完整判定 options 打包规避 |
| `BCS005` | partial | partial | partial | 冗余 else；C/C++ 另检查 goto 名称与方向；查表重构语义不自动判定 |
| `BCS006` | partial | partial | partial | 每行语句、条件赋值、链式赋值和逗号表达式；one-liner 语义只覆盖可判定形式 |
| `BCS007` | supported | not_applicable | supported | 大括号位置、单语句分支、if/else 一致性 |
| `BCS008` | partial | not_applicable | partial | case 对齐和显式 fallthrough；复杂控制流可能无法静态证明 |
| `BCS009` | partial | partial | partial | 常见关键字、调用、括号和二元运算符空格；不完整推断所有运算符语义 |
| `BCS010` | partial | partial | partial | 函数、变量、类型和常量命名；类型别名的语义白名单未完整实现 |
| `BCS011` | partial | partial | partial | 常见可变全局、直接环境读取和显式 Singleton 形式；生命周期与全部隐藏输入不自动判定 |
| `BCS012` | partial | not_applicable | not_applicable | C/C++ 宏、inline 和函数内条件编译；宏的全部调用方语义不完整 |
| `BCS013` | C++ partial；C not_applicable | partial | partial | 空 catch/except 和常见伪造成功返回；完整错误等价关系不自动判定 |
| `BCS014` | partial | partial | partial | 通配符/简单未使用导入、相对依赖环、不可达代码和疑似注释旧代码；不是完整语义可达性分析 |
| `BCS015` | supported | supported | supported | TODO 格式、editor modeline、源码内格式覆盖 |

## 文件扩展名

| Language | Extensions |
|---|---|
| C | `.c`, `.h` |
| C++ | `.cc`, `.cpp`, `.cxx`, `.hh`, `.hpp`, `.hxx` |
| Python | `.py`, `.pyi` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| TypeScript | `.ts`, `.mts`, `.cts` |
| TSX | `.tsx` |

Go 和其他语言尚未声明支持。其文件不会进入当前扫描结果，也不应被描述为已经通过 Bits’s Coding Style。

## 依赖图边界

- JavaScript/TypeScript：解析相对路径 import，并尝试常见扩展名和 index 文件。
- Python：当前只对可解析到已发现文件的相对模块路径建立边。
- C/C++：相对头文件 include 可形成依赖边。
- 外部包、构建系统条件、宏生成 include 和动态 import 不参与完整依赖证明。

## 报告覆盖

`coverage.executed` 表示规则至少在一个成功解析的语言适配器中执行；`coverage.partial` 表示这里列出的保守子集；`coverage.not_applicable` 表示不适用语法；`coverage.not_implemented` 表示没有任何已执行实现。

因此，`status = passed` 的准确含义是：**当前语言适配器实际执行的检查均通过**，不是对所有无法静态判定的语义作出证明。
