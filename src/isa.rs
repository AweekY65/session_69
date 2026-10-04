//! Instruction set architecture (ISA) definition.
//!
//! Fixed-width 8-byte little-endian instruction encoding (eBPF-inspired):
//!
//! ```text
//! byte 0    : opcode
//! byte 1    : dst register (low nibble) | src register (high nibble)
//! bytes 2-3 : signed 16-bit offset (jump offset or memory offset)
//! bytes 4-7 : signed 32-bit immediate
//! ```

/// Number of architectural registers: R0..=R9 are general purpose,
/// R10 is the read-only stack frame pointer.
pub const REG_COUNT: u8 = 11;
/// Read-only frame pointer register.
pub const REG_FP: u8 = 10;
/// Return-value register.
pub const REG_RET: u8 = 0;

pub const INSN_SIZE: usize = 8;

// ---- ALU64, register source ----
pub const ADD64_REG: u8 = 0x0f;
pub const SUB64_REG: u8 = 0x1f;
pub const MUL64_REG: u8 = 0x2f;
pub const DIV64_REG: u8 = 0x3f;
pub const OR64_REG: u8 = 0x4f;
pub const AND64_REG: u8 = 0x5f;
pub const LSH64_REG: u8 = 0x6f;
pub const RSH64_REG: u8 = 0x7f;
pub const MOD64_REG: u8 = 0x9f;
pub const XOR64_REG: u8 = 0xaf;
pub const MOV64_REG: u8 = 0xbf;
pub const ARSH64_REG: u8 = 0xcf;

// ---- ALU64, immediate source ----
pub const ADD64_IMM: u8 = 0x07;
pub const SUB64_IMM: u8 = 0x17;
pub const MUL64_IMM: u8 = 0x27;
pub const DIV64_IMM: u8 = 0x37;
pub const OR64_IMM: u8 = 0x47;
pub const AND64_IMM: u8 = 0x57;
pub const LSH64_IMM: u8 = 0x67;
pub const RSH64_IMM: u8 = 0x77;
pub const NEG64: u8 = 0x87;
pub const MOD64_IMM: u8 = 0x97;
pub const XOR64_IMM: u8 = 0xa7;
pub const MOV64_IMM: u8 = 0xb7;
pub const ARSH64_IMM: u8 = 0xc7;

// ---- Jumps ----
pub const JA: u8 = 0x05;
pub const JEQ_IMM: u8 = 0x15;
pub const JEQ_REG: u8 = 0x1d;
pub const JGT_IMM: u8 = 0x25;
pub const JGT_REG: u8 = 0x2d;
pub const JGE_IMM: u8 = 0x35;
pub const JGE_REG: u8 = 0x3d;
pub const JSET_IMM: u8 = 0x45;
pub const JSET_REG: u8 = 0x4d;
pub const JNE_IMM: u8 = 0x55;
pub const JNE_REG: u8 = 0x5d;
pub const JSGT_IMM: u8 = 0x65;
pub const JSGT_REG: u8 = 0x6d;
pub const JSGE_IMM: u8 = 0x75;
pub const JSGE_REG: u8 = 0x7d;
pub const JLT_IMM: u8 = 0xa5;
pub const JLT_REG: u8 = 0xad;
pub const JLE_IMM: u8 = 0xb5;
pub const JLE_REG: u8 = 0xbd;
pub const JSLT_IMM: u8 = 0xc5;
pub const JSLT_REG: u8 = 0xcd;
pub const JSLE_IMM: u8 = 0xd5;
pub const JSLE_REG: u8 = 0xdd;
pub const EXIT: u8 = 0x95;

// ---- Memory ----
pub const LDXW: u8 = 0x61;
pub const LDXH: u8 = 0x69;
pub const LDXB: u8 = 0x71;
pub const LDXDW: u8 = 0x79;
pub const STXW: u8 = 0x63;
pub const STXH: u8 = 0x6b;
pub const STXB: u8 = 0x73;
pub const STXDW: u8 = 0x7b;
pub const STW: u8 = 0x62;
pub const STH: u8 = 0x6a;
pub const STB: u8 = 0x72;
pub const STDW: u8 = 0x7a;

/// One decoded instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Insn {
    pub opc: u8,
    pub dst: u8,
    pub src: u8,
    pub off: i16,
    pub imm: i32,
}

impl Insn {
    pub fn new(opc: u8, dst: u8, src: u8, off: i16, imm: i32) -> Self {
        Insn { opc, dst, src, off, imm }
    }

    pub fn encode(&self) -> [u8; INSN_SIZE] {
        let mut b = [0u8; INSN_SIZE];
        b[0] = self.opc;
        b[1] = (self.src << 4) | (self.dst & 0x0f);
        b[2..4].copy_from_slice(&self.off.to_le_bytes());
        b[4..8].copy_from_slice(&self.imm.to_le_bytes());
        b
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < INSN_SIZE {
            return None;
        }
        Some(Insn {
            opc: bytes[0],
            dst: bytes[1] & 0x0f,
            src: bytes[1] >> 4,
            off: i16::from_le_bytes([bytes[2], bytes[3]]),
            imm: i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        })
    }
}

/// ALU operation kind, shared by IMM and REG variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AluOp {
    Add,
    Sub,
    Mul,
    Div,
    Or,
    And,
    Lsh,
    Rsh,
    Neg,
    Mod,
    Xor,
    Mov,
    Arsh,
}

/// Conditional jump predicate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cond {
    Eq,
    Gt,
    Ge,
    Set,
    Ne,
    Sgt,
    Sge,
    Lt,
    Le,
    Slt,
    Sle,
}

