# BenchGuard Design Specification

Date: 2026-07-28  
Status: Approved design, revised after written-spec review

## 1. Product summary

BenchGuard is an open-source Rust command-line tool that benchmarks any executable command and detects performance regressions against a repository-committed JSON baseline. It targets developers using Windows or Linux and ships as a single, easily installed binary.

The one-month MVP prioritizes trustworthy measurements, predictable automation, and a small cross-platform surface. It does not include a TUI, hosted service, plugin system, Git-managed baselines, or macOS support.

## 2. Goals

- Benchmark arbitrary executables without requiring language-specific integration.
- Record portable, versioned baselines in `benchguard.json`.
- Detect time, CPU, and peak-memory regressions using configurable budgets.
- Produce readable terminal output and machine-readable JSON.
- Provide stable exit codes suitable for any CI system.
- Install through npm, `cargo install`, or prebuilt Windows/Linux binaries.
- Demonstrate production-quality Rust, cross-platform systems programming, testing, and performance engineering.

## 3. Non-goals

- Microbenchmarking individual functions within an application.
- Replacing language-specific frameworks such as Criterion.
- Executing command strings through an implicit shell.
- Comparing benchmarks across operating systems or CPU architectures.
- Automatically committing, fetching, or selecting Git baselines.
- Providing a graphical or terminal user interface beyond normal CLI output.

## 4. User workflow

### Record a baseline

```console
benchguard record startup --runs 10 -- ./my-app --version
```

This performs warm-up runs, executes the measured runs, aggregates the results, and writes the benchmark definition and baseline to `benchguard.json`.

### Check for a regression

```console
benchguard check startup --runs 10 --max-time +10% -- ./my-app --version
```

This measures the command, compares the result with the named baseline, prints a report, and returns an automation-friendly exit code.

### List benchmarks

```console
benchguard list
```

This lists the benchmark names, commands, platforms, baseline dates, and configured budgets.

### Get help

```console
benchguard help
benchguard help record
benchguard record --help
```

`benchguard help` and `benchguard --help` print top-level usage, commands, and global options. `benchguard help <command>` and `benchguard <command> --help` print command-specific arguments, defaults, aliases, and examples. Help requests exit successfully without reading a baseline or executing a target command.

### Shell features

BenchGuard launches executables directly and preserves their argument boundaries. Pipes, redirects, variable expansion, and other shell syntax require an explicit shell:

```console
benchguard record pipeline -- bash -c 'producer | consumer'
benchguard record pipeline -- powershell -Command "producer | consumer"
```

During warm-up and measured runs, the target's standard input, standard
output, and standard error are connected to the operating system null device.
BenchGuard does not capture or replay target output in v0.1. This policy is
identical on Windows and Linux and guarantees that JSON output remains one
parseable report document.

## 5. Commands and options

### `benchguard record <name> [options] -- <program> [args...]`

Creates or replaces a successful baseline.

Initial options:

- `-r, --runs <count>`: measured executions; default `10`.
- `-w, --warmup <count>`: unmeasured executions; default `2`.
- `-t, --timeout <duration>`: timeout for each execution.
- `--max-time <budget>`: allowed wall-time regression.
- `--max-cpu <budget>`: allowed CPU-time regression.
- `--max-memory <budget>`: allowed peak-memory regression.
- `--format human|json`: output format; default `human`.
- `-f, --file <path>`: baseline file; default `benchguard.json`.
- `-h, --help`: print context-sensitive help.
- `-V, --version`: print the BenchGuard version.

Short aliases are limited to frequent, unambiguous options. Performance budgets retain descriptive long names to reduce mistakes in CI configuration.

`record` replaces a baseline only after every required validation passes and all requested measured runs succeed. A failed recording leaves the previous file unchanged.

### `benchguard check <name> [options] [-- <program> [args...]]`

Loads the named baseline and performs a new measurement. The command stored in the baseline is used when no command is supplied. Explicit CLI options override stored run settings and budgets for that invocation without modifying the file.

### `benchguard list [options]`

Reads the baseline file and prints its benchmark entries. It does not execute commands.

## 6. Metrics and comparison policy

Each successful run records:

- Wall-clock duration.
- Managed-scope CPU time.
- Process-tree peak resident memory.
- Exit code.

The aggregate contains:

- Median.
- Arithmetic mean.
- Standard deviation.
- Minimum and maximum.
- Selected percentiles, initially p50 and p95.
- Successful run count.

The median is the primary comparison statistic because it is less sensitive to isolated system noise than the mean.

