use std::{ffi::OsString, time::Duration};

use crate::{domain::Sample, error::BenchguardError};

#[cfg(any(target_os = "linux", test))]
mod linux;
#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
}

impl CommandSpec {
    pub fn new<P, I, A>(program: P, args: I) -> Self
    where
        P: Into<OsString>,
        I: IntoIterator<Item = A>,
        A: Into<OsString>,
    {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunConfig {
    pub warmups: u32,
    pub runs: u32,
    pub timeout: Option<Duration>,
}

pub fn run(spec: &CommandSpec, config: &RunConfig) -> Result<Vec<Sample>, BenchguardError> {
    if config.runs == 0 {
        return Err(BenchguardError::ZeroRuns);
    }

    for _ in 0..config.warmups {
        run_once(spec, config.timeout)?;
    }

    (0..config.runs)
        .map(|_| run_once(spec, config.timeout))
        .collect()
}

#[cfg(target_os = "linux")]
fn run_once(spec: &CommandSpec, timeout: Option<Duration>) -> Result<Sample, BenchguardError> {
    linux::platform_run_once(spec, timeout)
}

#[cfg(windows)]
fn run_once(spec: &CommandSpec, timeout: Option<Duration>) -> Result<Sample, BenchguardError> {
    windows::platform_run_once(spec, timeout)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn run_once(_spec: &CommandSpec, _timeout: Option<Duration>) -> Result<Sample, BenchguardError> {
    unreachable!("unsupported targets are rejected by the crate-level compile error")
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, process::Command, sync::OnceLock};

    use super::*;
    use crate::error::BenchguardError;

    fn fixture_target_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("benchguard-runner-fixture-tests")
    }

    fn fixture_path() -> PathBuf {
        static FIXTURE_PATH: OnceLock<PathBuf> = OnceLock::new();

        FIXTURE_PATH
            .get_or_init(|| {
                let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                let output = Command::new("cargo")
                    .args([
                        "build",
                        "--quiet",
                        "--package",
                        "benchguard-fixture",
                        "--message-format=json",
                    ])
                    .current_dir(&manifest_dir)
                    .env("CARGO_TARGET_DIR", fixture_target_dir())
                    .output()
                    .expect("fixture build command should launch");
                assert!(
                    output.status.success(),
                    "fixture build should succeed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );

                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                    .find_map(|message| {
                        (message["reason"] == "compiler-artifact"
                            && message["target"]["name"] == "benchguard-fixture")
                            .then(|| message["executable"].as_str().map(PathBuf::from))
                            .flatten()
                    })
                    .expect("fixture build should report its executable path")
            })
            .clone()
    }

    #[test]
    fn preserves_argument_boundaries_and_collects_requested_runs() {
        let spec = CommandSpec::new(
            fixture_path(),
            [
                "assert-exact-args",
                "two words",
                "",
                "embedded \"quote\"",
                "trailing backslashes \\\\",
            ],
        );
        let samples = run(
            &spec,
            &RunConfig {
                warmups: 1,
                runs: 3,
                timeout: None,
            },
        )
        .unwrap();

        assert_eq!(samples.len(), 3);
        assert!(samples.iter().all(|sample| sample.wall_ns > 0));
        assert!(samples.iter().all(|sample| sample.exit_code == 0));
    }

    #[test]
    fn builds_the_fixture_in_a_test_owned_target_directory() {
        assert!(fixture_path().starts_with(fixture_target_dir()));
    }

    #[test]
    fn rejects_zero_measured_runs() {
        let spec = CommandSpec::new(fixture_path(), ["sleep-ms", "1"]);

        assert!(matches!(
            run(
                &spec,
                &RunConfig {
                    warmups: 0,
                    runs: 0,
                    timeout: None,
                },
            ),
            Err(BenchguardError::ZeroRuns)
        ));
    }

    #[test]
    fn rejects_non_zero_command_exit_codes() {
        let spec = CommandSpec::new(fixture_path(), ["exit", "23"]);

        assert!(matches!(
            run(
                &spec,
                &RunConfig {
                    warmups: 0,
                    runs: 1,
                    timeout: None,
                },
            ),
            Err(BenchguardError::CommandFailed { exit_code: 23 })
        ));
    }

    #[test]
    fn rejects_command_launch_failures() {
        let missing_program =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benchguard-missing-executable");
        let spec = CommandSpec::new(missing_program, [] as [&str; 0]);

        assert!(matches!(
            run(
                &spec,
                &RunConfig {
                    warmups: 0,
                    runs: 1,
                    timeout: None,
                },
            ),
            Err(BenchguardError::CommandLaunch { .. })
        ));
    }
}
