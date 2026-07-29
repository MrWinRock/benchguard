#[cfg(not(any(target_os = "linux", windows)))]
compile_error!("BenchGuard v0.1 supports only Windows and Linux targets.");

pub mod app;
pub mod baseline;
pub mod cli;
pub mod comparison;
pub mod domain;
pub mod error;
pub mod report;
pub mod runner;
pub mod stats;
