#![cfg(any(target_os = "linux", windows))]

#[allow(dead_code)]
mod common;

use std::fs;

#[cfg(windows)]
use std::process::Stdio;
#[cfg(windows)]
use std::time::Instant;
#[cfg(any(target_os = "linux", windows))]
use std::{thread, time::Duration};

use benchguard::runner::{CommandSpec, RunConfig, run};
use common::{TestProject, fixture_path};
#[cfg(target_os = "linux")]
use predicates::prelude::*;

// Catches routing a real timeout through a generic measurement/command code,
// writing JSON diagnostics to stderr, or omitting the additive warning list.
#[test]
fn timeout_returns_the_stable_structured_json_error() {
    let fixture = fixture_path();
    let project = TestProject::new();
    let output = project
        .command()
        .args([
            "record",
            "timeout",
            "--runs",
            "1",
            "--warmup",
            "0",
            "--timeout",
            "20ms",
            "--format",
            "json",
            "--",
        ])
        .arg(&fixture)
        .args(["sleep-ms", "1000"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["status"], "error");
    assert_eq!(report["benchmarks"], serde_json::json!([]));
    assert_eq!(report["warnings"], serde_json::json!([]));
    assert_eq!(report["errors"][0]["code"], "timeout");
    assert!(
        report["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("timed out")
    );
}

// Catches failed or timed-out targets contaminating the single JSON report
// document with inherited stdout/stderr.
#[test]
fn json_errors_suppress_verbose_target_output() {
    let fixture = fixture_path();
    let project = TestProject::new();

    let failed = project
        .command()
        .args([
            "record", "failed", "--runs", "1", "--warmup", "0", "--format", "json", "--",
        ])
        .arg(&fixture)
        .args(["verbose-exit", "23"])
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(2));
    assert!(failed.stderr.is_empty());
    let failed_report: serde_json::Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(failed_report["errors"][0]["code"], "command_failed");

    let timed_out = project
        .command()
        .args([
            "record",
            "timeout",
            "--runs",
            "1",
            "--warmup",
            "0",
            "--timeout",
            "20ms",
            "--format",
            "json",
            "--",
        ])
        .arg(&fixture)
        .args(["verbose-sleep-ms", "1000"])
        .output()
        .unwrap();
    assert_eq!(timed_out.status.code(), Some(2));
    assert!(timed_out.stderr.is_empty());
    let timeout_report: serde_json::Value = serde_json::from_slice(&timed_out.stdout).unwrap();
    assert_eq!(timeout_report["errors"][0]["code"], "timeout");
}

#[cfg(target_os = "linux")]
#[test]
fn linux_allocation_reports_at_least_thirty_mibibytes() {
    let samples = run(
        &CommandSpec::new(fixture_path(), ["allocate-mib", "32"]),
        &RunConfig {
            warmups: 0,
            runs: 1,
            timeout: None,
        },
    )
    .unwrap();

    assert!(samples[0].peak_memory_bytes >= 30 * 1024 * 1024);
}

#[cfg(target_os = "linux")]
#[test]
fn linux_timeout_exits_two_and_leaves_no_fixture_sleeper() {
    let fixture = fixture_path();
    let project = TestProject::new();
    project
        .command()
        .args([
            "record",
            "timeout",
            "--runs",
            "1",
            "--warmup",
            "0",
            "--timeout",
            "50ms",
            "--",
        ])
        .arg(&fixture)
        .arg("spawn-sleeper")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("timed out"));

    thread::sleep(Duration::from_millis(25));
    assert!(
        !fixture_sleeper_exists(&fixture),
        "fixture sleeper survived timeout cleanup"
    );
}

