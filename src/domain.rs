use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformId {
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sample {
    pub wall_ns: u64,
    pub cpu_ns: u64,
    pub peak_memory_bytes: u64,
    pub exit_code: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aggregate {
    pub median: u64,
    pub mean: u64,
    pub standard_deviation: u64,
    pub min: u64,
    pub max: u64,
    pub p50: u64,
    pub p95: u64,
    pub sample_count: u32,
}