/// Static classification of an opcode. Returns `None` for unknown opcodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// 64-bit ALU with immediate operand (NEG has no operand).
    AluImm(AluOp),
    /// 64-bit ALU with register operand.
    AluReg(AluOp),
    /// Unconditional jump (offset in `off`).
    Ja,
    /// Conditional jump; IMM variant compares against `imm`, REG against `src`.
    JCondImm(Cond),
    JCondReg(Cond),
    Exit,
    /// Load from memory: dst = *(base=src + off), size in bytes.
    Load(usize),
    /// Store register to memory: *(base=dst + off) = src.
    StoreReg(usize),
    /// Store immediate to memory: *(base=dst + off) = imm.
    StoreImm(usize),
}

pub fn classify(opc: u8) -> Option<Class> {
    use AluOp::*;
    use Class::*;
    use Cond::*;
    Some(match opc {
        ADD64_REG => AluReg(Add),
        SUB64_REG => AluReg(Sub),
        MUL64_REG => AluReg(Mul),
        DIV64_REG => AluReg(Div),
        OR64_REG => AluReg(Or),
        AND64_REG => AluReg(And),
        LSH64_REG => AluReg(Lsh),
        RSH64_REG => AluReg(Rsh),
        MOD64_REG => AluReg(Mod),
        XOR64_REG => AluReg(Xor),
        MOV64_REG => AluReg(Mov),
        ARSH64_REG => AluReg(Arsh),
        ADD64_IMM => AluImm(Add),
        SUB64_IMM => AluImm(Sub),
        MUL64_IMM => AluImm(Mul),
        DIV64_IMM => AluImm(Div),
        OR64_IMM => AluImm(Or),
        AND64_IMM => AluImm(And),
        LSH64_IMM => AluImm(Lsh),
        RSH64_IMM => AluImm(Rsh),
        NEG64 => AluImm(Neg),
        MOD64_IMM => AluImm(Mod),
        XOR64_IMM => AluImm(Xor),
        MOV64_IMM => AluImm(Mov),
        ARSH64_IMM => AluImm(Arsh),
        JA => Ja,
        JEQ_IMM => JCondImm(Eq),
        JGT_IMM => JCondImm(Gt),
        JGE_IMM => JCondImm(Ge),
        JSET_IMM => JCondImm(Set),
        JNE_IMM => JCondImm(Ne),
        JSGT_IMM => JCondImm(Sgt),
        JSGE_IMM => JCondImm(Sge),
        JLT_IMM => JCondImm(Lt),
        JLE_IMM => JCondImm(Le),
        JSLT_IMM => JCondImm(Slt),
        JSLE_IMM => JCondImm(Sle),
        JEQ_REG => JCondReg(Eq),
        JGT_REG => JCondReg(Gt),
        JGE_REG => JCondReg(Ge),
        JSET_REG => JCondReg(Set),
        JNE_REG => JCondReg(Ne),
        JSGT_REG => JCondReg(Sgt),
        JSGE_REG => JCondReg(Sge),
        JLT_REG => JCondReg(Lt),
        JLE_REG => JCondReg(Le),
        JSLT_REG => JCondReg(Slt),
        JSLE_REG => JCondReg(Sle),
        EXIT => Exit,
        LDXW => Load(4),
        LDXH => Load(2),
        LDXB => Load(1),
        LDXDW => Load(8),
        STXW => StoreReg(4),
        STXH => StoreReg(2),
        STXB => StoreReg(1),
        STXDW => StoreReg(8),
        STW => StoreImm(4),
        STH => StoreImm(2),
        STB => StoreImm(1),
        STDW => StoreImm(8),
        _ => return None,
    })
}

// ---- Assembly helpers (used by tests and embedders) ----

pub fn alu64_imm(opc: u8, dst: u8, imm: i32) -> Insn {
    Insn::new(opc, dst, 0, 0, imm)
}

pub fn alu64_reg(opc: u8, dst: u8, src: u8) -> Insn {
    Insn::new(opc, dst, src, 0, 0)
}

pub fn mov64_imm(dst: u8, imm: i32) -> Insn {
    alu64_imm(MOV64_IMM, dst, imm)
}

pub fn ja(off: i16) -> Insn {
    Insn::new(JA, 0, 0, off, 0)
}

pub fn jcond_imm(opc: u8, dst: u8, imm: i32, off: i16) -> Insn {
    Insn::new(opc, dst, 0, off, imm)
}

pub fn jcond_reg(opc: u8, dst: u8, src: u8, off: i16) -> Insn {
    Insn::new(opc, dst, src, off, 0)
}

pub fn ldx(opc: u8, dst: u8, base: u8, off: i16) -> Insn {
    Insn::new(opc, dst, base, off, 0)
}

pub fn stx(opc: u8, base: u8, src: u8, off: i16) -> Insn {
    Insn::new(opc, base, src, off, 0)
}

pub fn st_imm(opc: u8, base: u8, off: i16, imm: i32) -> Insn {
    Insn::new(opc, base, 0, off, imm)
}

pub fn exit() -> Insn {
    Insn::new(EXIT, 0, 0, 0, 0)
}

/// Encode a slice of instructions into flat bytecode.
pub fn encode_program(insns: &[Insn]) -> Vec<u8> {
    let mut out = Vec::with_capacity(insns.len() * INSN_SIZE);
    for i in insns {
        out.extend_from_slice(&i.encode());
    }
    out
}
