//! Static verifier.
//!
//! The verifier is a *pure function* of `(bytecode, Config)`: it holds no
//! interior state, reads no environment, clock, randomness or I/O, and uses
//! no hash-based containers whose iteration order could leak into the result.
//! Therefore the accept/reject verdict for a given bytecode is fully
//! deterministic across runs and machines.
//!
//! Loop policy: **loops are forbidden**. Every jump (conditional or
//! unconditional) must target a strictly later instruction (`off >= 0`,
//! target = pc + 1 + off). Termination follows trivially: each instruction
//! strictly increases the program counter, and the program is finite.

use crate::isa::{self, Class, Insn, AluOp, INSN_SIZE, REG_COUNT, REG_FP};

/// Static + dynamic limits for a VM instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// Maximum number of instructions a program may contain.
    pub max_program_len: usize,
    /// Maximum number of instructions executed before the VM halts
    /// with `RunError::BudgetExceeded`.
    pub max_insns: u64,
    /// Size in bytes of the VM-owned stack (accessed via R10).
    pub stack_size: usize,
    /// Size in bytes of the linear memory region the program may access.
    pub mem_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_program_len: 4096,
            max_insns: 100_000,
            stack_size: 512,
            mem_size: 4096,
        }
    }
}

/// All verification failures. Each variant carries the program counter of
/// the offending instruction so diagnostics are deterministic too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// Bytecode length is not a multiple of the instruction size.
    Truncated { len: usize },
    /// Program has no instructions.
    Empty,
    /// Program exceeds `Config::max_program_len`.
    ProgramTooLarge { len: usize, max: usize },
    /// Unknown opcode.
    BadOpcode { pc: usize, opc: u8 },
    /// Register index >= REG_COUNT.
    BadRegister { pc: usize, reg: u8 },
    /// Attempt to write the read-only frame pointer R10.
    WriteToFramePointer { pc: usize },
    /// R10 used where only general-purpose registers are allowed
    /// (as ALU source, or as jump operand).
    FramePointerMisuse { pc: usize },
    /// Jump target outside `[0, program_len)`.
    JumpOutOfBounds { pc: usize, target: i64 },
    /// Backward or self jump (loops are forbidden by policy).
    BackwardJump { pc: usize, target: i64 },
    /// Jump offset escapes the 16-bit encoding range.
    JumpOffsetOverflow { pc: usize },
    /// Program does not terminate with EXIT.
    MissingExit,
    /// EXIT appears before the end of the program, leaving unreachable
    /// trailing instructions (rejected to keep control flow unambiguous).
    UnreachableCode { pc: usize },
    /// Stack access outside `[-stack_size, 0)` relative to R10.
    StackOutOfBounds { pc: usize, off: i16, size: usize },
    /// Division or modulo by an immediate zero (detectable statically).
    DivByZeroImm { pc: usize },
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for VerifyError {}

fn check_reg(pc: usize, reg: u8) -> Result<(), VerifyError> {
    if reg >= REG_COUNT {
        return Err(VerifyError::BadRegister { pc, reg });
    }
    Ok(())
}

fn check_writable(pc: usize, reg: u8) -> Result<(), VerifyError> {
    if reg == REG_FP {
        return Err(VerifyError::WriteToFramePointer { pc });
    }
    Ok(())
}

