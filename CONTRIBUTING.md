# Contributing to BenchGuard

Thank you for helping make command-level performance checks more dependable.
Open an issue before a large change so scope and platform behavior can be
agreed before implementation.

## Local setup

BenchGuard requires Rust 1.85 or newer. npm launcher work also requires Node.js
18 or newer.

Run the release gates before requesting review:

```console
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo bench --bench core -- --test
node npm/test-launcher.mjs
node .github/scripts/test-release-contract.mjs
node .github/scripts/test-doc-contract.mjs
```

Build the fixture and CLI before running the README acceptance script:

```console
cargo build --workspace --locked
node scripts/readme-acceptance.mjs
```

Tests that change behavior should be written first and observed failing for
the intended reason. Platform-specific work must run on its real operating
system; a cross-compile is not runtime evidence.

## Commits and pull requests

Use focused Conventional Commits, for example:

```text
feat(linux): sample process-group memory
fix(windows): terminate timed-out jobs
docs: explain JSON report warnings
test: preserve failed-record baseline bytes
```

Describe the user-visible contract, RED/GREEN evidence for behavior changes,
platforms tested, and any remaining limitation. Do not combine unrelated
refactors with a functional change.

## Adding a platform backend

v0.1 supports only x86-64 Windows and Linux. A new backend must:

1. implement the platform-neutral runner contract in `src/runner`;
2. launch the executable and exact argument vector directly;
3. define and document its managed accounting and cleanup scope;
4. normalize wall/CPU durations to integer nanoseconds and memory to bytes;
5. enforce a bounded timeout and confirm managed descendants are no longer
   running before returning;
6. add real-platform tests for argument boundaries, descendant CPU, descendant
   memory, normal exit, timeout cleanup, and repeated reliability; and
7. update baseline compatibility, npm/release packaging, CI, README
   limitations, and the JSON/platform documentation.

Do not describe sampled or scoped metrics as exhaustive process-tree metrics.
Linux v0.1's 5 ms process-group sampling limitations are intentional public
behavior and must remain honest unless the implementation changes.

## Security

Do not open a public issue for a suspected vulnerability. Use GitHub's
**Security** tab and submit a private security advisory to the repository
maintainers. Include affected versions, platform, reproduction steps, impact,
and any suggested mitigation. Do not include secrets, private data, or
third-party credentials.

Maintainers will acknowledge the report privately, coordinate validation and a
fix, and credit reporters who want attribution. There is no bounty program.

All participation is governed by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
Contributions are accepted under the project's
[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE) license.
