//! The restricted bytecode virtual machine.
//!
//! All state lives in process memory: a fixed register file and a fixed-size
//! stack. The VM never touches the Linux eBPF subsystem, the network, or any
//! external service. Memory accesses are bounds-checked at runtime (defense
//! in depth on top of static verification), so a verified program can never
//! read or write outside the VM-owned stack.

use crate::isa::{Instruction, Op};
use crate::verifier::Config;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmError {
    /// Executed more instructions than the configured budget allows.
    BudgetExceeded { budget: u64 },
    /// Division or modulo by zero.
    DivisionByZero,
    /// Program counter left the program without hitting EXIT.
    /// (Unreachable for verified programs; defensive.)
    PcOutOfBounds { pc: usize },
    /// Memory access outside the VM stack.
    /// (Unreachable for verified programs; defensive.)
    StackOutOfBounds { slot: i64 },
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::BudgetExceeded { budget } => {
                write!(f, "instruction budget of {budget} exceeded")
            }
            VmError::DivisionByZero => write!(f, "division by zero"),
            VmError::PcOutOfBounds { pc } => write!(f, "pc {pc} out of bounds"),
            VmError::StackOutOfBounds { slot } => write!(f, "stack slot {slot} out of bounds"),
        }
    }
}

impl std::error::Error for VmError {}

/// A VM instance. Registers and stack are owned by the VM; nothing outside
/// this struct is ever read or written by a running program.
pub struct Vm {
    regs: Vec<i64>,
    stack: Vec<i64>,
    budget: u64,
    executed: u64,
}

impl Vm {
    pub fn new(cfg: &Config) -> Vm {
        Vm {
            regs: vec![0; cfg.num_registers],
            stack: vec![0; cfg.stack_words],
            budget: cfg.max_instructions,
            executed: 0,
        }
    }

    pub fn registers(&self) -> &[i64] {
        &self.regs
    }

    pub fn stack(&self) -> &[i64] {
        &self.stack
    }

    pub fn executed_instructions(&self) -> u64 {
        self.executed
    }

    /// Run a verified program to completion. Returns the value of r0.
    pub fn run(&mut self, program: &[Instruction]) -> Result<i64, VmError> {
        let mut pc: usize = 0;
        loop {
            if self.executed >= self.budget {
                return Err(VmError::BudgetExceeded {
                    budget: self.budget,
                });
            }
            let ins = program.get(pc).ok_or(VmError::PcOutOfBounds { pc })?;
            self.executed += 1;

            let mut next_pc = pc + 1;
            match ins.op {
                Op::MovImm => self.regs[ins.dst as usize] = ins.imm as i64,
                Op::MovReg => self.regs[ins.dst as usize] = self.regs[ins.src as usize],
                Op::AddImm => {
                    self.regs[ins.dst as usize] =
                        self.regs[ins.dst as usize].wrapping_add(ins.imm as i64)
                }
                Op::AddReg => {
                    self.regs[ins.dst as usize] = self.regs[ins.dst as usize]
                        .wrapping_add(self.regs[ins.src as usize])
                }
                Op::SubImm => {
                    self.regs[ins.dst as usize] =
                        self.regs[ins.dst as usize].wrapping_sub(ins.imm as i64)
                }
                Op::SubReg => {
                    self.regs[ins.dst as usize] = self.regs[ins.dst as usize]
                        .wrapping_sub(self.regs[ins.src as usize])
                }
                Op::MulImm => {
                    self.regs[ins.dst as usize] =
                        self.regs[ins.dst as usize].wrapping_mul(ins.imm as i64)
                }
                Op::MulReg => {
                    self.regs[ins.dst as usize] = self.regs[ins.dst as usize]
                        .wrapping_mul(self.regs[ins.src as usize])
                }
                Op::DivImm => {
                    let divisor = ins.imm as i64;
                    if divisor == 0 {
                        return Err(VmError::DivisionByZero);
                    }
                    self.regs[ins.dst as usize] =
                        self.regs[ins.dst as usize].wrapping_div(divisor)
                }
                Op::DivReg => {
                    let divisor = self.regs[ins.src as usize];
                    if divisor == 0 {
                        return Err(VmError::DivisionByZero);
                    }
                    self.regs[ins.dst as usize] =
                        self.regs[ins.dst as usize].wrapping_div(divisor)
                }
                Op::ModImm => {
                    let divisor = ins.imm as i64;
                    if divisor == 0 {
                        return Err(VmError::DivisionByZero);
                    }
                    self.regs[ins.dst as usize] =
                        self.regs[ins.dst as usize].wrapping_rem(divisor)
                }
                Op::ModReg => {
                    let divisor = self.regs[ins.src as usize];
                    if divisor == 0 {
                        return Err(VmError::DivisionByZero);
                    }
                    self.regs[ins.dst as usize] =
                        self.regs[ins.dst as usize].wrapping_rem(divisor)
                }
                Op::Neg => self.regs[ins.dst as usize] = self.regs[ins.dst as usize].wrapping_neg(),
                Op::Ja => next_pc = jump_target(pc, ins.imm),
                Op::Jeq => {
                    if self.regs[ins.dst as usize] == self.regs[ins.src as usize] {
                        next_pc = jump_target(pc, ins.imm);
                    }
                }
                Op::Jne => {
                    if self.regs[ins.dst as usize] != self.regs[ins.src as usize] {
                        next_pc = jump_target(pc, ins.imm);
                    }
                }
                Op::Jgt => {
                    if self.regs[ins.dst as usize] > self.regs[ins.src as usize] {
                        next_pc = jump_target(pc, ins.imm);
                    }
                }
                Op::Jge => {
                    if self.regs[ins.dst as usize] >= self.regs[ins.src as usize] {
                        next_pc = jump_target(pc, ins.imm);
                    }
                }
                Op::Jlt => {
                    if self.regs[ins.dst as usize] < self.regs[ins.src as usize] {
                        next_pc = jump_target(pc, ins.imm);
                    }
                }
                Op::Jle => {
                    if self.regs[ins.dst as usize] <= self.regs[ins.src as usize] {
                        next_pc = jump_target(pc, ins.imm);
                    }
                }
                Op::Ldw => {
                    let slot = ins.imm as i64;
                    let value = self
                        .stack
                        .get(slot as usize)
                        .copied()
                        .ok_or(VmError::StackOutOfBounds { slot })?;
                    self.regs[ins.dst as usize] = value;
                }
                Op::Stw => {
                    let slot = ins.imm as i64;
                    let cell = self
                        .stack
                        .get_mut(slot as usize)
                        .ok_or(VmError::StackOutOfBounds { slot })?;
                    *cell = self.regs[ins.src as usize];
                }
                Op::Exit => return Ok(self.regs[0]),
            }
            pc = next_pc;
        }
    }
}

fn jump_target(pc: usize, offset: i32) -> usize {
    (pc as i64 + 1 + offset as i64) as usize
}

/// Convenience helper: verify bytecode, then execute it.
pub fn verify_and_run(bytes: &[u8], cfg: &Config) -> Result<i64, RunFailure> {
    let program = crate::verifier::verify(bytes, cfg).map_err(RunFailure::Verify)?;
    let mut vm = Vm::new(cfg);
    vm.run(&program).map_err(RunFailure::Runtime)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunFailure {
    Verify(crate::verifier::VerifyError),
    Runtime(VmError),
}

impl std::fmt::Display for RunFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunFailure::Verify(e) => write!(f, "verification failed: {e}"),
            RunFailure::Runtime(e) => write!(f, "runtime error: {e}"),
        }
    }
}

impl std::error::Error for RunFailure {}
