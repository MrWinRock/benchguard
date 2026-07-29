# BenchGuard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and publish BenchGuard v0.1, a native Rust CLI that records JSON performance baselines and fails CI when commands regress on Windows or Linux.

**Architecture:** A thin `clap` CLI delegates to independent runner, statistics, baseline, comparison, and reporting modules. Platform backends own measurement and cleanup for their documented managed execution scope; all cross-platform values are normalized into nanoseconds and bytes before entering the domain model.

**Tech Stack:** Rust 2024 edition, `clap`, `serde`, `serde_json`, `thiserror`, `humantime`, `tempfile`, `libc`, `windows-sys`, Criterion, GitHub Actions, and an npm JavaScript launcher that dispatches to platform-specific native packages.

## Global Constraints

- Support Windows and Linux on x86-64; reject cross-platform or cross-architecture baseline comparisons.
- Launch an executable plus its argument array directly; never invoke an implicit shell.
- Public commands are `record`, `check`, `list`, and `help`.
- Common aliases are `-r/--runs`, `-w/--warmup`, `-t/--timeout`, and `-f/--file`.
- Default measured runs: `10`; default warm-ups: `2`; default baseline file: `benchguard.json`.
- Persist durations as integer nanoseconds and memory as integer bytes.
- Use the median for regression comparisons.
- A regression must exceed both its configured relative budget and its absolute noise floor.
- Default absolute floors: 1 ms wall time, 1 ms CPU time, and 1 MiB peak memory.
- Exit `0` on success, `1` on a performance regression, and `2` on every operational or configuration error.
- Any failed warm-up or measured run aborts the benchmark.
- A failed record operation must leave the prior baseline byte-for-byte intact.
- npm is an installer/launcher only; measurement runs in the native Rust binary.
- Defer macOS, ARM, a TUI, hosted storage, plugins, and Git-aware baseline selection.

## Planned file structure

```text
benchguard/
├── Cargo.toml                         workspace/package metadata and dependencies
├── Cargo.lock                        reproducible dependency resolution
├── README.md                         installation and quick start
├── LICENSE-MIT
├── LICENSE-APACHE
├── CHANGELOG.md
├── CONTRIBUTING.md
├── CODE_OF_CONDUCT.md
├── src/
│   ├── main.rs                       process entry point and exit-code mapping
│   ├── lib.rs                        public internal module boundary
│   ├── cli.rs                        clap command and option definitions
│   ├── app.rs                        command orchestration
│   ├── error.rs                      typed operational errors
│   ├── domain.rs                     normalized samples, aggregates, and platform ID
│   ├── stats.rs                      deterministic aggregate calculations
│   ├── comparison.rs                 budgets, noise floors, and outcomes
│   ├── runner/
│   │   ├── mod.rs                    platform-neutral runner interface
│   │   ├── linux.rs                  Linux process group, metrics, and cleanup
│   │   └── windows.rs                Windows Job Object, metrics, and cleanup
│   ├── baseline/
│   │   ├── mod.rs                    baseline public API
│   │   ├── schema.rs                 versioned serde structures
│   │   └── store.rs                  validation and atomic persistence
│   └── report/
│       ├── mod.rs                    report model and renderer interface
│       ├── human.rs                  terminal report
│       └── json.rs                   stable JSON report envelope
├── crates/
│   └── benchguard-fixture/
│       ├── Cargo.toml
│       └── src/main.rs               predictable integration-test target
├── tests/
│   ├── cli_help.rs
│   ├── record_check.rs
│   ├── failures.rs
│   └── common/mod.rs                 binary and fixture helpers
├── benches/
│   └── core.rs                       statistics and serialization benchmarks
├── npm/
│   ├── cli/
│   │   ├── package.json
│   │   └── bin/benchguard.js
│   ├── linux-x64/package.json
│   └── win32-x64/package.json
└── .github/
    └── workflows/
        ├── ci.yml
        └── release.yml
```

## Scrum delivery map

| Sprint | Goal | Tasks | Story points | Demonstrable increment |
|---|---|---:|---:|---|
| 1 | Execute and summarize arbitrary commands | 1–3 | 13 | Native CLI measures wall time and prints statistics |
| 2 | Record and enforce JSON baselines | 4–6 | 18 | CI can fail on a wall-time regression |
| 3 | Add dependable system metrics and cleanup | 7–9 | 21 | CPU/memory budgets and managed-scope timeouts work on both platforms |
| 4 | Publish an open-source v0.1 | 10–12 | 16 | npm, Cargo, and release binaries are installable |

Each task is a review gate and ends in a focused commit. If a task is unfinished at a sprint boundary, move the whole task; do not split an unverified change across sprints.

## Execution preflight

Run implementation in a new `benchguard` Git repository, not in the projectless planning directory:

```text
mkdir benchguard
cd benchguard
git init
git branch -M main
```

