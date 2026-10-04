//! A restricted, self-contained bytecode virtual machine with a static
//! verifier. No eBPF, no remote execution, no external services: programs,
//! bytecode, execution state and verification results live only in local
//! memory or local files.

pub mod isa;
pub mod verifier;
pub mod vm;
