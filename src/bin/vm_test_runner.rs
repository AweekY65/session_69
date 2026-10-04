//! Standalone test runner. Run with:
//!
//!     cargo run --bin vm_test_runner
//!
//! Prints one PASS/FAIL line per test and exits non-zero on any failure.
//! (Intentionally not `cargo test`: tests must be runnable as a plain
//! terminal program.)

use rvm::isa::*;
use rvm::verifier::{verify, Config, VerifyError};
use rvm::vm::{RunError, Vm};

type TestFn = fn() -> Result<(), String>;

fn default_cfg() -> Config {
    Config::default()
}

fn run_prog(code: &[Insn], cfg: Config) -> Result<u64, String> {
    let bytes = encode_program(code);
    let insns = verify(&bytes, &cfg).map_err(|e| format!("verify failed: {e}"))?;
    let mut mem = vec![0u8; cfg.mem_size];
    let mut vm = Vm::new(insns, cfg, &mut mem);
    vm.run().map_err(|e| format!("run failed: {e}"))
}

fn expect_verify_err(code: &[Insn], want: &VerifyError) -> Result<(), String> {
    let bytes = encode_program(code);
    match verify(&bytes, &default_cfg()) {
        Ok(_) => Err(format!("expected verify error {want:?}, got Ok")),
        Err(e) if &e == want => Ok(()),
        Err(e) => Err(format!("expected verify error {want:?}, got {e:?}")),
    }
}

fn expect_run_err(code: &[Insn], want: &RunError) -> Result<(), String> {
    let cfg = default_cfg();
    let bytes = encode_program(code);
    let insns = verify(&bytes, &cfg).map_err(|e| format!("unexpected verify error: {e}"))?;
    let mut mem = vec![0u8; cfg.mem_size];
    let mut vm = Vm::new(insns, cfg, &mut mem);
    match vm.run() {
        Ok(v) => Err(format!("expected run error {want:?}, got Ok({v})")),
        Err(e) if &e == want => Ok(()),
        Err(e) => Err(format!("expected run error {want:?}, got {e:?}")),
    }
}

// ---------------- legal programs ----------------

fn t_legal_arithmetic() -> Result<(), String> {
    // r0 = (2 + 3) * 4 - 6 = 14
    let got = run_prog(
        &[
            mov64_imm(0, 2),
            alu64_imm(ADD64_IMM, 0, 3),
            alu64_imm(MUL64_IMM, 0, 4),
            alu64_imm(SUB64_IMM, 0, 6),
            exit(),
        ],
        default_cfg(),
    )?;
    if got == 14 { Ok(()) } else { Err(format!("got {got}, want 14")) }
}

fn t_legal_reg_ops() -> Result<(), String> {
    // r1 = 7, r2 = 6, r0 = r1 * r2 = 42
    let got = run_prog(
        &[
            mov64_imm(1, 7),
            mov64_imm(2, 6),
            alu64_reg(MUL64_REG, 1, 2),
            alu64_reg(MOV64_REG, 0, 1),
            exit(),
        ],
        default_cfg(),
    )?;
    if got == 42 { Ok(()) } else { Err(format!("got {got}, want 42")) }
}

fn t_legal_memory() -> Result<(), String> {
    // Store 0x11223344 at mem[16..20], load it back.
    let cfg = default_cfg();
    let bytes = encode_program(&[
        mov64_imm(1, 16),          // r1 = address
        mov64_imm(2, 0x11223344),  // r2 = value
        stx(STXW, 1, 2, 0),        // *(r1) = r2
        ldx(LDXW, 0, 1, 0),        // r0 = *(r1)
        exit(),
    ]);
    let insns = verify(&bytes, &cfg).map_err(|e| e.to_string())?;
    let mut mem = vec![0u8; cfg.mem_size];
    let mut vm = Vm::new(insns, cfg, &mut mem);
    let got = vm.run().map_err(|e| e.to_string())?;
    if got != 0x11223344 {
        return Err(format!("got {got:#x}"));
    }
    if mem[16..20] != [0x44, 0x33, 0x22, 0x11] {
        return Err("memory not written little-endian".into());
    }
    Ok(())
}

