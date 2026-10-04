//! rvm: a restricted bytecode VM with a static verifier.
//!
//! Everything lives in local memory or local files: no Linux eBPF
//! subsystem, no remote execution, no external services.

pub mod isa;
pub mod verifier;
pub mod vm;

pub use verifier::{verify, Config, VerifyError};
pub use vm::{RunError, Vm};
