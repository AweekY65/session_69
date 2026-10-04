//! The restricted virtual machine.
//!
//! Memory model:
//! - 11 registers (`u64`). R0 holds the return value; R10 is a read-only
//!   frame pointer usable only as the base of load/store instructions.
//! - A VM-owned stack of `Config::stack_size` bytes, addressed as
//!   `[fp - stack_size, fp)` (negative offsets from R10).
//! - A caller-provided linear memory region of `Config::mem_size` bytes,
//!   addressed by general-purpose registers (`0..mem_size`).
//!
//! Every load/store is bounds-checked at runtime against exactly one of
//! these two regions; there is no code path that can read or write outside
//! VM-owned memory. The VM never touches the host file system, network,
//! or any OS-specific subsystem (no eBPF, no JIT, no external services).

use crate::isa::{AluOp, Class, Cond, Insn, REG_FP, REG_RET};
use crate::verifier::Config;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunError {
    /// Instruction budget exhausted before EXIT.
    BudgetExceeded,
    /// Division or modulo by zero (register operand).
    DivByZero { pc: usize },
    /// Shift amount >= 64.
    ShiftOverflow { pc: usize, shift: u64 },
    /// Memory access outside the VM-owned regions.
    MemOutOfBounds { pc: usize, addr: i64, size: usize },
    /// Control flow left the program without hitting EXIT
    /// (unreachable for verified programs).
    FellOff { pc: usize },
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for RunError {}

pub struct Vm<'a> {
    code: Vec<Insn>,
    cfg: Config,
    regs: [u64; 11],
    stack: Vec<u8>,
    mem: &'a mut [u8],
}

impl<'a> Vm<'a> {
    /// Create a VM for a *verified* program. `mem` must be exactly
    /// `cfg.mem_size` bytes; it is the only linear memory the program can
    /// reach through general-purpose registers.
    pub fn new(code: Vec<Insn>, cfg: Config, mem: &'a mut [u8]) -> Self {
        assert_eq!(
            mem.len(),
            cfg.mem_size,
            "memory region must match Config::mem_size"
        );
        Vm {
            code,
            cfg,
            regs: [0; 11],
            stack: vec![0u8; cfg.stack_size],
            mem,
        }
    }

    /// Execute until EXIT (returning R0) or a runtime fault.
    /// Executes at most `cfg.max_insns` instructions.
    pub fn run(&mut self) -> Result<u64, RunError> {
        let mut pc: usize = 0;
        let mut budget = self.cfg.max_insns;

        loop {
            if pc >= self.code.len() {
                return Err(RunError::FellOff { pc });
            }
            if budget == 0 {
                return Err(RunError::BudgetExceeded);
            }
            budget -= 1;

            let insn = self.code[pc];
            let class = crate::isa::classify(insn.opc)
                .expect("verified program contains only known opcodes");

            let mut next_pc = pc + 1;
            match class {
                Class::AluImm(op) => {
                    let v = self.alu(pc, op, self.regs[insn.dst as usize], insn.imm as i64 as u64)?;
                    self.regs[insn.dst as usize] = v;
                }
                Class::AluReg(op) => {
                    let src = self.regs[insn.src as usize];
                    let v = self.alu(pc, op, self.regs[insn.dst as usize], src)?;
                    self.regs[insn.dst as usize] = v;
                }
                Class::Ja => {
                    next_pc = (pc as i64 + 1 + insn.off as i64) as usize;
                }
                Class::JCondImm(cond) => {
                    let lhs = self.regs[insn.dst as usize];
                    let rhs = insn.imm as i64 as u64;
                    if eval_cond(cond, lhs, rhs) {
                        next_pc = (pc as i64 + 1 + insn.off as i64) as usize;
                    }
                }
                Class::JCondReg(cond) => {
                    let lhs = self.regs[insn.dst as usize];
                    let rhs = self.regs[insn.src as usize];
                    if eval_cond(cond, lhs, rhs) {
                        next_pc = (pc as i64 + 1 + insn.off as i64) as usize;
                    }
                }
                Class::Exit => return Ok(self.regs[REG_RET as usize]),
                Class::Load(size) => {
                    let addr = self.load_addr(&insn, pc, size)?;
                    let v = match insn.src {
                        REG_FP => read_le(&self.stack, addr, size),
                        _ => read_le(self.mem, addr, size),
                    };
                    self.regs[insn.dst as usize] = v;
                }
                Class::StoreReg(size) => {
                    let addr = self.store_addr(&insn, pc, size)?;
                    let v = self.regs[insn.src as usize];
                    match insn.dst {
                        REG_FP => write_le(&mut self.stack, addr, size, v),
                        _ => write_le(self.mem, addr, size, v),
                    }
                }
                Class::StoreImm(size) => {
                    let addr = self.store_addr(&insn, pc, size)?;
                    let v = insn.imm as i64 as u64;
                    match insn.dst {
                        REG_FP => write_le(&mut self.stack, addr, size, v),
                        _ => write_le(self.mem, addr, size, v),
                    }
                }
            }
            pc = next_pc;
        }
    }

