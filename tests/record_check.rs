mod common;

use std::fs;

use predicates::prelude::*;

use common::{TestProject, fixture_path};

// Catches missing record/check orchestration, failure to use the stored
// command, or incorrect success/regression exit-code mapping.
#[test]
fn record_then_stored_check_passes_and_override_regression_exits_one() {
    let project = TestProject::new();
    project
        .record_sleep("startup", 10)
        .success()
        .stdout(predicate::str::contains("RECORDED"));
    let baseline: serde_json::Value =
        serde_json::from_slice(&fs::read(project.baseline_path()).unwrap()).unwrap();
    assert_eq!(
        baseline["benchmarks"]["startup"]["budgets"]["wall_percent"],
        100_000.0
    );
    project
        .check_stored("startup")
        .success()
        .stdout(predicate::str::contains("PASS"))
        .stdout(predicate::str::contains("samples: 3"));
    project
        .check_sleep("startup", 80)
        .failure()
        .code(1)
        .stdout(predicate::str::contains("REGRESSION"));
    project
        .check_stored("startup")
        .success()
        .stdout(predicate::str::contains("PASS"));
}

// Catches an explicit color choice being ignored, Auto coloring redirected
// command output, NO_COLOR being ignored, or JSON carrying terminal escapes.
#[test]
fn command_color_modes_follow_the_human_output_contract() {
    let project = TestProject::new();
    let fixture = fixture_path();

    let run = |color: &str, format: Option<&str>, no_color: bool| {
        let mut command = project.command();
        command.args([
            "--color", color, "record", "colors", "--runs", "1", "--warmup", "0",
        ]);
        if let Some(format) = format {
            command.args(["--format", format]);
        }
        if no_color {
            command.env("NO_COLOR", "1");
        }
        command
            .arg(&fixture)
            .args(["sleep-ms", "1"])
            .output()
            .unwrap()
    };

    let human_with_always = run("always", None, false);
    assert!(human_with_always.status.success());
    assert!(String::from_utf8_lossy(&human_with_always.stdout).contains("\u{1b}["));

    let human_with_never = run("never", None, false);
    assert!(human_with_never.status.success());
    assert!(!String::from_utf8_lossy(&human_with_never.stdout).contains("\u{1b}["));

    let redirected_auto = run("auto", None, false);
    assert!(redirected_auto.status.success());
    assert!(!String::from_utf8_lossy(&redirected_auto.stdout).contains("\u{1b}["));

    let auto_with_no_color = run("auto", None, true);
    assert!(auto_with_no_color.status.success());
    assert!(!String::from_utf8_lossy(&auto_with_no_color.stdout).contains("\u{1b}["));

    let json_with_always = run("always", Some("json"), false);
    assert!(json_with_always.status.success());
    assert!(!String::from_utf8_lossy(&json_with_always.stdout).contains("\u{1b}["));
}