Copy the approved design and this plan into `docs/superpowers/specs/2026-07-28-benchguard-design.md` and `docs/superpowers/plans/2026-07-28-benchguard.md`, then commit them as `docs: add benchguard design and implementation plan`. At execution time, use the `superpowers:using-git-worktrees` skill before feature work when an isolated worktree is appropriate.

---

## Sprint 1 — Benchmark engine

### Task 1: Repository skeleton and CLI contract

**Story points:** 3

**Files:**

- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/lib.rs`
- Create: `src/cli.rs`
- Create: `src/error.rs`
- Create: `tests/cli_help.rs`

**Interfaces:**

- Produces: `cli::Cli`, `cli::Command`, `cli::RecordArgs`, `cli::CheckArgs`, and `cli::ListArgs`.
- Produces: `error::BenchguardError` and `error::ExitClass`.

- [ ] **Step 1: Create the package and write failing CLI tests**

```rust
// tests/cli_help.rs
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn top_level_help_lists_public_commands() {
    Command::cargo_bin("benchguard").unwrap()
        .args(["help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("record"))
        .stdout(predicate::str::contains("check"))
        .stdout(predicate::str::contains("list"));
}

#[test]
fn record_help_lists_short_aliases() {
    Command::cargo_bin("benchguard").unwrap()
        .args(["record", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("-r, --runs"))
        .stdout(predicate::str::contains("-w, --warmup"))
        .stdout(predicate::str::contains("-t, --timeout"))
        .stdout(predicate::str::contains("-f, --file"));
}
```

- [ ] **Step 2: Run the tests and verify the binary is missing**

Run: `cargo test --test cli_help`  
Expected: FAIL because the `benchguard` binary and CLI do not exist.

- [ ] **Step 3: Add package metadata and CLI definitions**

```toml
# Cargo.toml
[package]
name = "benchguard"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "MIT OR Apache-2.0"
description = "Performance regression budgets for any command"

[dependencies]
clap = { version = "4", features = ["derive"] }
humantime = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
tempfile = "3"
```

```rust
// src/cli.rs
use std::path::PathBuf;
use std::time::Duration;
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "benchguard", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Record(RecordArgs),
    Check(CheckArgs),
    List(ListArgs),
}

#[derive(Debug, Args)]
pub struct RunOptions {
    #[arg(short = 'r', long, default_value_t = 10)]
    pub runs: u32,
    #[arg(short = 'w', long, default_value_t = 2)]
    pub warmup: u32,
    #[arg(short = 't', long, value_parser = humantime::parse_duration)]
    pub timeout: Option<Duration>,
    #[arg(short = 'f', long, default_value = "benchguard.json")]
    pub file: PathBuf,
}

#[derive(Debug, Args)]
pub struct RecordArgs {
    pub name: String,
    #[command(flatten)]
    pub run: RunOptions,
    #[arg(last = true, required = true)]
    pub target: Vec<String>,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    pub name: String,
    #[command(flatten)]
    pub run: RunOptions,
    #[arg(last = true)]
    pub target: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[arg(short = 'f', long, default_value = "benchguard.json")]
    pub file: PathBuf,
}
```

```rust
// src/main.rs
use clap::Parser;
use benchguard::cli::Cli;

fn main() {
    let _cli = Cli::parse();
}
```

- [ ] **Step 4: Run formatting and the CLI tests**

Run: `cargo fmt --check && cargo test --test cli_help`  
Expected: PASS; both help forms exit `0` without accessing a baseline.

- [ ] **Step 5: Commit the CLI contract**

```bash
git add Cargo.toml Cargo.lock src tests/cli_help.rs
git commit -m "feat: define benchguard CLI contract"
```

### Task 2: Domain model and deterministic statistics

**Story points:** 5

**Files:**

- Create: `src/domain.rs`
- Create: `src/stats.rs`
- Modify: `src/lib.rs`

**Interfaces:**

- Produces: `Sample { wall_ns: u64, cpu_ns: u64, peak_memory_bytes: u64, exit_code: i32 }`.
- Produces: `Aggregate { median, mean, standard_deviation, min, max, p50, p95, sample_count }`.
- Produces: `stats::aggregate(&[u64]) -> Result<Aggregate, BenchguardError>`.

- [ ] **Step 1: Write failing tests for sorting, percentiles, and empty samples**

```rust
// src/stats.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_unsorted_integer_samples() {
        let result = aggregate(&[30, 10, 20, 40]).unwrap();
        assert_eq!(result.median, 25);
        assert_eq!(result.mean, 25);
        assert_eq!(result.min, 10);
        assert_eq!(result.max, 40);
        assert_eq!(result.p50, 20);
        assert_eq!(result.p95, 40);
        assert_eq!(result.sample_count, 4);
    }

    #[test]
    fn rejects_empty_samples() {
        assert!(matches!(aggregate(&[]), Err(BenchguardError::EmptySamples)));
    }
}
```

- [ ] **Step 2: Verify the tests fail**

Run: `cargo test stats::tests`  
Expected: FAIL because `Aggregate`, `aggregate`, and `EmptySamples` are undefined.

- [ ] **Step 3: Implement the normalized model and nearest-rank percentiles**

```rust
// src/domain.rs
use serde::{Deserialize, Serialize};

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
```

Implement `aggregate` with checked `u128` sums, integer median, population standard deviation, and nearest-rank percentiles. Define p50 for `[10,20,30,40]` as `20`; the median remains the midpoint `25`.

- [ ] **Step 4: Run unit tests and Clippy**

Run: `cargo test stats::tests && cargo clippy --all-targets -- -D warnings`  
Expected: PASS with no float-to-integer truncation warning and no unchecked overflow.

- [ ] **Step 5: Commit the statistics core**

```bash
git add src/domain.rs src/stats.rs src/lib.rs src/error.rs
git commit -m "feat: add benchmark statistics model"
```

### Task 3: Direct command runner and wall-time measurements

**Story points:** 5

**Files:**

- Create: `src/runner/mod.rs`
- Create: `crates/benchguard-fixture/Cargo.toml`
- Create: `crates/benchguard-fixture/src/main.rs`
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`

