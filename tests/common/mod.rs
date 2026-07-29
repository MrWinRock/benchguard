use std::{
    path::{Path, PathBuf},
    process::Command as StdCommand,
    sync::OnceLock,
};

use assert_cmd::{Command, assert::Assert};
use tempfile::TempDir;

pub struct TestProject {
    root: TempDir,
}

impl TestProject {
    pub fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }

    pub fn baseline_path(&self) -> PathBuf {
        self.root.path().join("benchguard.json")
    }

    pub fn command(&self) -> Command {
        let mut command = Command::cargo_bin("benchguard").unwrap();
        command.current_dir(self.root.path());
        command
    }

    pub fn record_sleep(&self, name: &str, milliseconds: u64) -> Assert {
        let mut command = self.command();
        command.args([
            "record",
            name,
            "--runs",
            "3",
            "--warmup",
            "0",
            "--max-time",
            "+100000%",
            "--",
        ]);
        command
            .arg(fixture_path())
            .args(["sleep-ms", &milliseconds.to_string()])
            .assert()
    }

    pub fn check_stored(&self, name: &str) -> Assert {
        let mut command = self.command();
        command.args(["check", name]).assert()
    }

    pub fn check_sleep(&self, name: &str, milliseconds: u64) -> Assert {
        let mut command = self.command();
        command.args(["check", name, "--max-time", "+10%", "--"]);
        command
            .arg(fixture_path())
            .args(["sleep-ms", &milliseconds.to_string()])
            .assert()
    }

    pub fn record_exit(&self, name: &str, exit_code: i32) -> Assert {
        let mut command = self.command();
        command.args([
            "record",
            name,
            "--runs",
            "1",
            "--warmup",
            "0",
            "--max-time",
            "+10%",
            "--",
        ]);
        command
            .arg(fixture_path())
            .args(["exit", &exit_code.to_string()])
            .assert()
    }
}

fn fixture_target_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("benchguard-record-check-fixture-tests")
}

pub fn fixture_path() -> PathBuf {
    static FIXTURE_PATH: OnceLock<PathBuf> = OnceLock::new();

    FIXTURE_PATH
        .get_or_init(|| {
            let output = StdCommand::new("cargo")
                .args([
                    "build",
                    "--quiet",
                    "--package",
                    "benchguard-fixture",
                    "--message-format=json",
                ])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
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