// Catches reaping and releasing a normally or unsuccessfully exited leader
// while a long-lived descendant remains in the managed process group.
#[cfg(target_os = "linux")]
#[test]
fn linux_leader_exit_cleans_remaining_group_descendants() {
    for exit_code in [0, 23] {
        let pid_dir = tempfile::tempdir().unwrap();
        let pid_path = pid_dir.path().join(format!("sleeper-{exit_code}.pid"));
        let result = run(
            &CommandSpec::new(
                fixture_path(),
                [
                    "spawn-sleeper-and-exit".into(),
                    pid_path.as_os_str().to_owned(),
                    exit_code.to_string().into(),
                ],
            ),
            &RunConfig {
                warmups: 0,
                runs: 1,
                timeout: None,
            },
        );
        if exit_code == 0 {
            assert!(result.is_ok());
        } else {
            assert!(matches!(
                result,
                Err(benchguard::error::BenchguardError::CommandFailed { exit_code: 23 })
            ));
        }

        thread::sleep(Duration::from_millis(25));
        assert!(
            !fixture_sleeper_exists(&fixture_path()),
            "fixture sleeper survived leader exit {exit_code}"
        );
    }
}

#[cfg(target_os = "linux")]
fn fixture_sleeper_exists(fixture: &std::path::Path) -> bool {
    let expected_executable = fixture.as_os_str().as_encoded_bytes();
    let Ok(entries) = fs::read_dir("/proc") else {
        return false;
    };

    entries.flatten().any(|entry| {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            return false;
        };
        let Ok(command_line) = fs::read(format!("/proc/{pid}/cmdline")) else {
            return false;
        };
        let mut args = command_line.split(|byte| *byte == 0);
        args.next() == Some(expected_executable)
            && args.next() == Some(b"sleep-ms")
            && args.next() == Some(b"30000")
    })
}

#[cfg(windows)]
#[test]
fn windows_descendant_allocation_reports_at_least_thirty_mibibytes() {
    let samples = run(
        &CommandSpec::new(fixture_path(), ["spawn-allocator"]),
        &RunConfig {
            warmups: 0,
            runs: 1,
            timeout: None,
        },
    )
    .unwrap();

    assert!(samples[0].peak_memory_bytes >= 30 * 1024 * 1024);
}

// Catches reporting Job Object commit charge as resident memory. Committing
// untouched pages reserves backing store but should not add 128 MiB to the
// process-tree working set.
#[cfg(windows)]
#[test]
fn windows_untouched_commit_is_not_reported_as_resident_memory() {
    let samples = run(
        &CommandSpec::new(fixture_path(), ["commit-untouched-mib", "128"]),
        &RunConfig {
            warmups: 0,
            runs: 1,
            timeout: None,
        },
    )
    .unwrap();

    assert!(
        samples[0].peak_memory_bytes < 64 * 1024 * 1024,
        "untouched commit was reported as resident: {} bytes",
        samples[0].peak_memory_bytes
    );
}

#[cfg(windows)]
#[test]
fn windows_descendant_cpu_reports_at_least_fifty_milliseconds() {
    let samples = run(
        &CommandSpec::new(fixture_path(), ["spawn-cpu-burner"]),
        &RunConfig {
            warmups: 0,
            runs: 1,
            timeout: None,
        },
    )
    .unwrap();

    assert!(samples[0].cpu_ns >= 50_000_000);
}

#[cfg(windows)]
#[test]
fn windows_normal_child_tree_exits_cleanly() {
    let samples = run(
        &CommandSpec::new(fixture_path(), ["spawn-child"]),
        &RunConfig {
            warmups: 0,
            runs: 1,
            timeout: None,
        },
    )
    .unwrap();

    assert_eq!(samples[0].exit_code, 0);
}

#[cfg(windows)]
#[test]
fn windows_preserves_spaces_empty_args_quotes_and_trailing_backslashes() {
    let samples = run(
        &CommandSpec::new(
            fixture_path(),
            [
                "assert-exact-args",
                "two words",
                "",
                "embedded \"quote\"",
                "trailing backslashes \\\\",
            ],
        ),
        &RunConfig {
            warmups: 0,
            runs: 1,
            timeout: None,
        },
    )
    .unwrap();

    assert_eq!(samples[0].exit_code, 0);
}