**Interfaces:**

- Consumes: `domain::Sample`.
- Produces: `CommandSpec { program: OsString, args: Vec<OsString> }`.
- Produces: `RunConfig { warmups: u32, runs: u32, timeout: Option<Duration> }`.
- Produces: `runner::run(&CommandSpec, &RunConfig) -> Result<Vec<Sample>, BenchguardError>`.

- [ ] **Step 1: Write a failing runner test using the fixture binary**

```rust
// src/runner/mod.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_argument_boundaries_and_collects_requested_runs() {
        let spec = CommandSpec::new(fixture_path(), ["echo-args", "two words"]);
        let samples = run(&spec, &RunConfig {
            warmups: 1,
            runs: 3,
            timeout: None,
        }).unwrap();
        assert_eq!(samples.len(), 3);
        assert!(samples.iter().all(|sample| sample.wall_ns > 0));
        assert!(samples.iter().all(|sample| sample.exit_code == 0));
    }
}
```

- [ ] **Step 2: Verify the runner test fails**

Run: `cargo test runner::tests::preserves_argument_boundaries_and_collects_requested_runs`  
Expected: FAIL because the runner and fixture do not exist.

- [ ] **Step 3: Implement the fixture commands**

```rust
// crates/benchguard-fixture/src/main.rs
use std::{env, process, thread, time::Duration};

fn main() {
    match env::args().nth(1).as_deref() {
        Some("echo-args") => println!("{}", env::args().nth(2).unwrap()),
        Some("sleep-ms") => {
            let ms: u64 = env::args().nth(2).unwrap().parse().unwrap();
            thread::sleep(Duration::from_millis(ms));
        }
        Some("exit") => {
            let code: i32 = env::args().nth(2).unwrap().parse().unwrap();
            process::exit(code);
        }
        _ => process::exit(64),
    }
}
```

- [ ] **Step 4: Implement warm-ups and measured direct execution**

Use `std::process::Command::new(&spec.program).args(&spec.args)` for every invocation. Measure wall time with `Instant`; reject zero runs, non-zero exits, and launch failures. Set CPU and peak-memory fields to `0` only inside this Sprint 1 implementation; Task 7 and Task 8 replace those platform stubs before v0.1.

- [ ] **Step 5: Run the entire Sprint 1 gate**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`  
Expected: PASS on Windows and Linux.

- [ ] **Step 6: Commit the benchmark engine**

```bash
git add Cargo.toml Cargo.lock src/runner crates
git commit -m "feat: execute commands and measure wall time"
```

---

## Sprint 2 — Baselines and regression checks

### Task 4: Versioned baseline schema and validation

**Story points:** 5

**Files:**

- Create: `src/baseline/mod.rs`
- Create: `src/baseline/schema.rs`
- Modify: `src/domain.rs`
- Modify: `src/lib.rs`

**Interfaces:**

- Produces: `PlatformId { os: String, arch: String }`.
- Produces: `BaselineFileV1 { schema_version: u32, benchmarks: BTreeMap<String, BenchmarkV1> }`.
- Produces: `BenchmarkV1`, `MetricAggregateV1`, `BudgetsV1`, and `NoiseFloorsV1`.
- Produces: `BaselineFileV1::validate(&self) -> Result<(), BenchguardError>`.

- [ ] **Step 1: Write failing schema round-trip and rejection tests**

```rust
#[test]
fn v1_round_trip_uses_integer_base_units() {
    let baseline = example_baseline();
    let json = serde_json::to_string_pretty(&baseline).unwrap();
    assert!(json.contains("\"schema_version\": 1"));
    assert!(json.contains("\"wall_ns\""));
    assert!(json.contains("\"peak_memory_bytes\""));
    assert_eq!(serde_json::from_str::<BaselineFileV1>(&json).unwrap(), baseline);
}

