use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BenchguardError {
    #[error("cannot aggregate an empty sample set")]
    EmptySamples,
    #[error("numeric overflow while calculating benchmark statistics")]
    NumericOverflow,
    #[error("unsupported baseline schema version: {0}")]
    UnsupportedSchema(u32),
    #[error("invalid baseline: {0}")]
    InvalidBaseline(String),
    #[error("failed to {operation} at {}", path.display())]
    BaselineIo {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize baseline")]
    BaselineSerialization(#[source] serde_json::Error),
    #[error("requested run count must be greater than zero")]
    ZeroRuns,
    #[error("{0}")]
    InvalidArguments(String),
    #[error("failed to launch benchmark command")]
    CommandLaunch {
        #[source]
        source: std::io::Error,
    },
    #[error("benchmark command exited with code {exit_code}")]
    CommandFailed { exit_code: i32 },
    #[error("benchmark command timed out")]
    Timeout,
    #[error("failed to {operation} while measuring benchmark command")]
    Measurement {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("{0} is not supported until platform metric collection is available")]
    UnsupportedMetricBudget(&'static str),
    #[error("benchmark {0:?} was not found in the baseline")]
    BenchmarkNotFound(String),
    #[error(
        "baseline platform {baseline_os}/{baseline_arch} does not match current platform \
         {current_os}/{current_arch}"
    )]
    IncompatiblePlatform {
        baseline_os: String,
        baseline_arch: String,
        current_os: String,
        current_arch: String,
    },
    #[error("failed to format the baseline timestamp")]
    TimestampFormatting(#[source] time::error::Format),
}

impl BenchguardError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ZeroRuns | Self::InvalidArguments(_) | Self::UnsupportedMetricBudget(_) => {
                "invalid_arguments"
            }
            Self::CommandLaunch { .. } | Self::CommandFailed { .. } => "command_failed",
            Self::Timeout => "timeout",
            Self::InvalidBaseline(_)
            | Self::BenchmarkNotFound(_)
            | Self::BaselineIo {
                operation: "read baseline",
                ..
            } => "invalid_baseline",
            Self::UnsupportedSchema(_) => "unsupported_schema",
            Self::IncompatiblePlatform { .. } => "incompatible_platform",
            Self::EmptySamples | Self::NumericOverflow | Self::Measurement { .. } => {
                "measurement_failed"
            }
            Self::BaselineIo { .. }
            | Self::BaselineSerialization(_)
            | Self::TimestampFormatting(_) => "internal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitClass {
    Success,
    Regression,
}

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf};

    use super::BenchguardError;

    // Catches unstable or overly generic JSON codes for the operational error
    // variants available before timeout and platform collectors land.
    #[test]
    fn operational_errors_map_to_the_stable_public_code_set() {
        let cases = [
            (BenchguardError::ZeroRuns, "invalid_arguments"),
            (
                BenchguardError::InvalidArguments("invalid CLI".to_owned()),
                "invalid_arguments",
            ),
            (
                BenchguardError::UnsupportedMetricBudget("--max-cpu"),
                "invalid_arguments",
            ),
            (
                BenchguardError::CommandLaunch {
                    source: io::Error::new(io::ErrorKind::NotFound, "missing"),
                },
                "command_failed",
            ),
            (
                BenchguardError::CommandFailed { exit_code: 23 },
                "command_failed",
            ),
            (BenchguardError::Timeout, "timeout"),
            (
                BenchguardError::Measurement {
                    operation: "read /proc",
                    source: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
                },
                "measurement_failed",
            ),
            (
                BenchguardError::InvalidBaseline("broken".to_owned()),
                "invalid_baseline",
            ),
            (
                BenchguardError::BenchmarkNotFound("startup".to_owned()),
                "invalid_baseline",
            ),
            (
                BenchguardError::BaselineIo {
                    operation: "read baseline",
                    path: PathBuf::from("benchguard.json"),
                    source: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
                },
                "invalid_baseline",
            ),
            (BenchguardError::UnsupportedSchema(9), "unsupported_schema"),
            (
                BenchguardError::IncompatiblePlatform {
                    baseline_os: "linux".to_owned(),
                    baseline_arch: "x86_64".to_owned(),
                    current_os: "windows".to_owned(),
                    current_arch: "x86_64".to_owned(),
                },
                "incompatible_platform",
            ),
            (BenchguardError::EmptySamples, "measurement_failed"),
            (BenchguardError::NumericOverflow, "measurement_failed"),
            (
                BenchguardError::BaselineIo {
                    operation: "replace baseline",
                    path: PathBuf::from("benchguard.json"),
                    source: io::Error::other("replace failed"),
                },
                "internal",
            ),
            (
                BenchguardError::BaselineSerialization(
                    serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
                ),
                "internal",
            ),
            (
                BenchguardError::TimestampFormatting(time::error::Format::StdIo(io::Error::other(
                    "format failed",
                ))),
                "internal",
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(error.code(), expected, "{error:?}");
        }
    }
}