#[cfg(windows)]
#[test]
fn windows_bare_program_uses_path_not_cwd_and_appends_exe() {
    use std::{
        ffi::OsString,
        path::{Path, PathBuf},
        sync::{Mutex, OnceLock},
    };

    static PATH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _path_lock = PATH_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = fixture_path();
    let path_dir = tempfile::tempdir().unwrap();
    let executable_stem = format!("benchguard-path-resolution-{}", std::process::id());
    let executable_name = format!("{executable_stem}.exe");
    fs::copy(&fixture, path_dir.path().join(&executable_name)).unwrap();

    let current_dir = std::env::current_dir().unwrap();
    let shadow_path = current_dir.join(&executable_name);
    let system_root = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
    fs::copy(system_root.join("System32").join("where.exe"), &shadow_path).unwrap();
    let _shadow = ScopedFile(shadow_path);
    let _path = PathGuard::set(path_dir.path());

    let samples = run(
        &CommandSpec::new(
            &executable_stem,
            [
                "assert-exact-args",
                "two words",
                "",
                "embedded \"quote\"",
                "trailing backslashes \\\\",
            ],
        ),
        &RunConfig {
            warmups: 0,
            runs: 1,
            timeout: None,
        },
    )
    .unwrap();

    assert_eq!(samples[0].exit_code, 0);

    struct PathGuard(Option<OsString>);

    impl PathGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::var_os("PATH");
            // SAFETY: this test serializes its PATH mutation and all other
            // Windows runner tests launch explicit executable paths.
            unsafe {
                std::env::set_var("PATH", path);
            }
            Self(previous)
        }
    }

    impl Drop for PathGuard {
        fn drop(&mut self) {
            // SAFETY: the same test-owned serialization covers restoration.
            unsafe {
                match self.0.take() {
                    Some(previous) => std::env::set_var("PATH", previous),
                    None => std::env::remove_var("PATH"),
                }
            }
        }
    }

    struct ScopedFile(PathBuf);

    impl Drop for ScopedFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
}

#[cfg(windows)]
#[test]
fn windows_relative_program_with_separator_stays_explicit() {
    let fixture = fixture_path();
    let current_dir = std::env::current_dir().unwrap();
    let relative_fixture = fixture
        .strip_prefix(&current_dir)
        .expect("fixture should be built below the workspace")
        .to_path_buf();
    assert!(relative_fixture.components().count() > 1);

    let samples = run(
        &CommandSpec::new(relative_fixture, ["spawn-child"]),
        &RunConfig {
            warmups: 0,
            runs: 1,
            timeout: None,
        },
    )
    .unwrap();

    assert_eq!(samples[0].exit_code, 0);
}

// Catches bInheritHandles=TRUE leaking every inheritable BenchGuard handle
// into the benchmark target instead of only its intentional null stdio.
#[cfg(windows)]
#[test]
fn windows_target_does_not_inherit_unrelated_inheritable_handles() {
    use windows_sys::Win32::{
        Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
        Security::SECURITY_ATTRIBUTES,
        System::Threading::{CreateEventW, WaitForSingleObject},
    };

    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>()).unwrap(),
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    // SAFETY: attributes requests an unnamed inheritable manual-reset event
    // initially in the nonsignaled state.
    let event = unsafe { CreateEventW(&attributes, 1, 0, std::ptr::null()) };
    assert!(!event.is_null(), "{}", std::io::Error::last_os_error());
    let event = WindowsTestHandle(event);

    let result = run(
        &CommandSpec::new(
            fixture_path(),
            ["signal-event-handle", &(event.0 as usize).to_string()],
        ),
        &RunConfig {
            warmups: 0,
            runs: 1,
            timeout: None,
        },
    );
    assert!(result.is_ok());

    // SAFETY: event remains a live synchronization handle.
    let wait_result = unsafe { WaitForSingleObject(event.0, 0) };
    assert_ne!(
        wait_result, WAIT_OBJECT_0,
        "benchmark target signaled an unrelated inheritable event"
    );
    assert_eq!(wait_result, WAIT_TIMEOUT, "unexpected event wait result");
}