#[test]
fn rejects_unsupported_schema_version() {
    let mut baseline = example_baseline();
    baseline.schema_version = 9;
    assert!(matches!(baseline.validate(), Err(BenchguardError::UnsupportedSchema(9))));
}
```

- [ ] **Step 2: Verify schema tests fail**

Run: `cargo test baseline::schema::tests`  
Expected: FAIL because the versioned schema is undefined.

- [ ] **Step 3: Implement schema v1 with explicit field names**

Define command storage as `program: String` plus `args: Vec<String>`, RFC 3339 UTC timestamp text, platform OS/architecture, BenchGuard version, run settings, three metric aggregates, optional percentage budgets, and absolute floors. Use `BTreeMap` so pretty JSON output is deterministic.

- [ ] **Step 4: Implement validation**

Reject:

- `schema_version != 1`;
- empty or whitespace-only benchmark names;
- `runs == 0`;
- missing program;
- aggregate `sample_count != runs`;
- relative budget values below `0.0`;
- zero absolute floors;
- empty OS or architecture identifiers.

Run: `cargo test baseline::schema::tests`  
Expected: PASS for round-trip and every rejection case.

- [ ] **Step 5: Commit the schema**

```bash
git add src/baseline src/domain.rs src/lib.rs
git commit -m "feat: define versioned baseline schema"
```

### Task 5: Atomic baseline store

**Story points:** 5

**Files:**

- Create: `src/baseline/store.rs`
- Modify: `src/baseline/mod.rs`
- Modify: `src/error.rs`

**Interfaces:**

- Consumes: `BaselineFileV1::validate`.
- Produces: `BaselineStore::load(path: &Path) -> Result<BaselineFileV1, BenchguardError>`.
- Produces: `BaselineStore::save_atomic(path: &Path, value: &BaselineFileV1) -> Result<(), BenchguardError>`.

- [ ] **Step 1: Write failing store tests**

```rust
#[test]
fn save_and_load_preserve_a_valid_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("benchguard.json");
    let expected = example_baseline();
    BaselineStore::save_atomic(&path, &expected).unwrap();
    assert_eq!(BaselineStore::load(&path).unwrap(), expected);
}

#[test]
fn invalid_value_preserves_existing_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("benchguard.json");
    std::fs::write(&path, b"previous-valid-content").unwrap();
    let mut invalid = example_baseline();
    invalid.schema_version = 9;
    let result = BaselineStore::save_atomic(&path, &invalid);
    assert!(result.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"previous-valid-content");
}

struct FailingAtomicReplace;

impl AtomicReplace for FailingAtomicReplace {
    fn replace(&self, _source: &Path, _destination: &Path) -> io::Result<()> {
        Err(io::Error::other("injected replacement failure"))
    }
}

#[test]
fn replacement_failure_preserves_existing_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("benchguard.json");
    std::fs::write(&path, b"previous-valid-content").unwrap();
    let result = BaselineStore::save_atomic_with(
        &path,
        &example_baseline(),
        &FailingAtomicReplace,
    );
    assert!(result.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"previous-valid-content");
}
```

- [ ] **Step 2: Verify store tests fail**

Run: `cargo test baseline::store::tests`  
Expected: FAIL because `BaselineStore` is undefined.

- [ ] **Step 3: Implement same-directory atomic replacement**

Validate and serialize before opening a temporary file. Use `tempfile::NamedTempFile::new_in(parent)`, write all bytes, call `flush` and `sync_all`, then persist over the destination. Put the final replacement behind a private `AtomicReplace` trait so the failure path is deterministic in tests. On Windows, the production implementation uses `ReplaceFileW` when the destination exists and `MoveFileExW` with `MOVEFILE_WRITE_THROUGH` for the first write; both operations leave an existing valid destination untouched until replacement succeeds.

- [ ] **Step 4: Run durability tests**

Run: `cargo test baseline::store::tests`  
Expected: PASS; failed serialization and failed persistence retain the original bytes.

- [ ] **Step 5: Commit the store**

```bash
git add src/baseline src/error.rs Cargo.toml Cargo.lock
git commit -m "feat: persist baselines atomically"
```

### Task 6: Budgets, reports, and public command orchestration

**Story points:** 8

**Files:**

- Create: `src/comparison.rs`
- Create: `src/app.rs`
- Create: `src/report/mod.rs`
- Create: `src/report/human.rs`
- Create: `src/report/json.rs`
- Modify: `src/cli.rs`
- Modify: `src/main.rs`
- Create: `tests/record_check.rs`
- Create: `tests/common/mod.rs`

**Interfaces:**

- Consumes: `runner::run`, `stats::aggregate`, and `BaselineStore`.
- Produces: `compare(current: u64, baseline: u64, relative_budget: Option<f64>, absolute_floor: u64) -> MetricOutcome`.
- Produces: `app::execute(cli: Cli) -> Result<ExitClass, BenchguardError>`.
- Produces: `ReportRenderer::render(&Report) -> String`.

- [ ] **Step 1: Write failing threshold boundary tests**

```rust
#[test]
fn regression_requires_relative_and_absolute_limits() {
    assert_eq!(
        compare(111, 100, Some(10.0), 5),
        MetricOutcome::Regression
    );
    assert_eq!(
        compare(104, 100, Some(1.0), 5),
        MetricOutcome::Pass
    );
    assert_eq!(
        compare(200, 100, None, 5),
        MetricOutcome::Unbudgeted
    );
}
```

- [ ] **Step 2: Write failing end-to-end record/check tests**

```rust
#[test]
fn record_then_check_passes_and_regression_exits_one() {
    let project = TestProject::new();
    project.record_sleep("startup", 10).success();
    project.check_sleep("startup", 10).success();
    project.check_sleep("startup", 80)
        .failure()
        .code(1)
        .stdout(predicate::str::contains("REGRESSION"));
}
```

- [ ] **Step 3: Verify both test groups fail**

Run: `cargo test comparison::tests --test record_check`  
Expected: FAIL because comparison and application orchestration are undefined.

- [ ] **Step 4: Implement comparison and reporting**

Calculate:

```rust
let absolute_delta = current.saturating_sub(baseline);
let relative_delta_pct = if baseline == 0 {
    if current == 0 { 0.0 } else { f64::INFINITY }
} else {
    (absolute_delta as f64 / baseline as f64) * 100.0
};
let regressed = relative_budget
    .is_some_and(|limit| relative_delta_pct > limit)
    && absolute_delta > absolute_floor;
