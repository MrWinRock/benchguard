# BenchGuard

BenchGuard is a native Rust command-line tool that records the performance of
any executable command and fails when a later run exceeds a committed budget.
It gives application startup, build steps, code generators, and other
developer workflows one portable `benchguard.json` baseline instead of
requiring a language-specific benchmark harness.

BenchGuard v0.1 supports x86-64 Windows and Linux. It measures wall time,
managed-scope CPU time, and peak memory, produces human or JSON reports, and
uses stable exit codes for CI.

> Release status: this repository is prepared for v0.1 publication. Package
> and release download commands become available after maintainers complete
> the [release checklist](docs/releasing.md); this document does not claim that
> those external artifacts already exist.

## Install

Choose one installation method after v0.1 has been published:

```console
npm install --global @benchguard/cli
benchguard --version
```

Run without a global install:

```console
npx @benchguard/cli help
```

Install the Rust crate:

```console
cargo install benchguard --locked
```

To install the current checkout before publication:

```console
cargo install --path . --locked
```

Prebuilt archives for `x86_64-unknown-linux-gnu` and
`x86_64-pc-windows-msvc`, plus `SHA256SUMS`, are attached to each GitHub
Release. Verify the checksum, extract the archive, and place `benchguard` (or
`benchguard.exe`) on `PATH`.

The npm package is only an installer and launcher. All measurements run in the
same native Rust binary used by Cargo and release archives. npm requires
Node.js 18 or newer; the standalone binary does not require Node.js.

## Five-minute quick start

From your project directory, choose a stable command whose performance matters.
Here `my-app --version` is only an example:

```console
benchguard record startup --runs 10 --max-time +10% -- my-app --version
benchguard check startup
benchguard list
benchguard help record
```

`record` runs two unmeasured warm-ups and ten measured runs by default, then
writes `benchguard.json`. Commit that file with the code it describes.
`check` reuses the stored command, settings, and budgets unless you override
them for that invocation. `list` reads the file without executing the target.

Common options have short aliases:

- `-r, --runs <COUNT>`: measured runs; record default `10`.
- `-w, --warmup <COUNT>`: unmeasured runs; record default `2`.
- `-t, --timeout <DURATION>`: per-run timeout, such as `500ms` or `2s`.
- `-f, --file <PATH>`: baseline file; default `benchguard.json`.

Budgets use descriptive long options: `--max-time`, `--max-cpu`, and
`--max-memory`. Values are non-negative percentages such as `+10%`. Add
`--format json` to `record`, `check`, or `list` for machine-readable output.
Use `benchguard help`, `benchguard help record`, or `<command> --help` for the
complete current interface.

## Use in CI

Record a baseline on the same operating system and CPU architecture used by
CI, commit `benchguard.json`, and run:

```yaml
- name: Install BenchGuard
  run: cargo install benchguard --locked
- name: Enforce performance budgets
  run: benchguard check startup
```

Do not compare a Windows baseline with Linux or one CPU architecture with
another; BenchGuard rejects that comparison with exit code `2`. Keep runner
hardware and background load as stable as practical. See [CI guidance](docs/ci.md)
for a complete workflow and baseline-update policy.

## Commands are executed directly

Everything after `--` is preserved as an executable plus an exact argument
array. BenchGuard does not invoke a shell:

```console
benchguard record arguments -- my-app "two words" ""
```

Pipes, redirects, wildcard expansion, and environment expansion require an
explicit shell:

```console
benchguard record pipeline -- bash -c 'producer | consumer'
benchguard record pipeline -- powershell -Command "producer | consumer"
```

Only use explicit shell execution with commands you trust; the chosen shell,
not BenchGuard, interprets that string.

During warm-up and measured runs, the target's standard input, output, and
error are connected to the operating system null device. BenchGuard does not
capture or replay target output in v0.1. This keeps human and JSON reports
deterministic, including when the target fails or times out.

## How checks are decided

For every successful measured run BenchGuard records:

- wall-clock duration in nanoseconds;
- CPU time in nanoseconds for the platform-managed execution scope;
- peak resident/working-set memory in bytes for that scope; and
- the target exit status.

The baseline stores minimum, maximum, mean, population standard deviation,
p50, p95, sample count, and median. The median is the comparison value because
it is less sensitive to isolated noise than the mean.

A metric regresses only when its increase is strictly greater than both the
configured relative budget and its absolute noise floor. The default floors
are 1 ms for wall time, 1 ms for CPU time, and 1 MiB for peak memory. A metric
without a budget is reported as `unbudgeted` and cannot fail a check.
Wall-time coefficient of variation above 10% produces a warning but does not
change the exit code.

On Windows, BenchGuard uses a Job Object for CPU and cleanup and samples the
aggregate current working set of active Job members every 5 ms. On Linux v0.1
it samples processes observed in the target session/process group every 5 ms
and cleans up that process group. A process that starts and exits between
samples may be missed on either platform; a Linux descendant that deliberately
leaves the group may also be missed. Memory values therefore describe the
sampled managed scope, not commit charge or exhaustive descendant accounting.

## Exit codes

| Code | Meaning |
|---:|---|
| `0` | All configured budgets passed, or the command only reported unbudgeted metrics. |
| `1` | At least one configured performance budget regressed. |
| `2` | Configuration, baseline, launch, target exit, timeout, measurement, platform, or internal error. |

Any failed warm-up or measured run aborts the operation. A failed `record`
leaves an existing baseline byte-for-byte unchanged. Timeouts terminate the
platform-managed process scope before returning.

## Compatibility and format

`benchguard.json` schema v1 stores integer nanoseconds and bytes, the exact
command argument array, settings, budgets, platform, version, and timestamp.
Comparisons require the same operating system and CPU architecture. See the
[baseline and report JSON reference](docs/json-format.md).

macOS, ARM, Git-aware baseline selection, confidence tests, a TUI, hosted
history, and plugins are not part of v0.1.

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) for local checks, commit conventions,
platform backend guidance, and the private vulnerability-reporting process.
Community participation follows the [Code of Conduct](CODE_OF_CONDUCT.md).

BenchGuard is dual-licensed under [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), at your option.