// Catches retaining the temporary budget rejection, recording placeholder CPU
// values, ignoring descendant CPU, or failing to let any metric regress the
// overall command.
#[test]
fn cpu_budget_records_passes_and_regresses_on_real_descendant_work() {
    let project = TestProject::new();
    let fixture = fixture_path();
    let record_output = project
        .command()
        .args([
            "record",
            "cpu",
            "--runs",
            "2",
            "--warmup",
            "0",
            "--max-cpu",
            "+10%",
            "--format",
            "json",
            "--",
        ])
        .arg(&fixture)
        .args(["spawn-cpu-burner", "80"])
        .output()
        .unwrap();
    assert!(record_output.status.success());
    let recorded: serde_json::Value = serde_json::from_slice(&record_output.stdout).unwrap();
    assert_eq!(recorded["benchmarks"][0]["cpu_time"]["status"], "recorded");
    assert_eq!(recorded["benchmarks"][0]["cpu_time"]["unit"], "ns");
    assert_eq!(
        recorded["benchmarks"][0]["cpu_time"]["absolute_floor"],
        1_000_000
    );

    let baseline: serde_json::Value =
        serde_json::from_slice(&fs::read(project.baseline_path()).unwrap()).unwrap();
    assert_eq!(
        baseline["benchmarks"]["cpu"]["budgets"]["cpu_percent"],
        10.0
    );
    assert!(
        baseline["benchmarks"]["cpu"]["cpu_ns"]["median"]
            .as_u64()
            .unwrap()
            >= 50_000_000
    );

    let pass_output = project
        .command()
        .args([
            "check",
            "cpu",
            "--runs",
            "2",
            "--warmup",
            "0",
            "--max-cpu",
            "+100000%",
            "--format",
            "json",
            "--",
        ])
        .arg(&fixture)
        .args(["spawn-cpu-burner", "80"])
        .output()
        .unwrap();
    assert!(pass_output.status.success());
    let passed: serde_json::Value = serde_json::from_slice(&pass_output.stdout).unwrap();
    assert_eq!(passed["benchmarks"][0]["status"], "pass");
    assert_eq!(passed["benchmarks"][0]["cpu_time"]["status"], "pass");
    assert_eq!(
        passed["benchmarks"][0]["peak_memory"]["status"],
        "unbudgeted"
    );

    let regression_output = project
        .command()
        .args([
            "check",
            "cpu",
            "--runs",
            "2",
            "--warmup",
            "0",
            "--max-cpu",
            "+10%",
            "--format",
            "json",
            "--",
        ])
        .arg(&fixture)
        .args(["spawn-cpu-burner", "350"])
        .output()
        .unwrap();
    assert_eq!(regression_output.status.code(), Some(1));
    let regression: serde_json::Value = serde_json::from_slice(&regression_output.stdout).unwrap();
    assert_eq!(regression["status"], "regression");
    assert_eq!(regression["benchmarks"][0]["status"], "regression");
    assert_eq!(
        regression["benchmarks"][0]["cpu_time"]["status"],
        "regression"
    );
}

// Catches measuring only the leader's memory, retaining placeholder memory,
// or applying a decimal/default floor instead of one mebibyte.
#[test]
fn peak_memory_budget_records_passes_and_regresses_on_real_descendant_work() {
    let project = TestProject::new();
    let fixture = fixture_path();
    let record_output = project
        .command()
        .args([
            "record",
            "memory",
            "--runs",
            "2",
            "--warmup",
            "0",
            "--max-memory",
            "+10%",
            "--format",
            "json",
            "--",
        ])
        .arg(&fixture)
        .args(["spawn-allocator", "4"])
        .output()
        .unwrap();
    assert!(record_output.status.success());
    let recorded: serde_json::Value = serde_json::from_slice(&record_output.stdout).unwrap();
    assert_eq!(
        recorded["benchmarks"][0]["peak_memory"]["status"],
        "recorded"
    );
    assert_eq!(recorded["benchmarks"][0]["peak_memory"]["unit"], "bytes");
    assert_eq!(
        recorded["benchmarks"][0]["peak_memory"]["absolute_floor"],
        1_048_576
    );

    let baseline: serde_json::Value =
        serde_json::from_slice(&fs::read(project.baseline_path()).unwrap()).unwrap();
    assert_eq!(
        baseline["benchmarks"]["memory"]["budgets"]["peak_memory_percent"],
        10.0
    );
    assert!(
        baseline["benchmarks"]["memory"]["peak_memory_bytes"]["median"]
            .as_u64()
            .unwrap()
            >= 4 * 1024 * 1024
    );

    let pass_output = project
        .command()
        .args([
            "check",
            "memory",
            "--runs",
            "2",
            "--warmup",
            "0",
            "--max-memory",
            "+100000%",
            "--format",
            "json",
            "--",
        ])
        .arg(&fixture)
        .args(["spawn-allocator", "4"])
        .output()
        .unwrap();
    assert!(pass_output.status.success());
    let passed: serde_json::Value = serde_json::from_slice(&pass_output.stdout).unwrap();
    assert_eq!(passed["benchmarks"][0]["status"], "pass");
    assert_eq!(passed["benchmarks"][0]["peak_memory"]["status"], "pass");

    let regression_output = project
        .command()
        .args([
            "check",
            "memory",
            "--runs",
            "2",
            "--warmup",
            "0",
            "--max-memory",
            "+10%",
            "--format",
            "json",
            "--",
        ])
        .arg(&fixture)
        .args(["spawn-allocator", "32"])
        .output()
        .unwrap();
    assert_eq!(regression_output.status.code(), Some(1));
    let regression: serde_json::Value = serde_json::from_slice(&regression_output.stdout).unwrap();
    assert_eq!(regression["status"], "regression");
    assert_eq!(
        regression["benchmarks"][0]["peak_memory"]["status"],
        "regression"
    );
}