fn t_legal_stack() -> Result<(), String> {
    // *(fp-8) = 99 via immediate store; r0 = *(fp-8)
    let got = run_prog(
        &[
            st_imm(STDW, REG_FP, -8, 99),
            ldx(LDXDW, 0, REG_FP, -8),
            exit(),
        ],
        default_cfg(),
    )?;
    if got == 99 { Ok(()) } else { Err(format!("got {got}, want 99")) }
}

fn t_legal_conditional_jump() -> Result<(), String> {
    // if r1 == 5 { r0 = 1 } else { r0 = 2 }
    let got = run_prog(
        &[
            mov64_imm(1, 5),
            jcond_imm(JEQ_IMM, 1, 5, 1), // skip next if equal
            ja(2),                       // -> else branch
            mov64_imm(0, 1),             // then
            ja(1),
            mov64_imm(0, 2),             // else
            exit(),
        ],
        default_cfg(),
    )?;
    if got == 1 { Ok(()) } else { Err(format!("got {got}, want 1")) }
}

fn t_legal_signed_compare() -> Result<(), String> {
    // r1 = -1 (0xffff...); signed: r1 < 0 -> r0 = 1
    let got = run_prog(
        &[
            mov64_imm(1, -1),
            jcond_imm(JSLT_IMM, 1, 0, 1),
            ja(1),
            mov64_imm(0, 1),
            exit(),
        ],
        default_cfg(),
    )?;
    if got == 1 { Ok(()) } else { Err(format!("got {got}, want 1")) }
}

// ---------------- verifier rejections ----------------

fn t_reject_bad_opcode() -> Result<(), String> {
    let mut bad = mov64_imm(0, 1);
    bad.opc = 0x00; // not a defined opcode
    expect_verify_err(&[bad, exit()], &VerifyError::BadOpcode { pc: 0, opc: 0x00 })
}

fn t_reject_bad_register() -> Result<(), String> {
    let mut bad = mov64_imm(0, 1);
    bad.dst = 11; // only R0..=R10 exist
    expect_verify_err(&[bad, exit()], &VerifyError::BadRegister { pc: 0, reg: 11 })
}

fn t_reject_write_frame_pointer() -> Result<(), String> {
    expect_verify_err(
        &[mov64_imm(REG_FP, 1), exit()],
        &VerifyError::WriteToFramePointer { pc: 0 },
    )
}

fn t_reject_fp_as_alu_source() -> Result<(), String> {
    expect_verify_err(
        &[alu64_reg(ADD64_REG, 0, REG_FP), exit()],
        &VerifyError::FramePointerMisuse { pc: 0 },
    )
}

fn t_reject_stack_underflow() -> Result<(), String> {
    // fp-600 is below the 512-byte stack.
    expect_verify_err(
        &[ldx(LDXDW, 0, REG_FP, -600), exit()],
        &VerifyError::StackOutOfBounds { pc: 0, off: -600, size: 8 },
    )
}

fn t_reject_stack_overflow_positive_off() -> Result<(), String> {
    // fp+0 with size 8 ends above the frame.
    expect_verify_err(
        &[st_imm(STDW, REG_FP, 0, 1), exit()],
        &VerifyError::StackOutOfBounds { pc: 0, off: 0, size: 8 },
    )
}

fn t_reject_backward_jump_loop() -> Result<(), String> {
    // Classic infinite loop: `ja -1` jumps to itself.
    expect_verify_err(&[ja(-1), exit()], &VerifyError::BackwardJump { pc: 0, target: 0 })
}

fn t_reject_conditional_backward_jump() -> Result<(), String> {
    expect_verify_err(
        &[mov64_imm(1, 0), jcond_imm(JEQ_IMM, 1, 0, -2), exit()],
        &VerifyError::BackwardJump { pc: 1, target: 0 },
    )
}

