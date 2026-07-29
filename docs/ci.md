# Using BenchGuard in CI

Commit `benchguard.json` beside the code it measures. Record and check on the
same operating system, CPU architecture, runner class, build profile, and
roughly equivalent machine load. BenchGuard rejects OS or architecture
mismatches, but it cannot detect changes in runner hardware.

## GitHub Actions example

The following job assumes `startup` is already recorded in the committed
baseline:

```yaml
name: Performance

on:
  pull_request:
  push:
    branches: [main]

jobs:
  benchguard:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Build the target
        run: cargo build --release --locked
      - name: Install BenchGuard
        run: cargo install benchguard --locked
      - name: Check committed budgets
        run: benchguard check startup
```

Pin third-party actions to immutable commit SHAs in security-sensitive
repositories. The short tags above keep the example readable, not prescriptive.

## Record and update a baseline

Create the baseline in a trusted environment that matches CI:

```console
benchguard record startup -r 20 -w 3 -t 10s --max-time +10% -- ./target/release/my-app --version
git add benchguard.json
git commit -m "perf: update startup baseline"
```

Review baseline changes like source changes. Confirm that the command,
platform, sample counts, budgets, and measured values changed for an understood
reason. Do not update a baseline merely to make an unexplained regression pass.

CLI options on `check` override stored settings for that run without rewriting
the file:

```console
benchguard check startup -r 20 --max-time +5%
```

## Interpreting results

- Exit `0`: configured checks passed.
- Exit `1`: a configured budget was exceeded; treat this as a performance test
  failure.
- Exit `2`: the benchmark could not be evaluated; fix the command, baseline,
  timeout, platform, or configuration rather than treating it as a regression.

BenchGuard compares medians. A metric must exceed both its percentage budget
and its absolute floor to regress. The default floors are 1 ms wall time, 1 ms
CPU time, and 1 MiB peak memory. Wall-time coefficient of variation above 10%
is a non-failing warning and often signals a noisy runner.

Linux v0.1 samples the target session/process group every 5 ms. Windows
samples aggregate resident working-set bytes from active Job Object members
every 5 ms while retaining Job accounting for CPU. Very short processes
between samples may not be included; Linux descendants that leave the group
may also be missed. Keep the baseline and check on the same platform and
understand that these scopes are not identical.

For machine processing, add `--format json` and consume the stable envelope
documented in [json-format.md](json-format.md).