```

Human output must show baseline, current median, delta, budget, floor, sample count, and `PASS`, `REGRESSION`, or `UNBUDGETED`. JSON output uses `{ "schema_version": 1, "status": ..., "benchmarks": [...], "errors": [] }`.

- [ ] **Step 5: Wire `record`, `check`, `list`, and exit codes**

Add these exact CLI value types:

```rust
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum OutputFormat { Human, Json }

#[derive(Debug, Clone, Copy)]
pub struct PercentBudget(pub f64);

impl FromStr for PercentBudget {
    type Err = String;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let value = input.trim().trim_start_matches('+')
            .strip_suffix('%').ok_or("budget must end in %")?
            .parse::<f64>().map_err(|_| "budget must be a number")?;
        (value >= 0.0 && value.is_finite())
            .then_some(Self(value))
            .ok_or_else(|| "budget must be finite and non-negative".into())
    }
}
```

Add `--max-time`, `--max-cpu`, `--max-memory`, and `--format` to `record` and `check`; add `--format` to `list`. Until the platform collectors land in Tasks 7 and 8, reject `--max-cpu` and `--max-memory` with exit code `2` instead of comparing placeholder zero values. `record` runs the command, aggregates the available metric vectors, and replaces only the named entry. `check` uses the stored command unless the user supplied `-- ...`. `list` never calls the runner. `main` maps `ExitClass::Success` to `0`, `Regression` to `1`, and every `Err` to `2`.

- [ ] **Step 6: Run the Sprint 2 gate**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`  
Expected: PASS; the regression integration test observes exit code `1`.

- [ ] **Step 7: Commit the working wall-time product**

```bash
git add src tests Cargo.toml Cargo.lock
git commit -m "feat: record and enforce performance baselines"
```

---

## Sprint 3 — System metrics and reliability

### Task 7: Linux process-group metrics, timeout, and cleanup

**Story points:** 8

**Files:**

- Create: `src/runner/linux.rs`
- Modify: `src/runner/mod.rs`
- Modify: `Cargo.toml`
- Extend: `crates/benchguard-fixture/src/main.rs`
- Create: `tests/failures.rs`

**Interfaces:**

- Produces on Linux: `platform_run_once(&CommandSpec, Option<Duration>) -> Result<Sample, BenchguardError>`.
- Guarantees: separate process group; aggregate sampled `/proc` CPU/RSS metrics; process-group termination on timeout.

- [ ] **Step 1: Extend the fixture and write Linux-only failing tests**

Add fixture commands:

```rust
Some("allocate-mib") => {
    let mib: usize = env::args().nth(2).unwrap().parse().unwrap();
    let mut bytes = vec![0_u8; mib * 1024 * 1024];
    bytes.iter_mut().step_by(4096).for_each(|byte| *byte = 1);
    thread::sleep(Duration::from_millis(100));
    std::hint::black_box(bytes);
}
Some("spawn-sleeper") => {
    let mut child = std::process::Command::new(env::current_exe().unwrap())
        .args(["sleep-ms", "30000"]).spawn().unwrap();
    child.wait().unwrap();
}
```

Test that a 32 MiB allocation reports at least 30 MiB and that a 50 ms timeout returns exit code `2` with no surviving process group.