/// Verify `code` against `cfg`. On success returns the decoded instruction
/// stream (guaranteed safe to execute by `vm::Vm` without any access
/// escaping the VM-owned memory regions).
pub fn verify(code: &[u8], cfg: &Config) -> Result<Vec<Insn>, VerifyError> {
    if code.len() % INSN_SIZE != 0 {
        return Err(VerifyError::Truncated { len: code.len() });
    }
    let n = code.len() / INSN_SIZE;
    if n == 0 {
        return Err(VerifyError::Empty);
    }
    if n > cfg.max_program_len {
        return Err(VerifyError::ProgramTooLarge { len: n, max: cfg.max_program_len });
    }

    let mut insns = Vec::with_capacity(n);
    for pc in 0..n {
        let raw = &code[pc * INSN_SIZE..(pc + 1) * INSN_SIZE];
        // Length checked above; decode cannot fail here.
        let insn = Insn::decode(raw).ok_or(VerifyError::Truncated { len: code.len() })?;
        check_insn(pc, n, &insn, cfg)?;
        insns.push(insn);
    }

    // Structural control-flow checks.
    if !matches!(isa::classify(insns[n - 1].opc), Some(Class::Exit)) {
        return Err(VerifyError::MissingExit);
    }
    for (pc, insn) in insns.iter().enumerate().take(n - 1) {
        if matches!(isa::classify(insn.opc), Some(Class::Exit)) {
            return Err(VerifyError::UnreachableCode { pc: pc + 1 });
        }
    }

    Ok(insns)
}

fn check_insn(pc: usize, n: usize, insn: &Insn, cfg: &Config) -> Result<(), VerifyError> {
    let class = isa::classify(insn.opc)
        .ok_or(VerifyError::BadOpcode { pc, opc: insn.opc })?;

    match class {
        Class::AluImm(op) => {
            check_reg(pc, insn.dst)?;
            check_writable(pc, insn.dst)?;
            // Deterministic static check: div/mod by immediate zero.
            if matches!(op, AluOp::Div | AluOp::Mod) && insn.imm == 0 {
                return Err(VerifyError::DivByZeroImm { pc });
            }
        }
        Class::AluReg(_) => {
            check_reg(pc, insn.dst)?;
            check_reg(pc, insn.src)?;
            check_writable(pc, insn.dst)?;
            if insn.src == REG_FP {
                return Err(VerifyError::FramePointerMisuse { pc });
            }
        }
        Class::Ja | Class::JCondImm(_) | Class::JCondReg(_) => {
            if matches!(class, Class::JCondReg(_)) {
                check_reg(pc, insn.src)?;
                if insn.src == REG_FP {
                    return Err(VerifyError::FramePointerMisuse { pc });
                }
            }
            if !matches!(class, Class::Ja) {
                check_reg(pc, insn.dst)?;
                if insn.dst == REG_FP {
                    return Err(VerifyError::FramePointerMisuse { pc });
                }
            }
            check_jump(pc, n, insn)?;
        }
        Class::Exit => {}
        Class::Load(size) => {
            check_reg(pc, insn.dst)?;
            check_reg(pc, insn.src)?;
            check_writable(pc, insn.dst)?;
            if insn.src == REG_FP {
                check_stack(pc, insn.off, size, cfg)?;
            }
        }
        Class::StoreReg(size) => {
            check_reg(pc, insn.dst)?;
            check_reg(pc, insn.src)?;
            if insn.dst == REG_FP {
                check_stack(pc, insn.off, size, cfg)?;
            }
        }
        Class::StoreImm(size) => {
            check_reg(pc, insn.dst)?;
            if insn.dst == REG_FP {
                check_stack(pc, insn.off, size, cfg)?;
            }
        }
    }
    Ok(())
}

/// Loop policy enforcement + jump target validation.
fn check_jump(pc: usize, n: usize, insn: &Insn) -> Result<(), VerifyError> {
    let target = pc as i64 + 1 + insn.off as i64;
    // Loops are forbidden: only strictly forward jumps are accepted.
    if target <= pc as i64 {
        return Err(VerifyError::BackwardJump { pc, target });
    }
    if target < 0 || target >= n as i64 {
        return Err(VerifyError::JumpOutOfBounds { pc, target });
    }
    Ok(())
}

/// Stack slots live at `[fp - stack_size, fp)`; `off` is relative to fp.
fn check_stack(pc: usize, off: i16, size: usize, cfg: &Config) -> Result<(), VerifyError> {
    let start = off as i64;
    let end = start + size as i64;
    if start < -(cfg.stack_size as i64) || end > 0 {
        return Err(VerifyError::StackOutOfBounds { pc, off, size });
    }
    Ok(())
}
