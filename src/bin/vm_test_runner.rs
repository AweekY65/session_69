//! Standalone test runner. Run with:
//!
//!     cargo run --bin vm_test_runner
//!
//! Prints one PASS/FAIL line per test and exits non-zero if any test fails.

use restricted_vm::isa::{Instruction, Op};
use restricted_vm::verifier::{verify, Config, VerifyError};
use restricted_vm::vm::{verify_and_run, RunFailure, Vm, VmError};

type TestResult = Result<(), String>;

struct TestCase {
    name: &'static str,
    run: fn() -> TestResult,
}

fn assemble(ins: &[Instruction]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ins.len() * 8);
    for i in ins {
        out.extend_from_slice(&i.encode());
    }
    out
}

fn expect_verify_ok(bytes: &[u8]) -> TestResult {
    verify(bytes, &Config::default())
        .map(|_| ())
        .map_err(|e| format!("expected verify ok, got: {e}"))
}

fn expect_verify_err(bytes: &[u8], want: &VerifyError) -> TestResult {
    match verify(bytes, &Config::default()) {
        Ok(_) => Err(format!("expected verify error {want:?}, got Ok")),
        Err(e) if &e == want => Ok(()),
        Err(e) => Err(format!("expected verify error {want:?}, got {e:?}")),
    }
}

fn expect_run(bytes: &[u8], cfg: &Config, want: i64) -> TestResult {
    match verify_and_run(bytes, cfg) {
        Ok(v) if v == want => Ok(()),
        Ok(v) => Err(format!("expected r0 = {want}, got {v}")),
        Err(e) => Err(format!("expected r0 = {want}, got failure: {e}")),
    }
}

fn expect_run_failure(bytes: &[u8], cfg: &Config, want: &RunFailure) -> TestResult {
    match verify_and_run(bytes, cfg) {
        Ok(v) => Err(format!("expected failure {want:?}, got Ok({v})")),
        Err(e) if &e == want => Ok(()),
        Err(e) => Err(format!("expected failure {want:?}, got {e:?}")),
    }
}

// ---------- legal programs ----------