// Catches record launching a target before it has validated the existing
// baseline that it would update.
#[test]
fn malformed_existing_baseline_is_rejected_before_command_launch() {
    let project = TestProject::new();
    fs::write(project.baseline_path(), b"{broken").unwrap();

    project
        .command()
        .args(["record", "startup", "--", "definitely-missing-program"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("failed to parse baseline"))
        .stderr(predicate::str::contains("failed to launch").not())
        .stdout(predicate::str::is_empty());
}

// Catches comparing samples across incompatible platforms or selecting the
// stored command before platform compatibility has been validated.
#[test]
fn incompatible_platform_is_rejected_before_command_launch() {
    let project = TestProject::new();
    project.record_sleep("startup", 10).success();

    let mut baseline: serde_json::Value =
        serde_json::from_slice(&fs::read(project.baseline_path()).unwrap()).unwrap();
    baseline["benchmarks"]["startup"]["platform"]["os"] =
        serde_json::Value::String("definitely-not-current-os".to_owned());
    baseline["benchmarks"]["startup"]["platform"]["arch"] =
        serde_json::Value::String("definitely-not-current-arch".to_owned());
    baseline["benchmarks"]["startup"]["program"] =
        serde_json::Value::String("definitely-missing-program".to_owned());
    fs::write(
        project.baseline_path(),
        serde_json::to_vec_pretty(&baseline).unwrap(),
    )
    .unwrap();

    project
        .check_stored("startup")
        .failure()
        .code(2)
        .stderr(predicate::str::contains("does not match current platform"))
        .stderr(predicate::str::contains("failed to launch").not());
}

// Catches trusting tampered aggregate order statistics and launching a stored
// command before all persisted measurement invariants are validated.
#[test]
fn impossible_aggregate_is_rejected_before_command_launch() {
    let project = TestProject::new();
    project.record_sleep("startup", 10).success();

    let mut baseline: serde_json::Value =
        serde_json::from_slice(&fs::read(project.baseline_path()).unwrap()).unwrap();
    baseline["benchmarks"]["startup"]["wall_ns"]["p50"] =
        baseline["benchmarks"]["startup"]["wall_ns"]["max"].clone();
    baseline["benchmarks"]["startup"]["wall_ns"]["median"] = serde_json::Value::from(u64::MAX);
    baseline["benchmarks"]["startup"]["program"] =
        serde_json::Value::String("definitely-missing-program".to_owned());
    fs::write(
        project.baseline_path(),
        serde_json::to_vec_pretty(&baseline).unwrap(),
    )
    .unwrap();

    project
        .check_stored("startup")
        .failure()
        .code(2)
        .stderr(predicate::str::contains("invalid baseline"))
        .stderr(predicate::str::contains("failed to launch").not());
}

// Catches JSON requests falling back to the human stderr path or emitting
// unstable string-only errors instead of the fixed machine envelope.
#[test]
fn json_operational_errors_use_the_fixed_envelope_on_stdout() {
    let project = TestProject::new();
    fs::write(project.baseline_path(), b"{broken").unwrap();

    let output = project
        .command()
        .args(["list", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["status"], "error");
    assert_eq!(report["benchmarks"], serde_json::json!([]));
    assert_eq!(report["warnings"], serde_json::json!([]));
    assert_eq!(report["errors"].as_array().unwrap().len(), 1);
    assert_eq!(report["errors"][0]["code"], "invalid_baseline");
    assert!(
        report["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("failed to parse baseline")
    );
    assert_eq!(
        report["errors"][0]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        ["code", "message"]
    );
}

// Catches target output sharing BenchGuard's report streams. A successful
// benchmark may be noisy, but JSON mode must still emit exactly one document.
#[test]
fn json_success_suppresses_target_stdout_and_stderr() {
    let project = TestProject::new();
    let output = project
        .command()
        .args([
            "record", "verbose", "--runs", "1", "--warmup", "0", "--format", "json", "--",
        ])
        .arg(fixture_path())
        .args(["verbose-exit", "0"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "ok");
}

// Catches clap terminating before BenchGuard can render the documented JSON
// error envelope. The scanner must honor the target delimiter so a target
// argument named --format cannot select JSON mode.
#[test]
fn invalid_arguments_honor_json_format_intent_before_target_delimiter() {
    let project = TestProject::new();
    let output = project
        .command()
        .args(["record", "--runs", "0", "--format=json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "error");
    assert_eq!(report["errors"][0]["code"], "invalid_arguments");

    let target_format = project
        .command()
        .args([
            "record",
            "missing-target",
            "--",
            "program",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(target_format.status.code(), Some(2));
    assert!(target_format.stdout.is_empty());
    assert!(!target_format.stderr.is_empty());
}

// Catches replacing the entire baseline map when recording one name and
// catches mutating the old file when a later command fails.
#[test]
fn record_replaces_only_the_named_entry_and_failure_preserves_bytes() {
    let project = TestProject::new();
    project.record_sleep("startup", 10).success();
    project.record_sleep("shutdown", 20).success();

    let before_failure = fs::read(project.baseline_path()).unwrap();
    project.record_exit("startup", 23).failure().code(2);
    assert_eq!(fs::read(project.baseline_path()).unwrap(), before_failure);

    let baseline: serde_json::Value = serde_json::from_slice(&before_failure).unwrap();
    assert!(baseline["benchmarks"]["startup"].is_object());
    assert!(baseline["benchmarks"]["shutdown"].is_object());
}

// Catches list accidentally dispatching the stored executable instead of
// performing a read-only baseline projection.
#[test]
fn list_never_runs_the_stored_command() {
    let project = TestProject::new();
    project.record_sleep("startup", 10).success();

    let mut baseline: serde_json::Value =
        serde_json::from_slice(&fs::read(project.baseline_path()).unwrap()).unwrap();
    baseline["benchmarks"]["startup"]["program"] =
        serde_json::Value::String("definitely-missing-program".to_owned());
    fs::write(
        project.baseline_path(),
        serde_json::to_vec_pretty(&baseline).unwrap(),
    )
    .unwrap();

    project
        .command()
        .args(["list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BASELINE"));
}

// Catches JSON shape drift and verifies improvements retain a signed delta.
#[test]
fn json_output_is_consistent_across_record_check_and_list() {
    let project = TestProject::new();
    let fixture = fixture_path();

    let record_output = project
        .command()
        .args([
            "record",
            "startup",
            "--runs",
            "3",
            "--warmup",
            "0",
            "--max-time",
            "+10%",
            "--format",
            "json",
            "--",
        ])
        .arg(&fixture)
        .args(["sleep-ms", "20"])
        .output()
        .unwrap();
    assert!(record_output.status.success());
    let recorded: serde_json::Value = serde_json::from_slice(&record_output.stdout).unwrap();
    assert_eq!(recorded["status"], "ok");
    assert!(recorded["warnings"].is_array());
    assert_eq!(recorded["benchmarks"][0]["status"], "recorded");
    assert!(recorded["benchmarks"][0]["current_median_ns"].is_null());
    assert!(recorded["benchmarks"][0]["delta_ns"].is_null());
    assert_eq!(recorded["benchmarks"][0]["cpu_time"]["status"], "recorded");
    assert_eq!(
        recorded["benchmarks"][0]["peak_memory"]["status"],
        "recorded"
    );

    let check_output = project
        .command()
        .args(["check", "startup", "--format", "json", "--"])
        .arg(&fixture)
        .args(["sleep-ms", "0"])
        .output()
        .unwrap();
    assert!(check_output.status.success());
    let checked: serde_json::Value = serde_json::from_slice(&check_output.stdout).unwrap();
    assert_eq!(checked["status"], "ok");
    assert!(checked["warnings"].is_array());
    assert!(checked["benchmarks"][0]["delta_ns"].as_i64().unwrap() < 0);
    assert!(
        checked["benchmarks"][0]["relative_delta_pct"]
            .as_f64()
            .unwrap()
            < 0.0
    );

    let list_output = project
        .command()
        .args(["list", "--format", "json"])
        .output()
        .unwrap();
    assert!(list_output.status.success());
    let listed: serde_json::Value = serde_json::from_slice(&list_output.stdout).unwrap();
    assert_eq!(listed["status"], "ok");
    assert_eq!(listed["warnings"], serde_json::json!([]));
    assert_eq!(listed["benchmarks"][0]["status"], "baseline");
    assert!(listed["benchmarks"][0]["current_median_ns"].is_null());
    assert_eq!(listed["benchmarks"][0]["cpu_time"]["status"], "baseline");
    assert_eq!(listed["benchmarks"][0]["peak_memory"]["status"], "baseline");

    let keys = listed["benchmarks"][0]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [
            "absolute_floor_ns",
            "args",
            "baseline_median_ns",
            "budget_pct",
            "cpu_time",
            "current_median_ns",
            "delta_ns",
            "name",
            "peak_memory",
            "platform",
            "program",
            "relative_delta_pct",
            "sample_count",
            "status",
        ]
    );
}

// Catches computing warnings without attaching them to command reports,
// emitting them only in one format, or allowing a warning to change success.
#[test]
fn high_wall_variability_is_reported_in_json_and_human_without_failing() {
    let project = TestProject::new();
    let fixture = fixture_path();
    let state_dir = tempfile::tempdir().unwrap();
    let record_state = state_dir.path().join("record-sequence");

    let record_output = project
        .command()
        .args([
            "record",
            "variable",
            "--runs",
            "3",
            "--warmup",
            "0",
            "--max-time",
            "+100000%",
            "--format",
            "json",
            "--",
        ])
        .arg(&fixture)
        .args(["vary-sleep-ms", record_state.to_str().unwrap(), "0", "100"])
        .output()
        .unwrap();
    assert!(record_output.status.success());
    assert!(record_output.stderr.is_empty());
    let recorded: serde_json::Value = serde_json::from_slice(&record_output.stdout).unwrap();
    assert_eq!(recorded["warnings"].as_array().unwrap().len(), 1);
    assert_eq!(recorded["warnings"][0]["code"], "high_variability");
    assert!(
        recorded["warnings"][0]["message"]
            .as_str()
            .unwrap()
            .contains("variable wall-time coefficient of variation")
    );

    let check_state = state_dir.path().join("check-sequence");
    project
        .command()
        .args([
            "check",
            "variable",
            "--runs",
            "3",
            "--warmup",
            "0",
            "--max-time",
            "+100000%",
            "--",
        ])
        .arg(&fixture)
        .args(["vary-sleep-ms", check_state.to_str().unwrap(), "0", "100"])
        .assert()
        .success()
        .stdout(predicate::str::contains("warning [high_variability]"))
        .stdout(predicate::str::contains(
            "variable wall-time coefficient of variation",
        ));
}