- [ ] **Step 2: Verify Linux tests fail**

Run on Linux: `cargo test --test failures linux_ -- --nocapture`  
Expected: FAIL because Linux collection and process-group cleanup are absent.

- [ ] **Step 3: Implement the Linux lifecycle**

Before `exec`, call `setsid()` through `std::os::unix::process::CommandExt::pre_exec`. Poll members observed in the target session/process group from `/proc/<pid>/stat` every 5 ms. Sum current RSS across that managed scope and retain the maximum sum. Convert observed user/system ticks using `_SC_CLK_TCK` to nanoseconds without double-counting repeated samples. On timeout, send `SIGTERM` to `-pgid`, keep the leader unreaped so the numeric PGID cannot be reused, wait 100 ms, send `SIGKILL` if required, and then reap the leader.

Document in `linux.rs` that Linux CPU and peak memory are sampled process-group aggregates; the 5 ms interval is fixed v0.1 behavior. Very short-lived processes between samples and descendants that deliberately leave the group may be missed. These limitations are part of the approved v0.1 contract.

- [ ] **Step 4: Run Linux reliability tests repeatedly**

Run: `cargo test --test failures linux_ -- --nocapture` five consecutive times.  
Expected: all five runs PASS; no fixture sleeper remains after each timeout.

- [ ] **Step 5: Commit Linux metrics**

```bash
git add Cargo.toml Cargo.lock src/runner crates/benchguard-fixture tests/failures.rs
git commit -m "feat(linux): collect process-group metrics"
```

### Task 8: Windows Job Object metrics, timeout, and cleanup

**Story points:** 8

**Files:**

- Create: `src/runner/windows.rs`
- Modify: `src/runner/mod.rs`
- Modify: `Cargo.toml`
- Modify: `tests/failures.rs`

**Interfaces:**

- Produces on Windows: `platform_run_once(&CommandSpec, Option<Duration>) -> Result<Sample, BenchguardError>`.
- Guarantees: child assigned to a Job Object before normal execution; job-wide CPU and peak memory; job termination on timeout.

- [ ] **Step 1: Write Windows-only failing tests**

Use the same fixture behaviors as Task 7. Assert that 32 MiB allocation reports at least 30 MiB, a normal child tree exits cleanly, and a timed-out tree returns exit code `2` without leaving the sleeper alive.

- [ ] **Step 2: Verify Windows tests fail**

Run on Windows: `cargo test --test failures windows_ -- --nocapture`  
Expected: FAIL because Windows Job Object collection is absent.

- [ ] **Step 3: Implement the Windows lifecycle**

Use `windows-sys` APIs:

- `CreateJobObjectW`;
- `SetInformationJobObject` with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`;
- create the child through `CreateProcessW` with `CREATE_SUSPENDED`;
- `AssignProcessToJobObject`;
- `ResumeThread`;
- `WaitForSingleObject`;
- `QueryInformationJobObject` for total user/kernel time and peak job memory;
- `TerminateJobObject` on timeout.

Wrap every raw `HANDLE` in an internal RAII type whose `Drop` calls `CloseHandle`. Convert 100 ns Windows time units to nanoseconds with checked multiplication.

- [ ] **Step 4: Run Windows reliability tests repeatedly**

Run: `cargo test --test failures windows_ -- --nocapture` five consecutive times.  
Expected: all five runs PASS and Task Manager/process inspection shows no surviving fixture.

- [ ] **Step 5: Commit Windows metrics**

```bash
git add Cargo.toml Cargo.lock src/runner/windows.rs src/runner/mod.rs tests/failures.rs
git commit -m "feat(windows): collect process-tree metrics"
```

### Task 9: Variability warnings and complete error/report contract

**Story points:** 5

**Files:**

- Modify: `src/stats.rs`
- Modify: `src/app.rs`
- Modify: `src/report/mod.rs`
- Modify: `src/report/human.rs`
- Modify: `src/report/json.rs`
- Modify: `tests/failures.rs`
- Modify: `tests/record_check.rs`

**Interfaces:**

- Produces: `coefficient_of_variation_pct(&[u64]) -> Result<f64, BenchguardError>`.
- Produces: warning when wall-time coefficient of variation is greater than `10.0`.
- Produces: stable JSON error envelope with exit code `2`.

- [ ] **Step 1: Write failing warning and error-envelope tests**

```rust
#[test]
fn warns_above_ten_percent_variability() {
    assert!(coefficient_of_variation_pct(&[10, 10, 20]).unwrap() > 10.0);
}