A metric is considered regressed only when its increase exceeds:

1. its configured relative budget; and
2. a metric-specific absolute noise floor.

Both conditions must be true. The initial default absolute floors are:

- Wall time: 1 millisecond.
- CPU time: 1 millisecond.
- Peak memory: 1 mebibyte.

Relative budgets are explicit configuration; BenchGuard does not silently invent a passing performance budget. If a metric has no configured budget, it is reported but cannot fail the check.

BenchGuard warns when the coefficient of variation for wall time exceeds 10%. This warning does not change the exit code in the MVP.

## 7. Exit codes

- `0`: all configured checks passed.
- `1`: at least one configured performance budget was exceeded.
- `2`: configuration, baseline, command execution, timeout, measurement, or internal error.

These meanings are stable public behavior and are covered by end-to-end tests.

## 8. Architecture

The codebase is divided into focused components:

### `cli`

Parses arguments, converts them into application requests, selects the output format, and maps outcomes to documented exit codes. It contains no measurement logic.

### `runner`

Executes warm-up and measured runs, enforces per-run timeouts, captures exit status, and coordinates platform-managed cleanup. It accepts an executable and an argument vector rather than a reconstructed shell command.

### `metrics`

Defines a shared measurement interface and platform-specific backends:

- Linux backend for sampled session/process-group CPU and peak resident memory.
- Windows backend for Job Object CPU and sampled aggregate peak working-set memory.

Platform-specific units are normalized before values leave this component.

For Linux v0.1, CPU and memory are sampled every 5 ms for processes observed in the target session/process group. Cleanup targets that process group. Very short-lived processes between samples and descendants that deliberately leave the group may be missed; the CLI and documentation must disclose this limitation rather than present Linux values as exhaustive process-tree accounting.

For Windows v0.1, CPU time comes from cumulative Job Object accounting. Peak
memory is the greatest aggregate current working set observed by sampling
active Job Object member processes every 5 ms. Processes that start and exit
between samples, or exit between enumeration and their working-set query, may
be missed. The value is resident working-set bytes, not Job commit charge.

### `statistics`

Validates samples and calculates aggregates, variability, absolute differences, relative differences, and threshold outcomes. It is independent of process execution and serialization.

### `baseline`

Owns the versioned JSON schema, validation, loading, and atomic updates. It rejects incompatible platform identifiers and malformed or unsupported schema versions.

### `report`

Transforms measurement and comparison results into human-readable terminal output or stable JSON output. Output rendering is separated from the benchmark logic.

## 9. Data flow

```text
CLI request
  -> load and validate benchmark definition
  -> execute warm-up runs
  -> execute measured runs and collect metrics
  -> calculate aggregate statistics
  -> compare aggregates with configured budgets
  -> render the report
  -> return the documented exit code
```

For `record`, the validated aggregate is serialized to a temporary file in the destination directory and atomically replaces the prior baseline. For `check`, the baseline file is read-only.

## 10. Baseline format

`benchguard.json` contains:

- A top-level schema version.
- A map of benchmark names to definitions and recorded aggregates.
- The executable and exact argument array.
- Warm-up count, measured-run count, and optional timeout.
- Configured relative budgets and absolute noise floors.
- Recorded aggregate statistics for each metric.
- Operating system and CPU architecture.
- BenchGuard version and baseline timestamp.

Durations and memory values use explicit integer base units in JSON—nanoseconds and bytes—to avoid floating-point ambiguity. Human-readable units are presentation only.

Benchmark names must be unique, non-empty strings. Unsupported schema versions produce exit code `2`; the MVP does not perform automatic migrations.

## 11. Error handling and reliability

- Any failed warm-up or measured run aborts the benchmark; partial sample sets are never compared or recorded.
- A non-zero target exit status is an execution error unless future configuration explicitly permits it.
- A timeout terminates the target process tree and produces exit code `2`.
- Failed or partial measurements never replace an existing baseline.
- Baseline updates use a temporary file followed by an atomic replacement in the same directory.
- Commands are stored and launched as an executable plus argument array.
- Results are labeled by operating system and CPU architecture.
- A check refuses comparison when the current platform differs from the baseline platform.
- Human errors are concise and actionable; JSON errors use a stable structured envelope.

## 12. Testing strategy

### Unit tests

- Aggregate-statistic calculations.
- Percentage and absolute-threshold comparisons.
- Noise-floor behavior.
- Variability warnings.
- JSON validation and schema-version rejection.
- Exit-code mapping.

### Integration tests

Small helper executables provide controlled behaviors:

- Predictable sleep duration.
- Predictable memory allocation.
- Non-zero exit status.
- Child-process creation.
- Timeout and cleanup.

Integration tests cover `record`, `check`, `list`, top-level and command-specific help, short-option aliases, human output, JSON output, and preservation of argument boundaries.

### Failure and durability tests

- Missing executable.
- Malformed baseline JSON.
- Unsupported schema version.
- Incompatible platform.
- Interrupted or failed baseline update.
- Timeout with a process tree.
- Previously valid baseline remains intact after recording failure.

### Continuous integration

GitHub Actions runs formatting, linting, tests, and release builds on current stable Rust for Windows and Linux. BenchGuard's statistics and serialization hot paths receive Criterion benchmarks to detect accidental overhead within the project itself.

## 13. Distribution

The MVP supports:

- `npm install --global @benchguard/cli`.
- `npx @benchguard/cli`.
- `cargo install benchguard`.
- Prebuilt x86-64 Windows and Linux binaries on GitHub Releases.
- SHA-256 checksums for release artifacts.

The npm launcher is a small JavaScript shim that selects a platform-specific optional dependency containing the native Rust binary. The Windows and Linux binary packages are published with the same version as the Rust release. Installation fails with an actionable message on an unsupported operating system or architecture. Node.js is required only for the npm installation and launcher; the benchmark engine remains the native Rust executable.

Homebrew, Scoop, ARM binaries, and macOS support are post-MVP work unless the core roadmap finishes early.

## 14. One-month Scrum roadmap

The project uses four one-week sprints. Each sprint ends with a demonstrable, usable increment.

### Sprint 1: Benchmark engine

Sprint goal: benchmark an arbitrary executable reliably on Windows and Linux.

- Establish the Rust workspace and CLI skeleton.
- Preserve executable argument boundaries.
- Implement warm-up and measured runs.
- Measure wall-clock duration.
- Calculate core statistics.
- Add unit tests and Windows/Linux CI.

Increment: `benchguard run`-equivalent internal capability can execute and summarize a command, even though the public baseline workflow is not complete.

### Sprint 2: Baselines and regression checks

Sprint goal: record and enforce repository-committed performance budgets.

- Implement the versioned `benchguard.json` schema.
- Implement `record`, `check`, and `list`.
- Add percentage budgets and absolute noise floors.
- Add stable exit codes.
- Add human-readable and JSON reports.
- Add atomic baseline replacement.

Increment: a CI job can detect wall-time regressions using a committed baseline.

### Sprint 3: System metrics and reliability

Sprint goal: make results useful and failure behavior dependable.

- Add Windows and Linux CPU-time collection.
- Add Windows and Linux peak-memory collection.
- Add per-run timeouts and platform-managed cleanup.
- Add variability warnings.
- Add platform compatibility checks.
- Complete integration, failure, and durability tests.

Increment: BenchGuard enforces time, CPU, and memory budgets with tested cleanup behavior.

### Sprint 4: Open-source release

Sprint goal: publish an approachable, reproducible v0.1 release.

- Write installation, quick-start, CI, and JSON-format documentation.
- Add a sample GitHub Actions workflow.
- Add release automation for Windows and Linux binaries.
- Publish the npm launcher and platform-specific native packages.
- Publish checksums, changelog, contribution guide, and code of conduct.
- Validate npm, `npx`, and `cargo install` workflows.
- Record a short terminal demonstration.

Increment: users can discover, install, evaluate, and contribute to BenchGuard.

## 15. Definition of done for v0.1

- `record`, `check`, `list`, and all help forms behave as documented.
- Common flags support the documented `-r`, `-w`, `-t`, and `-f` aliases.
- Wall time and documented managed-scope CPU/peak-memory metrics work on Windows and Linux.
- Baselines are committed as versioned, validated JSON.
- Regression checks honor relative budgets and absolute floors.
- Timeouts clean up child process trees.
- Exit codes are stable and tested end to end.
- Formatting, linting, and all tests pass on Windows and Linux CI.
- Prebuilt Windows/Linux binaries and checksums are published.
- npm and `npx` launch the matching native binary on supported Windows and Linux systems.
- A new user can complete the quick start without undocumented setup.

## 16. Deferred work

- macOS and ARM support.
- Git-aware baseline selection.
- Confidence-interval or hypothesis-test comparison modes.
- Language-specific adapters.
- Historical result storage and trend charts.
- TUI or hosted dashboard.
- Homebrew and Scoop manifests.
- Plugin API.
