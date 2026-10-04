# restricted-vm

一个受限字节码虚拟机及配套静态 verifier，纯 Rust 实现。所有程序、字节码、
执行状态和验证结果只保存在本地内存或本地文件中：**不依赖 Linux eBPF 子系统、
不依赖远程执行环境、不访问任何外部服务**。

## 结构

- `src/isa.rs` — 指令集定义、编解码
- `src/verifier.rs` — 静态 verifier（纯函数，确定性判定）
- `src/vm.rs` — 虚拟机解释器（寄存器 + 栈，预算限制）
- `src/bin/vm_test_runner.rs` — 独立测试 runner

## ISA

定长 8 字节指令，小端编码：

```
byte 0    : opcode
byte 1    : dst 寄存器编号
byte 2    : src 寄存器编号
byte 3..7 : imm (i32, 小端)
byte 7    : 保留（须为 0）
```

寄存器：`r0..r9` 共 10 个 64 位通用寄存器，`r0` 为返回值寄存器。

| Opcode | 指令 | 语义 |
|--------|------|------|
| 0x01 | MOV_IMM | `dst = imm` |
| 0x02 | MOV_REG | `dst = src` |
| 0x03/0x04 | ADD_IMM/ADD_REG | `dst += imm/src`（wrapping） |
| 0x05/0x06 | SUB_IMM/SUB_REG | `dst -= imm/src`（wrapping） |
| 0x07/0x08 | MUL_IMM/MUL_REG | `dst *= imm/src`（wrapping） |
| 0x09/0x0A | DIV_IMM/DIV_REG | `dst /= imm/src`（wrapping；除零为运行时错误） |
| 0x0B/0x0C | MOD_IMM/MOD_REG | `dst %= imm/src`（wrapping；除零为运行时错误） |
| 0x0D | NEG | `dst = -dst`（wrapping） |
| 0x10 | JA | `pc += imm`（无条件跳转，imm 为指令偏移） |
| 0x11..0x16 | JEQ/JNE/JGT/JGE/JLT/JLE | `if dst <op> src { pc += imm }`（有符号比较） |
| 0x20 | LDW | `dst = stack[imm]`（imm 为栈字索引） |
| 0x21 | STW | `stack[imm] = src` |
| 0xFF | EXIT | 停机，返回 `r0` |

算术统一采用 wrapping 语义（包括 `i64::MIN / -1` 回绕为 `i64::MIN`），
除零/取模零触发 `DivisionByZero` 运行时错误。

## 内存模型

VM 实例独占两块内存，程序无法触及除此之外的任何地址：

- 寄存器文件：10 × i64
- 栈：64 字 × 8 字节 = 512 字节，按字索引（`LDW`/`STW` 的 imm 即槽位号）

栈访问采用立即数寻址，槽位号在字节码中静态可见，因此越界可在验证期
完全排除；运行时仍做防御性边界检查（`.get()`/`.get_mut()`），即使
verifier 被绕过也不会发生 VM 分配范围之外的内存访问。

## 验证规则

`verify(bytecode, config)` 依次检查：

1. **格式**：长度非零且为 8 的倍数；指令数不超过 `max_program_len`。
2. **opcode**：未知 opcode 拒绝（`IllegalOpcode`）。
3. **寄存器**：指令实际使用的 `dst`/`src` 必须 `< num_registers`。
4. **栈边界**：`LDW`/`STW` 的 imm 必须满足 `0 <= imm < stack_words`。
5. **跳转**：目标 `pc + 1 + imm` 必须在程序范围内，且**严格大于 pc**
   （见下节循环策略）。
6. **终止性**：最后一条指令必须是 `EXIT`。

### 循环策略：禁止循环

所有跳转（含条件跳转）只允许向前。由于最后一条指令是 `EXIT` 且跳转
目标不越界，任何通过验证的程序最多执行 `program_len` 条指令后必然停机，
不存在死循环的可能——这是静态保证，而非运行时检测。

### 确定性

verifier 是 `(bytecode, config)` 的纯函数：不做 I/O、不读时钟和环境
变量、只按索引顺序遍历切片。同一字节码在任何执行环境下得到完全相同的
接受/拒绝结果。

## Instruction budget

`Config::max_instructions`（默认 100 000）限制单条程序最多执行的指令数。
运行时每执行一条指令计数一次，超限返回 `BudgetExceeded` 错误。由于循环
被禁止，正常程序远不会触及该上限；budget 作为纵深防御，保证异常程序
（例如绕过 verifier 直接送入解释器）也不会无限运行。

## 测试

不使用 `cargo test`，使用独立 runner 逐项输出 PASS/FAIL：

```
cargo run --bin vm_test_runner
```

覆盖：合法程序（算术/栈/分支）、非法 opcode、越界寄存器、栈越界
load/store、非法跳转（越界目标、向后跳转/自跳转死循环）、缺少 EXIT、
畸形字节码（长度不对齐、空程序）、instruction budget 超限、除零、
算术边界（`i64::MIN - 1` 回绕、`i64::MIN / -1`）、内存隔离（遍历全部
合法槽位并校验 VM 状态）以及 verifier 确定性（重复验证结果一致）。
任一测试失败时进程以非零码退出。
