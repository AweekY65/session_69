//! Instruction set architecture (ISA) definitions.
//!
//! Fixed-width 8-byte instructions, little-endian:
//!
//! ```text
//! byte 0    : opcode
//! byte 1    : dst register index
//! byte 2    : src register index
//! byte 3..7 : imm (i32, little-endian)
//! byte 7    : reserved (must be 0)
//! ```

pub const INSTRUCTION_SIZE: usize = 8;

/// Number of general-purpose registers: r0..r9. r0 holds the return value.
pub const NUM_REGISTERS: usize = 10;

/// Number of 8-byte words in the VM stack (64 words = 512 bytes).
pub const STACK_WORDS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// dst = imm
    MovImm,
    /// dst = src
    MovReg,
    /// dst += imm (wrapping)
    AddImm,
    /// dst += src (wrapping)
    AddReg,
    /// dst -= imm (wrapping)
    SubImm,
    /// dst -= src (wrapping)
    SubReg,
    /// dst *= imm (wrapping)
    MulImm,
    /// dst *= src (wrapping)
    MulReg,
    /// dst /= imm (wrapping; imm == 0 is a runtime error)
    DivImm,
    /// dst /= src (wrapping; src == 0 is a runtime error)
    DivReg,
    /// dst %= imm (wrapping; imm == 0 is a runtime error)
    ModImm,
    /// dst %= src (wrapping; src == 0 is a runtime error)
    ModReg,
    /// dst = -dst (wrapping)
    Neg,
    /// pc += imm (unconditional jump; imm is an instruction offset)
    Ja,
    /// if dst == src { pc += imm }
    Jeq,
    /// if dst != src { pc += imm }
    Jne,
    /// if dst > src (signed) { pc += imm }
    Jgt,
    /// if dst >= src (signed) { pc += imm }
    Jge,
    /// if dst < src (signed) { pc += imm }
    Jlt,
    /// if dst <= src (signed) { pc += imm }
    Jle,
    /// dst = stack[imm]  (imm is a word index into the stack)
    Ldw,
    /// stack[imm] = src
    Stw,
    /// halt; return value is r0
    Exit,
}

impl Op {
    pub fn from_byte(byte: u8) -> Option<Op> {
        Some(match byte {
            0x01 => Op::MovImm,
            0x02 => Op::MovReg,
            0x03 => Op::AddImm,
            0x04 => Op::AddReg,
            0x05 => Op::SubImm,
            0x06 => Op::SubReg,
            0x07 => Op::MulImm,
            0x08 => Op::MulReg,
            0x09 => Op::DivImm,
            0x0A => Op::DivReg,
            0x0B => Op::ModImm,
            0x0C => Op::ModReg,
            0x0D => Op::Neg,
            0x10 => Op::Ja,
            0x11 => Op::Jeq,
            0x12 => Op::Jne,
            0x13 => Op::Jgt,
            0x14 => Op::Jge,
            0x15 => Op::Jlt,
            0x16 => Op::Jle,
            0x20 => Op::Ldw,
            0x21 => Op::Stw,
            0xFF => Op::Exit,
            _ => return None,
        })
    }

    pub fn to_byte(self) -> u8 {
        match self {
            Op::MovImm => 0x01,
            Op::MovReg => 0x02,
            Op::AddImm => 0x03,
            Op::AddReg => 0x04,
            Op::SubImm => 0x05,
            Op::SubReg => 0x06,
            Op::MulImm => 0x07,
            Op::MulReg => 0x08,
            Op::DivImm => 0x09,
            Op::DivReg => 0x0A,
            Op::ModImm => 0x0B,
            Op::ModReg => 0x0C,
            Op::Neg => 0x0D,
            Op::Ja => 0x10,
            Op::Jeq => 0x11,
            Op::Jne => 0x12,
            Op::Jgt => 0x13,
            Op::Jge => 0x14,
            Op::Jlt => 0x15,
            Op::Jle => 0x16,
            Op::Ldw => 0x20,
            Op::Stw => 0x21,
            Op::Exit => 0xFF,
        }
    }

    /// Whether this opcode reads the dst register field as a register index.
    pub fn uses_dst(self) -> bool {
        !matches!(self, Op::Stw)
    }

    /// Whether this opcode reads the src register field as a register index.
    pub fn uses_src(self) -> bool {
        matches!(
            self,
            Op::MovReg
                | Op::AddReg
                | Op::SubReg
                | Op::MulReg
                | Op::DivReg
                | Op::ModReg
                | Op::Jeq
                | Op::Jne
                | Op::Jgt
                | Op::Jge
                | Op::Jlt
                | Op::Jle
                | Op::Stw
        )
    }

    /// Whether this opcode treats imm as a jump offset.
    pub fn is_jump(self) -> bool {
        matches!(
            self,
            Op::Ja | Op::Jeq | Op::Jne | Op::Jgt | Op::Jge | Op::Jlt | Op::Jle
        )
    }

    /// Whether this opcode treats imm as a stack word index.
    pub fn is_mem(self) -> bool {
        matches!(self, Op::Ldw | Op::Stw)
    }
}

/// A decoded instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    pub op: Op,
    pub dst: u8,
    pub src: u8,
    pub imm: i32,
}

impl Instruction {
    pub fn new(op: Op, dst: u8, src: u8, imm: i32) -> Instruction {
        Instruction { op, dst, src, imm }
    }

    /// Decode one 8-byte instruction. Returns None on unknown opcode.
    pub fn decode(bytes: &[u8]) -> Option<Instruction> {
        debug_assert_eq!(bytes.len(), INSTRUCTION_SIZE);
        let op = Op::from_byte(bytes[0])?;
        let imm = i32::from_le_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]);
        Some(Instruction {
            op,
            dst: bytes[1],
            src: bytes[2],
            imm,
        })
    }

    pub fn encode(&self) -> [u8; INSTRUCTION_SIZE] {
        let mut out = [0u8; INSTRUCTION_SIZE];
        out[0] = self.op.to_byte();
        out[1] = self.dst;
        out[2] = self.src;
        out[3..7].copy_from_slice(&self.imm.to_le_bytes());
        out
    }
}