#[test]
fn malformed_baseline_returns_structured_json_error() {
    let project = TestProject::with_baseline("{broken");
    project.command()
        .args(["list", "--format", "json"])
        .assert()
        .failure()
        .code(2)
        .stdout(predicate::str::contains("\"status\":\"error\""))
        .stdout(predicate::str::contains("\"code\":\"invalid_baseline\""));
}
```

- [ ] **Step 2: Verify tests fail**

Run: `cargo test warns_above malformed_baseline`  
Expected: FAIL because the warning and stable error mapping are absent.

- [ ] **Step 3: Implement warning and error codes**

Use population standard deviation divided by the arithmetic mean. A zero mean yields `0.0` when every sample is zero. Add stable error codes: `invalid_arguments`, `command_failed`, `timeout`, `invalid_baseline`, `unsupported_schema`, `incompatible_platform`, `measurement_failed`, and `internal`.

- [ ] **Step 4: Complete the cross-platform acceptance matrix**

Run on Windows and Linux:

```text
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -- record smoke --max-time +10% -- ./target/debug/benchguard-fixture sleep-ms 20
cargo run -- check smoke -- ./target/debug/benchguard-fixture sleep-ms 20
cargo run -- check smoke -- ./target/debug/benchguard-fixture sleep-ms 100
```

On Windows, use `.\target\debug\benchguard-fixture.exe` in the same three commands. Expected: tests PASS; the first check exits `0`; the slower check exits `1`; malformed files and timeouts exit `2`.

- [ ] **Step 5: Commit the reliability contract**

```bash
git add src tests
git commit -m "feat: finalize warnings and error reports"
```

---

## Sprint 4 — Open-source release

### Task 10: npm native-package launcher

**Story points:** 5

**Files:**

- Create: `npm/cli/package.json`
- Create: `npm/cli/bin/benchguard.js`
- Create: `npm/linux-x64/package.json`
- Create: `npm/win32-x64/package.json`
- Create: `npm/test-launcher.mjs`

**Interfaces:**

- Produces: `npm install --global @benchguard/cli`.
- Produces: `npx @benchguard/cli`.
- Delegates arguments and exit status unchanged to the native binary.

- [ ] **Step 1: Write a failing launcher smoke test**

```javascript
// npm/test-launcher.mjs
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";

const result = spawnSync(process.execPath, ["cli/bin/benchguard.js", "--version"], {
  cwd: new URL(".", import.meta.url),
  encoding: "utf8",
});
assert.equal(result.status, 0);
assert.match(result.stdout, /^benchguard 0\.1\.0/);
```

- [ ] **Step 2: Verify the launcher test fails**

Run: `node npm/test-launcher.mjs`  
Expected: FAIL because the launcher and native packages do not exist.

- [ ] **Step 3: Define platform optional dependencies**

```json
{
  "name": "@benchguard/cli",
  "version": "0.1.0",
  "bin": { "benchguard": "bin/benchguard.js" },
  "optionalDependencies": {
    "@benchguard/linux-x64": "0.1.0",
    "@benchguard/win32-x64": "0.1.0"
  },
  "engines": { "node": ">=18" }
}
```

Each native package declares exact `os` and `cpu` arrays and contains only its executable, README, license, and package metadata.

- [ ] **Step 4: Implement the launcher**

Map `process.platform/process.arch` to the matching scoped package, resolve its executable with `createRequire`, spawn synchronously with `stdio: "inherit"`, and exit with the child's exact status. Unsupported platforms print supported targets and direct-download instructions, then exit `2`.

- [ ] **Step 5: Test packed npm artifacts**

Run on both platforms:

```text
npm pack ./npm/linux-x64
npm pack ./npm/cli
npm install --global ./benchguard-linux-x64-0.1.0.tgz ./benchguard-cli-0.1.0.tgz
benchguard --version
npx --yes ./benchguard-cli-0.1.0.tgz help
```

On Windows, replace `linux-x64` with `win32-x64` in the pack command and native tarball filename. Expected: both invocations print v0.1 help/version and exit `0`.

- [ ] **Step 6: Commit npm packaging**

```bash
git add npm
git commit -m "feat: add npm native binary launcher"
```

### Task 11: CI, release automation, and performance benchmarks

**Story points:** 8

**Files:**

- Create: `.github/workflows/ci.yml`
- Create: `.github/workflows/release.yml`
- Create: `benches/core.rs`
- Modify: `Cargo.toml`

**Interfaces:**

- Produces: Windows/Linux format, lint, test, and build gates.
- Produces: version-tag release artifacts, SHA-256 checksums, Cargo publish, and npm publish.

- [ ] **Step 1: Add Criterion benchmarks**

Benchmark aggregation for 10, 100, and 10,000 samples plus serialization of 1 and 100 benchmark entries. Set `harness = false` for `benches/core.rs`.

- [ ] **Step 2: Run the benchmark smoke gate**

Run: `cargo bench --bench core -- --test`  
Expected: PASS; every benchmark function executes once without measuring a target command.

- [ ] **Step 3: Add the CI matrix**

`ci.yml` runs on pull requests and pushes with:

- `ubuntu-latest` and `windows-latest`;
- stable Rust with the project MSRV checked in a separate compile job;
- `cargo fmt --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace`;
- packed npm launcher smoke tests using Node 18 and current LTS.

- [ ] **Step 4: Add tag-driven release automation**

For tags matching `v*`:

1. verify Cargo and npm versions equal the tag;
2. run the full CI matrix;
3. build stripped x86-64 Windows and Linux binaries;
4. archive binaries with license and README;
5. generate SHA-256 checksums;
6. create a GitHub Release;
7. publish native npm packages before `@benchguard/cli`;
8. publish the Cargo crate.

Publishing jobs use GitHub environments requiring manual approval. A rerun checks whether each version already exists and skips it safely.

- [ ] **Step 5: Validate workflows locally where possible**

Run:

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench --bench core -- --test
```