fn test_legal_arithmetic() -> TestResult {
    // r0 = (40 + 2) * 3 - 6 = 120
    let prog = assemble(&[
        Instruction::new(Op::MovImm, 0, 0, 40),
        Instruction::new(Op::AddImm, 0, 0, 2),
        Instruction::new(Op::MulImm, 0, 0, 3),
        Instruction::new(Op::SubImm, 0, 0, 6),
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    expect_verify_ok(&prog)?;
    expect_run(&prog, &Config::default(), 120)
}

fn test_legal_stack_and_branch() -> TestResult {
    // r1 = 7; stack[3] = r1; r2 = stack[3];
    // r0 = 100; if r1 == r2 skip next; r0 = 0 (skipped); exit -> 100
    let prog = assemble(&[
        Instruction::new(Op::MovImm, 1, 0, 7),
        Instruction::new(Op::Stw, 0, 1, 3),
        Instruction::new(Op::Ldw, 2, 0, 3),
        Instruction::new(Op::MovImm, 0, 0, 100),
        Instruction::new(Op::Jeq, 1, 2, 1), // skip the next instruction
        Instruction::new(Op::MovImm, 0, 0, 0),
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    expect_verify_ok(&prog)?;
    expect_run(&prog, &Config::default(), 100)
}

fn test_legal_branch_not_taken() -> TestResult {
    // r1 = 1; r2 = 2; if r1 > r2 skip; r0 = 5; exit -> 5
    let prog = assemble(&[
        Instruction::new(Op::MovImm, 1, 0, 1),
        Instruction::new(Op::MovImm, 2, 0, 2),
        Instruction::new(Op::Jgt, 1, 2, 0),
        Instruction::new(Op::MovImm, 0, 0, 5),
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    expect_verify_ok(&prog)?;
    expect_run(&prog, &Config::default(), 5)
}

// ---------- verifier rejections ----------

fn test_reject_illegal_opcode() -> TestResult {
    let mut prog = assemble(&[Instruction::new(Op::Exit, 0, 0, 0)]);
    let mut bad = vec![0x7Eu8, 0, 0, 0, 0, 0, 0, 0];
    bad.extend_from_slice(&prog);
    prog = bad;
    expect_verify_err(
        &prog,
        &VerifyError::IllegalOpcode { pc: 0, byte: 0x7E },
    )
}

fn test_reject_register_out_of_bounds() -> TestResult {
    let prog = assemble(&[
        Instruction::new(Op::MovImm, 10, 0, 1), // r10 does not exist
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    expect_verify_err(
        &prog,
        &VerifyError::RegisterOutOfBounds { pc: 0, reg: 10 },
    )
}

fn test_reject_stack_out_of_bounds_store() -> TestResult {
    let prog = assemble(&[
        Instruction::new(Op::MovImm, 1, 0, 1),
        Instruction::new(Op::Stw, 0, 1, 64), // stack has slots 0..63
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    expect_verify_err(
        &prog,
        &VerifyError::StackOutOfBounds { pc: 1, slot: 64 },
    )
}

fn test_reject_stack_out_of_bounds_load_negative() -> TestResult {
    let prog = assemble(&[
        Instruction::new(Op::Ldw, 0, 0, -1),
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    expect_verify_err(
        &prog,
        &VerifyError::StackOutOfBounds { pc: 0, slot: -1 },
    )
}

fn test_reject_jump_out_of_bounds() -> TestResult {
    let prog = assemble(&[
        Instruction::new(Op::Ja, 0, 0, 5), // target 6, program len 2
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    expect_verify_err(
        &prog,
        &VerifyError::JumpOutOfBounds { pc: 0, target: 6 },
    )
}

fn test_reject_backward_jump_loop() -> TestResult {
    // "Infinite loop": JA -1 jumps to itself. Verifier must reject it.
    let prog = assemble(&[
        Instruction::new(Op::Ja, 0, 0, -1),
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    expect_verify_err(
        &prog,
        &VerifyError::BackwardJump { pc: 0, target: 0 },
    )
}

fn test_reject_conditional_backward_jump() -> TestResult {
    let prog = assemble(&[
        Instruction::new(Op::MovImm, 1, 0, 0),
        Instruction::new(Op::Jeq, 1, 1, -2), // back to instruction 0
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    expect_verify_err(
        &prog,
        &VerifyError::BackwardJump { pc: 1, target: 0 },
    )
}

fn test_reject_missing_exit() -> TestResult {
    let prog = assemble(&[Instruction::new(Op::MovImm, 0, 0, 1)]);
    expect_verify_err(&prog, &VerifyError::MissingExit)
}

fn test_reject_malformed_length() -> TestResult {
    let prog = vec![0x01u8, 0, 0, 1]; // 4 bytes, not a multiple of 8
    match verify(&prog, &Config::default()) {
        Err(VerifyError::Malformed { .. }) => Ok(()),
        other => Err(format!("expected Malformed, got {other:?}")),
    }
}

fn test_reject_empty_program() -> TestResult {
    match verify(&[], &Config::default()) {
        Err(VerifyError::Malformed { .. }) => Ok(()),
        other => Err(format!("expected Malformed, got {other:?}")),
    }
}

// ---------- runtime safety ----------

fn test_budget_exceeded() -> TestResult {
    // Verified, terminating program, but budget is smaller than its length.
    let prog = assemble(&[
        Instruction::new(Op::MovImm, 0, 0, 1),
        Instruction::new(Op::AddImm, 0, 0, 1),
        Instruction::new(Op::AddImm, 0, 0, 1),
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    let cfg = Config {
        max_instructions: 2,
        ..Config::default()
    };
    expect_run_failure(
        &prog,
        &cfg,
        &RunFailure::Runtime(VmError::BudgetExceeded { budget: 2 }),
    )
}

fn test_division_by_zero() -> TestResult {
    let prog = assemble(&[
        Instruction::new(Op::MovImm, 0, 0, 10),
        Instruction::new(Op::DivImm, 0, 0, 0),
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    expect_verify_ok(&prog)?;
    expect_run_failure(
        &prog,
        &Config::default(),
        &RunFailure::Runtime(VmError::DivisionByZero),
    )
}

fn test_arithmetic_boundary_overflow_wraps() -> TestResult {
    // i64::MIN - 1 wraps to i64::MAX (documented wrapping semantics).
    let prog = assemble(&[
        Instruction::new(Op::MovImm, 0, 0, 1),
        Instruction::new(Op::MulImm, 0, 0, 1 << 20),
        Instruction::new(Op::MulImm, 0, 0, 1 << 11),
        Instruction::new(Op::MulImm, 0, 0, 1 << 20),
        Instruction::new(Op::MulImm, 0, 0, 1 << 12), // r0 = 2^63 wraps to i64::MIN
        Instruction::new(Op::AddImm, 0, 0, -1),      // i64::MIN - 1 wraps to i64::MAX
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    expect_verify_ok(&prog)?;
    expect_run(&prog, &Config::default(), i64::MAX)
}

fn test_arithmetic_boundary_min_div_neg_one() -> TestResult {
    // i64::MIN / -1 wraps to i64::MIN instead of panicking.
    let prog = assemble(&[
        Instruction::new(Op::MovImm, 0, 0, 1),
        Instruction::new(Op::MulImm, 0, 0, 1 << 20),
        Instruction::new(Op::MulImm, 0, 0, 1 << 11),
        Instruction::new(Op::MulImm, 0, 0, 1 << 20),
        Instruction::new(Op::MulImm, 0, 0, 1 << 12), // r0 = 2^63 wraps to i64::MIN
        Instruction::new(Op::MovImm, 1, 0, -1),
        Instruction::new(Op::DivReg, 0, 1, 0),
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    expect_verify_ok(&prog)?;
    expect_run(&prog, &Config::default(), i64::MIN)
}

fn test_memory_isolation() -> TestResult {
    // A verified program touches only VM-owned stack slots; the VM struct
    // itself owns exactly `stack_words` cells, so nothing outside can be
    // reached. Access every legal slot and confirm success.
    let mut ins = Vec::new();
    ins.push(Instruction::new(Op::MovImm, 1, 0, 9));
    for slot in 0..64 {
        ins.push(Instruction::new(Op::Stw, 0, 1, slot));
    }
    ins.push(Instruction::new(Op::Ldw, 0, 0, 63));
    ins.push(Instruction::new(Op::Exit, 0, 0, 0));
    let prog = assemble(&ins);
    expect_verify_ok(&prog)?;
    let cfg = Config::default();
    let program = verify(&prog, &cfg).map_err(|e| e.to_string())?;
    let mut vm = Vm::new(&cfg);
    let r0 = vm.run(&program).map_err(|e| e.to_string())?;
    if r0 != 9 {
        return Err(format!("expected r0 = 9, got {r0}"));
    }
    if vm.stack().len() != 64 || vm.stack().iter().any(|&w| w != 9) {
        return Err("stack contents unexpected".to_string());
    }
    Ok(())
}

fn test_verifier_determinism() -> TestResult {
    // Same bytecode verified repeatedly must yield the identical verdict.
    let good = assemble(&[
        Instruction::new(Op::MovImm, 0, 0, 1),
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    let bad = assemble(&[
        Instruction::new(Op::Ja, 0, 0, -1),
        Instruction::new(Op::Exit, 0, 0, 0),
    ]);
    for _ in 0..100 {
        if verify(&good, &Config::default()).is_err() {
            return Err("good program rejected on re-verification".to_string());
        }
        match verify(&bad, &Config::default()) {
            Err(VerifyError::BackwardJump { pc: 0, target: 0 }) => {}
            other => return Err(format!("non-deterministic verdict: {other:?}")),
        }
    }
    Ok(())
}

fn main() {
    let tests: &[TestCase] = &[
        TestCase { name: "legal_arithmetic", run: test_legal_arithmetic },
        TestCase { name: "legal_stack_and_branch", run: test_legal_stack_and_branch },
        TestCase { name: "legal_branch_not_taken", run: test_legal_branch_not_taken },
        TestCase { name: "reject_illegal_opcode", run: test_reject_illegal_opcode },
        TestCase { name: "reject_register_out_of_bounds", run: test_reject_register_out_of_bounds },
        TestCase { name: "reject_stack_out_of_bounds_store", run: test_reject_stack_out_of_bounds_store },
        TestCase { name: "reject_stack_out_of_bounds_load_negative", run: test_reject_stack_out_of_bounds_load_negative },
        TestCase { name: "reject_jump_out_of_bounds", run: test_reject_jump_out_of_bounds },
        TestCase { name: "reject_backward_jump_loop", run: test_reject_backward_jump_loop },
        TestCase { name: "reject_conditional_backward_jump", run: test_reject_conditional_backward_jump },
        TestCase { name: "reject_missing_exit", run: test_reject_missing_exit },
        TestCase { name: "reject_malformed_length", run: test_reject_malformed_length },
        TestCase { name: "reject_empty_program", run: test_reject_empty_program },
        TestCase { name: "budget_exceeded", run: test_budget_exceeded },
        TestCase { name: "division_by_zero", run: test_division_by_zero },
        TestCase { name: "arithmetic_boundary_overflow_wraps", run: test_arithmetic_boundary_overflow_wraps },
        TestCase { name: "arithmetic_boundary_min_div_neg_one", run: test_arithmetic_boundary_min_div_neg_one },
        TestCase { name: "memory_isolation", run: test_memory_isolation },
        TestCase { name: "verifier_determinism", run: test_verifier_determinism },
    ];

    let mut failed = 0usize;
    for t in tests {
        match (t.run)() {
            Ok(()) => println!("PASS {}", t.name),
            Err(reason) => {
                failed += 1;
                println!("FAIL {} -- {}", t.name, reason);
            }
        }
    }
    println!("---");
    println!(
        "{} passed, {} failed, {} total",
        tests.len() - failed,
        failed,
        tests.len()
    );
    if failed > 0 {
        std::process::exit(1);
    }
}
