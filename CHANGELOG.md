# Changelog

All notable changes to BenchGuard are documented here. The project follows
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-07-29

### Added

- Native `record`, `check`, `list`, and context-sensitive `help` commands.
- Versioned `benchguard.json` baselines with atomic replacement.
- Wall-time, managed-scope CPU-time, and peak-memory measurements on x86-64
  Windows and Linux.
- Human and stable JSON reports, variability warnings, metric budgets, and
  automation-friendly exit codes.
- Per-run timeouts with Windows Job Object and Linux process-group cleanup.
- Cargo, npm launcher/native-package, and GitHub Release packaging automation.

### Changed

- Regression decisions use medians and require both the relative budget and
  absolute noise floor to be exceeded.
- Linux v0.1 metrics explicitly describe 5 ms sampled process-group accounting
  rather than exhaustive descendant accounting.

### Fixed

- Failed records preserve the previous baseline byte-for-byte.
- Exact coefficient-of-variation and budget boundaries avoid floating-point
  threshold drift.
- Windows argument quoting, PATH resolution, Job Object cleanup, and atomic
  baseline replacement handle platform edge cases.

### Security

- Targets execute directly without an implicit shell.
- Release actions are SHA-pinned; npm bootstrap is approval-gated and normal
  npm publishing uses trusted-publisher OIDC.
- Release publishing is separated by protected environments and ordered native
  npm packages before the launcher.
