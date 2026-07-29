# JSON format

BenchGuard uses two versioned JSON contracts:

- `benchguard.json` is the repository-committed baseline file.
- `--format json` writes a report envelope to standard output.

Both currently use `schema_version: 1`. Unsupported baseline schema versions
are rejected with exit code `2`; v0.1 does not migrate them automatically.
Durations are integer nanoseconds and memory is integer bytes.

## Baseline schema v1

The top level contains a map keyed by unique, non-empty benchmark names:

```json
{
  "schema_version": 1,
  "benchmarks": {
    "startup": {
      "program": "my-app",
      "args": ["--version"],
      "recorded_at": "2026-07-28T12:34:56Z",
      "platform": {
        "os": "linux",
        "arch": "x86_64"
      },
      "benchguard_version": "0.1.0",
      "warmups": 2,
      "runs": 10,
      "timeout_ns": null,
      "wall_ns": {
        "median": 12000000,
        "mean": 12100000,
        "standard_deviation": 300000,
        "min": 11700000,
        "max": 12700000,
        "p50": 12000000,
        "p95": 12700000,
        "sample_count": 10
      },
      "cpu_ns": {
        "median": 3000000,
        "mean": 3100000,
        "standard_deviation": 200000,
        "min": 2800000,
        "max": 3500000,
        "p50": 3000000,
        "p95": 3500000,
        "sample_count": 10
      },
      "peak_memory_bytes": {
        "median": 8388608,
        "mean": 8400000,
        "standard_deviation": 100000,
        "min": 8257536,
        "max": 8650752,
        "p50": 8388608,
        "p95": 8650752,
        "sample_count": 10
      },
      "budgets": {
        "wall_percent": 10.0,
        "cpu_percent": null,
        "peak_memory_percent": null
      },
      "noise_floors": {
        "wall_ns": 1000000,
        "cpu_ns": 1000000,
        "peak_memory_bytes": 1048576
      }
    }
  }
}
```

`sample_count` must equal `runs` for every aggregate. `recorded_at` is an RFC
3339 UTC timestamp. Budgets are either a finite non-negative percentage or
`null`. Noise floors must be positive. The exact `program` and `args` array are
launched directly; they are not a shell command string.

The repository includes this example as a CLI-validated
[sample baseline](../examples/benchguard.json).

## Report envelope v1

`record`, `check`, and `list` return the same top-level shape. Fields that do
not apply to a command are `null`, and `warnings` and `errors` are always
arrays:

```json
{
  "schema_version": 1,
  "status": "ok",
  "benchmarks": [
    {
      "name": "startup",
      "program": "my-app",
      "args": ["--version"],
      "platform": {
        "os": "linux",
        "arch": "x86_64"
      },
      "baseline_median_ns": 12000000,
      "current_median_ns": 12500000,
      "delta_ns": 500000,
      "relative_delta_pct": 4.166666666666667,
      "budget_pct": 10.0,
      "absolute_floor_ns": 1000000,
      "sample_count": 10,
      "status": "pass",
      "cpu_time": {
        "baseline": 3000000,
        "current": 3100000,
        "delta": 100000,
        "relative_delta_pct": 3.3333333333333335,
        "budget_pct": null,
        "absolute_floor": 1000000,
        "status": "unbudgeted",
        "unit": "ns"
      },
      "peak_memory": {
        "baseline": 8388608,
        "current": 8388608,
        "delta": 0,
        "relative_delta_pct": 0.0,
        "budget_pct": null,
        "absolute_floor": 1048576,
        "status": "unbudgeted",
        "unit": "bytes"
      }
    }
  ],
  "warnings": [],
  "errors": []
}
```

Top-level `status` is `ok`, `regression`, or `error`. Benchmark and metric
statuses are `recorded`, `baseline`, `pass`, `regression`, or `unbudgeted`.
Signed deltas may be negative when performance improves. Relative deltas are
`null` when a zero baseline makes a percentage undefined.

An operational error exits `2` and keeps the same envelope:

```json
{
  "schema_version": 1,
  "status": "error",
  "benchmarks": [],
  "warnings": [],
  "errors": [
    {
      "code": "timeout",
      "message": "benchmark command timed out"
    }
  ]
}
```

Error messages are actionable text; automation should branch on `code` and
exit status. Warning objects contain `code` and `message`. A
`high_variability` warning never changes an otherwise successful exit code.
