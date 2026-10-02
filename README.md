# sat-solver

一个完全本地运行的 SAT 求解器：核心 DPLL 算法基于 Python 标准库自行实现，
不调用 Z3、MiniSat 或任何云求解服务。所有 CNF 输入、求解状态与模型只存在
于本地文件或内存中。

## 目录结构

- `sat_solver/dimacs.py` — 严格校验的 DIMACS CNF 解析器
- `sat_solver/dpll.py` — DPLL 求解器（unit propagation、pure literal
  elimination、DLIS 分支启发式、回溯统计）
- `sat_solver/__main__.py` — 命令行入口
- `tests/test_sat_solver.py` — 自动化测试（含穷举 oracle 对照）

## 使用方法

    python3 -m sat_solver path/to/formula.cnf

输出示例：

    s SATISFIABLE
    v 1 2 -3 0
    c stats: decisions=0 propagations=3 pure_literals=0 backtracks=0 max_depth=0

退出码遵循 SAT 竞赛惯例：`10` = SAT，`20` = UNSAT，`1` = 输入格式错误。
UNSAT 只在整个搜索树被穷尽后返回，绝不通过超时猜测。

## 输入格式（DIMACS CNF）

    c 注释行以 c 开头
    p cnf <变量数> <子句数>
    1 -2 0
    2 3 0

解析器严格校验：

- `p cnf` 头部必须恰好出现一次，且位于所有子句之前；
- 每个 literal 必须是非零整数，绝对值在 `1..变量数` 范围内；
- 每个子句必须以 `0` 结尾，文件中途结束（子句未闭合或数量不足）报错；
- 子句数量必须与头部声明完全一致；声明的子句之后只允许注释、空行和
  一个可选的 `%` 文件结束标记，其余内容一律报错。

确定语义：

- 空公式（0 个子句）→ SAT，模型中所有变量默认 `False`（可任意扩展）；
- 空子句（单独的 `0`）→ UNSAT；
- 子句内重复 literal 自动去重；
- tautology 子句（同时含 `x` 与 `-x`）恒真，直接丢弃。

## DPLL 流程

`DpllSolver._dpll` 的每个递归节点执行：

1. **Unit propagation（单元传播）**：反复找出长度为 1 的子句，将其唯一
   literal 赋真并化简公式，直到不动点；若导出空子句则产生冲突，当前分支
   失败。每次赋值计入 `propagations`。
2. **Pure literal elimination（纯文字消除）**：在剩余子句中只以单一极性
   出现的变量直接按该极性赋值（计入 `pure_literals`），随后回到第 1 步。
3. **分支决策**：使用 DLIS 启发式——统计每个变量在剩余子句中的正/负出现
   次数，选取总出现次数最多的变量（平局取编号最小者保证确定性），先尝试
   出现次数较多的极性。每次分支计入 `decisions`。
4. **回溯**：某一分支失败则撤销该分支的所有赋值，尝试相反极性；两极性
   均失败则向上一层返回冲突。每次失败分支计入 `backtracks`，同时维护
   `max_depth`（最深决策层数）。

SAT 时返回完整模型（`{变量编号: bool}`），未被搜索赋值的变量补 `False`；
可用 `sat_solver.verify_model(clauses, model)` 重新验证所有子句。UNSAT 时
模型为 `None`。

## 运行测试

在仓库根目录执行：

    python3 -m unittest discover -s tests -v

测试覆盖：

- SAT / UNSAT 基本用例与模型重验证；
- unit propagation 链、pure literal elimination；
- 深度回溯（鸽笼原理 PHP(4,3) UNSAT、PHP(3,3) SAT、首分支冲突后回溯）；
- DIMACS 各类错误（缺头部、非法 literal、越界变量、子句未闭合、子句数量
  不符、文件尾部垃圾、重复头部、`%` 标记误用等）；
- 300 个固定种子的随机小公式，与穷举全部赋值的 oracle 逐一比对结果。