fn t_reject_jump_out_of_bounds() -> Result<(), String> {
    expect_verify_err(&[ja(10), exit()], &VerifyError::JumpOutOfBounds { pc: 0, target: 11 })
}

fn t_reject_missing_exit() -> Result<(), String> {
    expect_verify_err(&[mov64_imm(0, 1)], &VerifyError::MissingExit)
}

fn t_reject_truncated_bytecode() -> Result<(), String> {
    let mut bytes = encode_program(&[mov64_imm(0, 1), exit()]);
    bytes.truncate(9); // not a multiple of 8
    match verify(&bytes, &default_cfg()) {
        Err(VerifyError::Truncated { len: 9 }) => Ok(()),
        other => Err(format!("expected Truncated, got {other:?}")),
    }
}

fn t_reject_div_by_zero_imm() -> Result<(), String> {
    expect_verify_err(
        &[mov64_imm(0, 1), alu64_imm(DIV64_IMM, 0, 0), exit()],
        &VerifyError::DivByZeroImm { pc: 1 },
    )
}

// ---------------- runtime faults ----------------

fn t_runtime_mem_out_of_bounds() -> Result<(), String> {
    // r1 = mem_size (one past the end); load must fault, not read host memory.
    let cfg = default_cfg();
    expect_run_err(
        &[
            mov64_imm(1, cfg.mem_size as i32),
            ldx(LDXW, 0, 1, 0),
            exit(),
        ],
        &RunError::MemOutOfBounds { pc: 1, addr: cfg.mem_size as i64, size: 4 },
    )
}

fn t_runtime_mem_negative_addr() -> Result<(), String> {
    expect_run_err(
        &[mov64_imm(1, -1), ldx(LDXB, 0, 1, 0), exit()],
        &RunError::MemOutOfBounds { pc: 1, addr: -1, size: 1 },
    )
}

fn t_runtime_div_by_zero_reg() -> Result<(), String> {
    expect_run_err(
        &[
            mov64_imm(0, 10),
            mov64_imm(1, 0),
            alu64_reg(DIV64_REG, 0, 1),
            exit(),
        ],
        &RunError::DivByZero { pc: 2 },
    )
}

fn t_budget_exceeded() -> Result<(), String> {
    // Loops are impossible, but the budget still caps runaway programs:
    // 50 MOVs with a budget of 10 must halt with BudgetExceeded.
    let mut code: Vec<Insn> = (0..50).map(|i| mov64_imm(0, i)).collect();
    code.push(exit());
    let cfg = Config { max_insns: 10, ..default_cfg() };
    let bytes = encode_program(&code);
    let insns = verify(&bytes, &cfg).map_err(|e| e.to_string())?;
    let mut mem = vec![0u8; cfg.mem_size];
    let mut vm = Vm::new(insns, cfg, &mut mem);
    match vm.run() {
        Err(RunError::BudgetExceeded) => Ok(()),
        other => Err(format!("expected BudgetExceeded, got {other:?}")),
    }
}

// ---------------- arithmetic boundaries ----------------

fn t_arith_wrapping_overflow() -> Result<(), String> {
    // u64::MAX + 1 wraps to 0 (deterministic wrapping semantics).
    let got = run_prog(
        &[
            mov64_imm(0, -1), // 0xffff_ffff_ffff_ffff
            alu64_imm(ADD64_IMM, 0, 1),
            exit(),
        ],
        default_cfg(),
    )?;
    if got == 0 { Ok(()) } else { Err(format!("got {got}, want 0")) }
}

fn t_arith_wrapping_underflow() -> Result<(), String> {
    let got = run_prog(
        &[mov64_imm(0, 0), alu64_imm(SUB64_IMM, 0, 1), exit()],
        default_cfg(),
    )?;
    if got == u64::MAX { Ok(()) } else { Err(format!("got {got}")) }
}