#[cfg(windows)]
#[test]
fn windows_timeout_exits_two_and_leaves_no_fixture_sleeper() {
    let fixture = fixture_path();
    let project = TestProject::new();
    let pid_dir = tempfile::tempdir().unwrap();
    let pid_path = pid_dir.path().join("sleeper.pid");
    let mut command = {
        let configured = project.command();
        let mut command = std::process::Command::new(configured.get_program());
        command.args(configured.get_args());
        if let Some(current_dir) = configured.get_current_dir() {
            command.current_dir(current_dir);
        }
        command
    };
    command
        .args([
            "record",
            "timeout",
            "--runs",
            "1",
            "--warmup",
            "0",
            "--timeout",
            "10s",
            "--",
        ])
        .arg(&fixture)
        .arg("spawn-sleeper")
        .arg(&pid_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let benchguard = command.spawn().expect("benchguard should launch");
    // Keep a generous bounded setup window inside the benchmark deadline.
    // Parallel Windows CI can delay both process creation and the atomic PID
    // publication; the remaining five seconds keep setup failure distinct
    // from the timeout behavior this test exercises.
    let sleeper = wait_for_published_pid(&pid_path, Duration::from_secs(5))
        .and_then(open_windows_process_for_exit_observation);
    let output = benchguard
        .wait_with_output()
        .expect("benchguard should return after its timeout");

    assert_eq!(
        output.status.code(),
        Some(2),
        "unexpected timeout status; stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("timed out"),
        "timeout diagnostic missing; stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let (sleeper_pid, sleeper) = sleeper.unwrap_or_else(|error| {
        panic!(
            "{error}; stdout: {}; stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert!(
        windows_process_has_exited(&sleeper, sleeper_pid),
        "fixture sleeper was still alive when timeout returned"
    );
}

#[cfg(windows)]
fn wait_for_published_pid(
    pid_path: &std::path::Path,
    setup_timeout: Duration,
) -> Result<u32, String> {
    let deadline = Instant::now() + setup_timeout;
    loop {
        match fs::read_to_string(pid_path) {
            Ok(contents) if !contents.trim().is_empty() => {
                return contents.trim().parse::<u32>().map_err(|source| {
                    format!(
                        "fixture published an invalid sleeper PID at {}: {source}",
                        pid_path.display()
                    )
                });
            }
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(format!(
                    "failed to read fixture sleeper PID at {}: {source}",
                    pid_path.display()
                ));
            }
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "fixture did not publish its sleeper PID within {} ms",
                setup_timeout.as_millis()
            ));
        }
        thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(windows)]
fn open_windows_process_for_exit_observation(pid: u32) -> Result<(u32, WindowsTestHandle), String> {
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};

    // SAFETY: OpenProcess receives the concrete PID published atomically by
    // the fixture and requests only synchronization access.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return Err(format!(
            "failed to retain fixture sleeper PID {pid} for exit observation: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok((pid, WindowsTestHandle(handle)))
}

#[cfg(windows)]
fn windows_process_has_exited(process: &WindowsTestHandle, pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::{WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::WaitForSingleObject,
    };

    // SAFETY: process retains the exact live process handle opened after the
    // fixture published its PID, so PID reuse cannot affect this observation.
    let wait_result = unsafe { WaitForSingleObject(process.0, 0) };
    let wait_error = (wait_result == WAIT_FAILED).then(std::io::Error::last_os_error);
    match wait_result {
        WAIT_OBJECT_0 => true,
        WAIT_TIMEOUT => false,
        WAIT_FAILED => panic!(
            "failed to wait on fixture sleeper PID {pid}: {}",
            wait_error.expect("WAIT_FAILED should capture the OS error")
        ),
        result => panic!("unexpected Windows wait result {result} for PID {pid}"),
    }
}

#[cfg(windows)]
struct WindowsTestHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for WindowsTestHandle {
    fn drop(&mut self) {
        // SAFETY: the wrapper is constructed only from a non-null owned handle
        // and cannot be cloned, so Drop is the unique closing path.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
