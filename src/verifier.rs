//! Static verifier.
//!
//! The verifier is a pure function of (bytecode, config): it performs no I/O,
//! reads no clocks or environment variables, and iterates only over
//! index-ordered slices, so its accept/reject verdict is fully deterministic
//! for a given input regardless of the execution environment.
//!
//! Loop policy: loops are *forbidden*. Every jump must target a strictly
//! later instruction (forward-only). Combined with the requirement that the
//! final instruction is `EXIT`, this statically guarantees termination in at
//! most `program_len` executed instructions.

use crate::isa::{Instruction, Op, INSTRUCTION_SIZE, NUM_REGISTERS, STACK_WORDS};

/// Verification / execution limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Maximum number of instructions a program may execute at runtime.
    pub max_instructions: u64,
    /// Number of registers the program may use.
    pub num_registers: usize,
    /// Number of 8-byte stack words the program may address.
    pub stack_words: usize,
    /// Maximum number of instructions in a program.
    pub max_program_len: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_instructions: 100_000,
            num_registers: NUM_REGISTERS,
            stack_words: STACK_WORDS,
            max_program_len: 4096,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// Bytecode length is zero or not a multiple of the instruction size.
    Malformed { reason: String },
    /// Program exceeds the configured maximum instruction count.
    ProgramTooLong { len: usize, max: usize },
    /// Unknown opcode byte.
    IllegalOpcode { pc: usize, byte: u8 },
    /// Register index out of range.
    RegisterOutOfBounds { pc: usize, reg: u8 },
    /// Stack slot index out of range.
    StackOutOfBounds { pc: usize, slot: i32 },
    /// Jump target outside the program.
    JumpOutOfBounds { pc: usize, target: i64 },
    /// Backward or self jump (loops are forbidden).
    BackwardJump { pc: usize, target: i64 },
    /// Last instruction is not EXIT.
    MissingExit,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifyError::Malformed { reason } => write!(f, "malformed bytecode: {reason}"),
            VerifyError::ProgramTooLong { len, max } => {
                write!(f, "program too long: {len} instructions (max {max})")
            }
            VerifyError::IllegalOpcode { pc, byte } => {
                write!(f, "illegal opcode 0x{byte:02x} at instruction {pc}")
            }
            VerifyError::RegisterOutOfBounds { pc, reg } => {
                write!(f, "register r{reg} out of bounds at instruction {pc}")
            }
            VerifyError::StackOutOfBounds { pc, slot } => {
                write!(f, "stack slot {slot} out of bounds at instruction {pc}")
            }
            VerifyError::JumpOutOfBounds { pc, target } => {
                write!(f, "jump from {pc} to out-of-bounds target {target}")
            }
            VerifyError::BackwardJump { pc, target } => {
                write!(f, "backward/self jump from {pc} to {target} (loops forbidden)")
            }
            VerifyError::MissingExit => write!(f, "last instruction is not EXIT"),
        }
    }
}

impl std::error::Error for VerifyError {}

/// Verify raw bytecode and return the decoded program on success.
///
/// Deterministic: same (bytecode, config) always yields the same verdict.
pub fn verify(bytes: &[u8], cfg: &Config) -> Result<Vec<Instruction>, VerifyError> {
    if bytes.is_empty() {
        return Err(VerifyError::Malformed {
            reason: "empty program".to_string(),
        });
    }
    if bytes.len() % INSTRUCTION_SIZE != 0 {
        return Err(VerifyError::Malformed {
            reason: format!(
                "length {} is not a multiple of instruction size {}",
                bytes.len(),
                INSTRUCTION_SIZE
            ),
        });
    }
    let len = bytes.len() / INSTRUCTION_SIZE;
    if len > cfg.max_program_len {
        return Err(VerifyError::ProgramTooLong {
            len,
            max: cfg.max_program_len,
        });
    }

    let mut program = Vec::with_capacity(len);
    for pc in 0..len {
        let raw = &bytes[pc * INSTRUCTION_SIZE..(pc + 1) * INSTRUCTION_SIZE];
        let op_byte = raw[0];
        let ins = Instruction::decode(raw).ok_or(VerifyError::IllegalOpcode {
            pc,
            byte: op_byte,
        })?;
        check_instruction(pc, &ins, len, cfg)?;
        program.push(ins);
    }

    if program.last().map(|i| i.op) != Some(Op::Exit) {
        return Err(VerifyError::MissingExit);
    }

    Ok(program)
}

fn check_instruction(
    pc: usize,
    ins: &Instruction,
    program_len: usize,
    cfg: &Config,
) -> Result<(), VerifyError> {
    if ins.op.uses_dst() && (ins.dst as usize) >= cfg.num_registers {
        return Err(VerifyError::RegisterOutOfBounds { pc, reg: ins.dst });
    }
    if ins.op.uses_src() && (ins.src as usize) >= cfg.num_registers {
        return Err(VerifyError::RegisterOutOfBounds { pc, reg: ins.src });
    }
    if ins.op.is_mem() {
        let slot = ins.imm;
        if slot < 0 || slot as usize >= cfg.stack_words {
            return Err(VerifyError::StackOutOfBounds { pc, slot });
        }
    }
    if ins.op.is_jump() {
        let target = pc as i64 + 1 + ins.imm as i64;
        if target <= pc as i64 {
            return Err(VerifyError::BackwardJump { pc, target });
        }
        if target >= program_len as i64 {
            return Err(VerifyError::JumpOutOfBounds { pc, target });
        }
    }
    Ok(())
}
