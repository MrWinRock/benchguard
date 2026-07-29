# `@benchguard/linux-x64`

Native BenchGuard binary package for x86-64 Linux. This package is selected by
`@benchguard/cli`; install the CLI package rather than depending on this
platform package directly.

Linux v0.1 samples the target session/process group every 5 ms. Processes that
start and exit between samples, or descendants that leave the group, may be
missed.