fn t_arith_shift_overflow() -> Result<(), String> {
    expect_run_err(
        &[mov64_imm(0, 1), alu64_imm(LSH64_IMM, 0, 64), exit()],
        &RunError::ShiftOverflow { pc: 1, shift: 64 },
    )
}

fn t_arith_divide_boundary() -> Result<(), String> {
    // u64::MAX / 2
    let got = run_prog(
        &[
            mov64_imm(0, -1),
            alu64_imm(DIV64_IMM, 0, 2),
            exit(),
        ],
        default_cfg(),
    )?;
    if got == u64::MAX / 2 { Ok(()) } else { Err(format!("got {got}")) }
}

// ---------------- determinism ----------------

fn t_verifier_determinism() -> Result<(), String> {
    // Same bytecode, verified many times with fresh configs and different
    // memory contents in between: the verdict must be identical.
    let good = encode_program(&[mov64_imm(0, 1), exit()]);
    let bad = encode_program(&[ja(-1), exit()]);
    let mem_len = 4096;
    let mut mem = vec![0xABu8; mem_len];
    for i in 0..64 {
        mem[i % mem_len] = i as u8; // mutate environment between runs
        let cfg = default_cfg();
        let g1 = verify(&good, &cfg);
        let b1 = verify(&bad, &cfg);
        let g2 = verify(&good, &cfg);
        let b2 = verify(&bad, &cfg);
        if g1.is_err() || g2.is_err() || g1 != g2 {
            return Err(format!("good program verdict unstable at iter {i}"));
        }
        if b1 != b2 || b1.is_ok() {
            return Err(format!("bad program verdict unstable at iter {i}"));
        }
    }
    Ok(())
}

// ---------------- runner ----------------

fn main() {
    let tests: &[(&str, TestFn)] = &[
        ("legal_arithmetic", t_legal_arithmetic),
        ("legal_reg_ops", t_legal_reg_ops),
        ("legal_memory", t_legal_memory),
        ("legal_stack", t_legal_stack),
        ("legal_conditional_jump", t_legal_conditional_jump),
        ("legal_signed_compare", t_legal_signed_compare),
        ("reject_bad_opcode", t_reject_bad_opcode),
        ("reject_bad_register", t_reject_bad_register),
        ("reject_write_frame_pointer", t_reject_write_frame_pointer),
        ("reject_fp_as_alu_source", t_reject_fp_as_alu_source),
        ("reject_stack_underflow", t_reject_stack_underflow),
        ("reject_stack_overflow_positive_off", t_reject_stack_overflow_positive_off),
        ("reject_backward_jump_loop", t_reject_backward_jump_loop),
        ("reject_conditional_backward_jump", t_reject_conditional_backward_jump),
        ("reject_jump_out_of_bounds", t_reject_jump_out_of_bounds),
        ("reject_missing_exit", t_reject_missing_exit),
        ("reject_truncated_bytecode", t_reject_truncated_bytecode),
        ("reject_div_by_zero_imm", t_reject_div_by_zero_imm),
        ("runtime_mem_out_of_bounds", t_runtime_mem_out_of_bounds),
        ("runtime_mem_negative_addr", t_runtime_mem_negative_addr),
        ("runtime_div_by_zero_reg", t_runtime_div_by_zero_reg),
        ("budget_exceeded", t_budget_exceeded),
        ("arith_wrapping_overflow", t_arith_wrapping_overflow),
        ("arith_wrapping_underflow", t_arith_wrapping_underflow),
        ("arith_shift_overflow", t_arith_shift_overflow),
        ("arith_divide_boundary", t_arith_divide_boundary),
        ("verifier_determinism", t_verifier_determinism),
    ];

    let mut failed = 0usize;
    for (name, f) in tests {
        match f() {
            Ok(()) => println!("PASS {name}"),
            Err(e) => {
                failed += 1;
                println!("FAIL {name}: {e}");
            }
        }
    }
    println!("---");
    println!("{} passed, {} failed, {} total", tests.len() - failed, failed, tests.len());
    if failed > 0 {
        std::process::exit(1);
    }
}