    /// Address for a load: base register is `src`.
    fn load_addr(&self, insn: &Insn, pc: usize, size: usize) -> Result<usize, RunError> {
        if insn.src == REG_FP {
            self.stack_addr(insn.off, size, pc)
        } else {
            let base = self.regs[insn.src as usize] as i64;
            self.mem_addr(base + insn.off as i64, size, pc)
        }
    }

    /// Address for a store: base register is `dst`.
    fn store_addr(&self, insn: &Insn, pc: usize, size: usize) -> Result<usize, RunError> {
        if insn.dst == REG_FP {
            self.stack_addr(insn.off, size, pc)
        } else {
            let base = self.regs[insn.dst as usize] as i64;
            self.mem_addr(base + insn.off as i64, size, pc)
        }
    }

    /// Stack access: fp-relative, `[fp - stack_size, fp)`. The verifier
    /// already guarantees this statically; the runtime re-checks so safety
    /// never depends on the verifier being invoked.
    fn stack_addr(&self, off: i16, size: usize, pc: usize) -> Result<usize, RunError> {
        let start = self.cfg.stack_size as i64 + off as i64;
        if start < 0 || start as usize + size > self.stack.len() {
            return Err(RunError::MemOutOfBounds { pc, addr: start, size });
        }
        Ok(start as usize)
    }

    /// Linear-memory access, bounds-checked against the VM-owned region.
    fn mem_addr(&self, addr: i64, size: usize, pc: usize) -> Result<usize, RunError> {
        if addr < 0 {
            return Err(RunError::MemOutOfBounds { pc, addr, size });
        }
        let a = addr as usize;
        if a > self.mem.len() || size > self.mem.len() - a {
            return Err(RunError::MemOutOfBounds { pc, addr, size });
        }
        Ok(a)
    }

    fn alu(&self, pc: usize, op: AluOp, lhs: u64, rhs: u64) -> Result<u64, RunError> {
        Ok(match op {
            AluOp::Add => lhs.wrapping_add(rhs),
            AluOp::Sub => lhs.wrapping_sub(rhs),
            AluOp::Mul => lhs.wrapping_mul(rhs),
            AluOp::Or => lhs | rhs,
            AluOp::And => lhs & rhs,
            AluOp::Xor => lhs ^ rhs,
            AluOp::Neg => lhs.wrapping_neg(),
            AluOp::Mov => rhs,
            AluOp::Div => {
                if rhs == 0 {
                    return Err(RunError::DivByZero { pc });
                }
                lhs / rhs
            }
            AluOp::Mod => {
                if rhs == 0 {
                    return Err(RunError::DivByZero { pc });
                }
                lhs % rhs
            }
            AluOp::Lsh => {
                if rhs >= 64 {
                    return Err(RunError::ShiftOverflow { pc, shift: rhs });
                }
                lhs << rhs
            }
            AluOp::Rsh => {
                if rhs >= 64 {
                    return Err(RunError::ShiftOverflow { pc, shift: rhs });
                }
                lhs >> rhs
            }
            AluOp::Arsh => {
                if rhs >= 64 {
                    return Err(RunError::ShiftOverflow { pc, shift: rhs });
                }
                ((lhs as i64) >> rhs) as u64
            }
        })
    }
}

fn eval_cond(cond: Cond, lhs: u64, rhs: u64) -> bool {
    match cond {
        Cond::Eq => lhs == rhs,
        Cond::Ne => lhs != rhs,
        Cond::Gt => lhs > rhs,
        Cond::Ge => lhs >= rhs,
        Cond::Lt => lhs < rhs,
        Cond::Le => lhs <= rhs,
        Cond::Set => lhs & rhs != 0,
        Cond::Sgt => (lhs as i64) > (rhs as i64),
        Cond::Sge => (lhs as i64) >= (rhs as i64),
        Cond::Slt => (lhs as i64) < (rhs as i64),
        Cond::Sle => (lhs as i64) <= (rhs as i64),
    }
}

fn read_le(buf: &[u8], addr: usize, size: usize) -> u64 {
    let mut v: u64 = 0;
    for i in 0..size {
        v |= (buf[addr + i] as u64) << (8 * i);
    }
    v
}

fn write_le(buf: &mut [u8], addr: usize, size: usize, v: u64) {
    for i in 0..size {
        buf[addr + i] = (v >> (8 * i)) as u8;
    }
}