Validate both YAML files with a workflow linter. Expected: all commands PASS and the workflow parser reports no schema errors.

- [ ] **Step 6: Commit automation**

```bash
git add .github Cargo.toml Cargo.lock benches
git commit -m "ci: test and publish benchguard releases"
```

### Task 12: Documentation, licensing, and v0.1 release readiness

**Story points:** 3

**Files:**

- Create: `README.md`
- Create: `CHANGELOG.md`
- Create: `CONTRIBUTING.md`
- Create: `CODE_OF_CONDUCT.md`
- Create: `LICENSE-MIT`
- Create: `LICENSE-APACHE`
- Modify: `Cargo.toml`
- Modify: npm package metadata

**Interfaces:**

- Produces: a quick start that a new user can execute without hidden setup.
- Produces: contribution and release policies suitable for a public repository.

- [ ] **Step 1: Write the README acceptance script first**

Create a temporary test project and run every quick-start command exactly as documented:

```text
benchguard record startup --runs 10 --max-time +10% -- my-app --version
benchguard check startup
benchguard list
benchguard help record
```

The script must fail if any command, output assertion, or exit code differs from the README.

- [ ] **Step 2: Write user documentation**

README sections:

- what problem BenchGuard solves;
- npm, `npx`, Cargo, and binary installation;
- five-minute record/check quick start;
- CI example that commits `benchguard.json`;
- direct-execution and explicit-shell examples;
- metric definitions and 5 ms Linux sampling disclosure;
- noise, median, budgets, and absolute floors;
- exit-code table;
- supported platforms;
- link to the JSON schema and contribution guide.

- [ ] **Step 3: Write project governance files**

Use MIT OR Apache-2.0 dual licensing. Document:

- `cargo fmt`, Clippy, test, and benchmark commands;
- Conventional Commit examples;
- how to add a platform backend;
- vulnerability reporting through private GitHub security advisories;
- v0.1 changelog entries grouped under Added, Changed, Fixed, and Security.

- [ ] **Step 4: Run the final Definition of Done gate**

On clean Windows and Linux environments:

```text
cargo install --path .
benchguard help
benchguard help record
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
node npm/test-launcher.mjs
```

Then execute the README acceptance script and verify:

- normal checks exit `0`;
- an intentional regression exits `1`;
- a timeout exits `2` and leaves no child process;
- a failed record leaves the baseline hash unchanged;
- npm and Cargo commands invoke the same BenchGuard version.

- [ ] **Step 5: Commit release documentation**

```bash
git add README.md CHANGELOG.md CONTRIBUTING.md CODE_OF_CONDUCT.md LICENSE-* Cargo.toml npm
git commit -m "docs: prepare benchguard v0.1 release"
```

## Sprint ceremonies and tracking

### Sprint planning

At the beginning of each week:

1. Confirm the sprint goal and task acceptance criteria.
2. Re-estimate only if new evidence changes platform complexity.
3. Pull tasks in numerical order because their interfaces are dependencies.
4. Reserve 20% of the week for cross-platform failures and review.

### Daily Scrum

Keep the update to:

- the last verified test or deliverable;
- the next failing test being addressed;
- a concrete blocker, including platform and error output.

### Sprint review

Demonstrate the increment from the delivery map on both Windows and Linux. A slide deck or progress description does not substitute for the executable demonstration.

### Retrospective

Record one keep, one problem, and one experiment for the next sprint. Convert only actionable product or engineering work into backlog items.

## Release acceptance checklist

- [ ] The exact Git tag, Cargo crate, npm launcher, npm native packages, and binary `--version` values match.
- [ ] Windows and Linux CI pass formatting, Clippy, tests, npm packing, and release builds.
- [ ] The committed sample baseline validates against schema v1.
- [ ] Help documents all commands, defaults, and short aliases.
- [ ] Direct command argument boundaries are covered end to end.
- [ ] CPU time and peak memory follow the documented Linux process-group and Windows Job Object scopes.
- [ ] Timeouts terminate process trees and return exit code `2`.
- [ ] Regression boundaries test equality and just-over-budget behavior.
- [ ] A failed record preserves the previous baseline bytes.
- [ ] npm, `npx`, Cargo, and downloaded binaries complete the quick start.
- [ ] README metric limitations and Linux sampling behavior match the implementation.
