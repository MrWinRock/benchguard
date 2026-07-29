use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

use crate::{domain::PlatformId, error::BenchguardError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineFileV1 {
    pub schema_version: u32,
    pub benchmarks: BTreeMap<String, BenchmarkV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkV1 {
    pub program: String,
    pub args: Vec<String>,
    pub recorded_at: String,
    pub platform: PlatformId,
    pub benchguard_version: String,
    pub warmups: u32,
    pub runs: u32,
    pub timeout_ns: Option<u64>,
    pub wall_ns: MetricAggregateV1,
    pub cpu_ns: MetricAggregateV1,
    pub peak_memory_bytes: MetricAggregateV1,
    pub budgets: BudgetsV1,
    pub noise_floors: NoiseFloorsV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricAggregateV1 {
    pub median: u64,
    pub mean: u64,
    pub standard_deviation: u64,
    pub min: u64,
    pub max: u64,
    pub p50: u64,
    pub p95: u64,
    pub sample_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetsV1 {
    pub wall_percent: Option<f64>,
    pub cpu_percent: Option<f64>,
    pub peak_memory_percent: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoiseFloorsV1 {
    pub wall_ns: u64,
    pub cpu_ns: u64,
    pub peak_memory_bytes: u64,
}

impl BaselineFileV1 {
    pub fn validate(&self) -> Result<(), BenchguardError> {
        if self.schema_version != 1 {
            return Err(BenchguardError::UnsupportedSchema(self.schema_version));
        }

        for (name, benchmark) in &self.benchmarks {
            if name.trim().is_empty() {
                return Err(invalid("benchmark name must not be empty"));
            }
            if name.chars().any(char::is_control) {
                return Err(invalid(
                    "benchmark name must not contain control characters",
                ));
            }
            benchmark.validate(name)?;
        }

        Ok(())
    }
}

impl BenchmarkV1 {
    fn validate(&self, name: &str) -> Result<(), BenchguardError> {
        if self.runs == 0 {
            return Err(invalid(format!(
                "benchmark {name:?} run count must be greater than zero"
            )));
        }
        if self.program.trim().is_empty() {
            return Err(invalid(format!(
                "benchmark {name:?} program must not be empty"
            )));
        }

        validate_aggregate(name, "wall_ns", &self.wall_ns, self.runs)?;
        validate_aggregate(name, "cpu_ns", &self.cpu_ns, self.runs)?;
        validate_aggregate(
            name,
            "peak_memory_bytes",
            &self.peak_memory_bytes,
            self.runs,
        )?;

        for (metric, budget) in [
            ("wall_percent", self.budgets.wall_percent),
            ("cpu_percent", self.budgets.cpu_percent),
            ("peak_memory_percent", self.budgets.peak_memory_percent),
        ] {
            if budget.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(invalid(format!(
                    "benchmark {name:?} budget {metric} must be finite and non-negative"
                )));
            }
        }

        for (metric, floor) in [
            ("wall_ns", self.noise_floors.wall_ns),
            ("cpu_ns", self.noise_floors.cpu_ns),
            ("peak_memory_bytes", self.noise_floors.peak_memory_bytes),
        ] {
            if floor == 0 {
                return Err(invalid(format!(
                    "benchmark {name:?} noise floor {metric} must be greater than zero"
                )));
            }
        }

        if self.platform.os.trim().is_empty() {
            return Err(invalid(format!(
                "benchmark {name:?} platform OS must not be empty"
            )));
        }
        if self.platform.os.chars().any(char::is_control) {
            return Err(invalid(format!(
                "benchmark {name:?} platform OS must not contain control characters"
            )));
        }
        if self.platform.arch.trim().is_empty() {
            return Err(invalid(format!(
                "benchmark {name:?} platform architecture must not be empty"
            )));
        }
        if self.platform.arch.chars().any(char::is_control) {
            return Err(invalid(format!(
                "benchmark {name:?} platform architecture must not contain control characters"
            )));
        }

        let recorded_at = OffsetDateTime::parse(&self.recorded_at, &Rfc3339).map_err(|_| {
            invalid(format!(
                "benchmark {name:?} recorded_at must be an RFC 3339 timestamp"
            ))
        })?;
        if recorded_at.offset() != UtcOffset::UTC {
            return Err(invalid(format!(
                "benchmark {name:?} recorded_at must use UTC"
            )));
        }

        Ok(())
    }
}

fn validate_aggregate(
    benchmark_name: &str,
    metric: &str,
    aggregate: &MetricAggregateV1,
    runs: u32,
) -> Result<(), BenchguardError> {
    if aggregate.sample_count != runs {
        return Err(invalid(format!(
            "benchmark {benchmark_name:?} {metric} sample count must equal runs"
        )));
    }
    if !(aggregate.min <= aggregate.p50
        && aggregate.p50 <= aggregate.median
        && aggregate.median <= aggregate.p95
        && aggregate.p95 <= aggregate.max)
    {
        return Err(invalid(format!(
            "benchmark {benchmark_name:?} {metric} order statistics are inconsistent"
        )));
    }
    if aggregate.sample_count % 2 == 1 && aggregate.p50 != aggregate.median {
        return Err(invalid(format!(
            "benchmark {benchmark_name:?} {metric} p50 must equal median for an odd sample count"
        )));
    }
    if aggregate.mean < aggregate.min || aggregate.mean > aggregate.max {
        return Err(invalid(format!(
            "benchmark {benchmark_name:?} {metric} mean must be within the observed range"
        )));
    }
    if aggregate.standard_deviation > aggregate.max - aggregate.min {
        return Err(invalid(format!(
            "benchmark {benchmark_name:?} {metric} standard deviation exceeds the observed range"
        )));
    }

    Ok(())
}

fn invalid(message: impl Into<String>) -> BenchguardError {
    BenchguardError::InvalidBaseline(message.into())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{BaselineFileV1, BenchmarkV1, BudgetsV1, MetricAggregateV1, NoiseFloorsV1};
    use crate::{domain::PlatformId, error::BenchguardError};

    fn example_aggregate() -> MetricAggregateV1 {
        MetricAggregateV1 {
            median: 1_000,
            mean: 1_010,
            standard_deviation: 25,
            min: 950,
            max: 1_100,
            p50: 1_000,
            p95: 1_100,
            sample_count: 3,
        }
    }

    fn example_baseline() -> BaselineFileV1 {
        BaselineFileV1 {
            schema_version: 1,
            benchmarks: BTreeMap::from([(
                "startup".to_owned(),
                BenchmarkV1 {
                    program: "benchguard-fixture".to_owned(),
                    args: vec!["sleep-ms".to_owned(), "10".to_owned()],
                    recorded_at: "2026-07-28T12:34:56Z".to_owned(),
                    platform: PlatformId {
                        os: "windows".to_owned(),
                        arch: "x86_64".to_owned(),
                    },
                    benchguard_version: "0.1.0".to_owned(),
                    warmups: 2,
                    runs: 3,
                    timeout_ns: Some(5_000_000_000),
                    wall_ns: example_aggregate(),
                    cpu_ns: example_aggregate(),
                    peak_memory_bytes: example_aggregate(),
                    budgets: BudgetsV1 {
                        wall_percent: Some(10.0),
                        cpu_percent: None,
                        peak_memory_percent: Some(15.5),
                    },
                    noise_floors: NoiseFloorsV1 {
                        wall_ns: 1_000_000,
                        cpu_ns: 500_000,
                        peak_memory_bytes: 1_048_576,
                    },
                },
            )]),
        }
    }

    fn benchmark_mut(baseline: &mut BaselineFileV1) -> &mut BenchmarkV1 {
        baseline.benchmarks.get_mut("startup").unwrap()
    }

    fn assert_invalid_baseline(result: Result<(), BenchguardError>) {
        assert!(matches!(result, Err(BenchguardError::InvalidBaseline(_))));
    }

    #[test]
    fn v1_round_trip_uses_integer_base_units_and_exact_field_names() {
        let baseline = example_baseline();
        let json = serde_json::to_string_pretty(&baseline).unwrap();
        let expected = r#"{
  "schema_version": 1,
  "benchmarks": {
    "startup": {
      "program": "benchguard-fixture",
      "args": [
        "sleep-ms",
        "10"
      ],
      "recorded_at": "2026-07-28T12:34:56Z",
      "platform": {
        "os": "windows",
        "arch": "x86_64"
      },
      "benchguard_version": "0.1.0",
      "warmups": 2,
      "runs": 3,
      "timeout_ns": 5000000000,
      "wall_ns": {
        "median": 1000,
        "mean": 1010,
        "standard_deviation": 25,
        "min": 950,
        "max": 1100,
        "p50": 1000,
        "p95": 1100,
        "sample_count": 3
      },
      "cpu_ns": {
        "median": 1000,
        "mean": 1010,
        "standard_deviation": 25,
        "min": 950,
        "max": 1100,
        "p50": 1000,
        "p95": 1100,
        "sample_count": 3
      },
      "peak_memory_bytes": {
        "median": 1000,
        "mean": 1010,
        "standard_deviation": 25,
        "min": 950,
        "max": 1100,
        "p50": 1000,
        "p95": 1100,
        "sample_count": 3
      },
      "budgets": {
        "wall_percent": 10.0,
        "cpu_percent": null,
        "peak_memory_percent": 15.5
      },
      "noise_floors": {
        "wall_ns": 1000000,
        "cpu_ns": 500000,
        "peak_memory_bytes": 1048576
      }
    }
  }
}"#;

        assert_eq!(json, expected);
        assert_eq!(
            serde_json::from_str::<BaselineFileV1>(expected).unwrap(),
            baseline
        );
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        let mut baseline = example_baseline();
        baseline.schema_version = 9;

        assert!(matches!(
            baseline.validate(),
            Err(BenchguardError::UnsupportedSchema(9))
        ));
    }

    #[test]
    fn rejects_whitespace_only_benchmark_name() {
        let mut baseline = example_baseline();
        let benchmark = baseline.benchmarks.remove("startup").unwrap();
        baseline.benchmarks.insert(" \t".to_owned(), benchmark);

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_control_characters_in_benchmark_name() {
        for name in [
            "forged\nwarning",
            "terminal\u{1b}]8;;https://example.com\u{7}link",
        ] {
            let mut baseline = example_baseline();
            let benchmark = baseline.benchmarks.remove("startup").unwrap();
            baseline.benchmarks.insert(name.to_owned(), benchmark);

            assert_invalid_baseline(baseline.validate());
        }
    }

    #[test]
    fn rejects_control_characters_in_platform_labels() {
        let mut os_baseline = example_baseline();
        benchmark_mut(&mut os_baseline).platform.os = "linux\nforged".to_owned();
        assert_invalid_baseline(os_baseline.validate());

        let mut arch_baseline = example_baseline();
        benchmark_mut(&mut arch_baseline).platform.arch =
            "x86_64\u{1b}]8;;https://example.com\u{7}".to_owned();
        assert_invalid_baseline(arch_baseline.validate());
    }

    #[test]
    fn rejects_zero_runs() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).runs = 0;

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_missing_program() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).program = "  ".to_owned();

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_wall_sample_count_that_differs_from_runs() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).wall_ns.sample_count = 2;

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_cpu_sample_count_that_differs_from_runs() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).cpu_ns.sample_count = 2;

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_memory_sample_count_that_differs_from_runs() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).peak_memory_bytes.sample_count = 2;

        assert_invalid_baseline(baseline.validate());
    }

    // Catches accepting aggregates whose order statistics cannot have been
    // produced by any sample set.
    #[test]
    fn rejects_impossible_aggregate_ordering() {
        for mutate in [
            |aggregate: &mut MetricAggregateV1| aggregate.min = aggregate.p50 + 1,
            |aggregate: &mut MetricAggregateV1| aggregate.p50 = aggregate.median + 1,
            |aggregate: &mut MetricAggregateV1| aggregate.median = aggregate.p95 + 1,
            |aggregate: &mut MetricAggregateV1| aggregate.p95 = aggregate.max + 1,
        ] {
            let mut baseline = example_baseline();
            mutate(&mut benchmark_mut(&mut baseline).wall_ns);
            assert_invalid_baseline(baseline.validate());
        }
    }

    // Catches accepting distinct p50 and median values for an odd sample
    // count, where both select the same middle order statistic.
    #[test]
    fn rejects_inconsistent_odd_sample_p50_and_median() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).wall_ns.p50 = 999;

        assert_invalid_baseline(baseline.validate());
    }

    // Catches accepting a mean outside the observed range or a population
    // standard deviation larger than the complete observed range.
    #[test]
    fn rejects_impossible_aggregate_moments() {
        let mut below_minimum = example_baseline();
        benchmark_mut(&mut below_minimum).wall_ns.mean = 949;
        assert_invalid_baseline(below_minimum.validate());

        let mut above_maximum = example_baseline();
        benchmark_mut(&mut above_maximum).wall_ns.mean = 1_101;
        assert_invalid_baseline(above_maximum.validate());

        let mut excessive_spread = example_baseline();
        benchmark_mut(&mut excessive_spread)
            .wall_ns
            .standard_deviation = 151;
        assert_invalid_baseline(excessive_spread.validate());
    }

    // Catches accepting a nonzero spread for a one-valued sample set.
    #[test]
    fn rejects_nonzero_deviation_when_minimum_equals_maximum() {
        let mut baseline = example_baseline();
        let aggregate = &mut benchmark_mut(&mut baseline).cpu_ns;
        aggregate.median = 0;
        aggregate.mean = 0;
        aggregate.min = 0;
        aggregate.max = 0;
        aggregate.p50 = 0;
        aggregate.p95 = 0;
        aggregate.standard_deviation = 1;

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_negative_wall_budget() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).budgets.wall_percent = Some(-0.1);

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_negative_cpu_budget() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).budgets.cpu_percent = Some(-0.1);

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_negative_memory_budget() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).budgets.peak_memory_percent = Some(-0.1);

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_non_finite_nan_budget() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).budgets.wall_percent = Some(f64::NAN);

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_non_finite_positive_infinity_budget() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).budgets.cpu_percent = Some(f64::INFINITY);

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_zero_wall_noise_floor() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).noise_floors.wall_ns = 0;

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_zero_cpu_noise_floor() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).noise_floors.cpu_ns = 0;

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_zero_memory_noise_floor() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).noise_floors.peak_memory_bytes = 0;

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_empty_platform_os() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).platform.os = String::new();

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_whitespace_only_platform_architecture() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).platform.arch = " \n".to_owned();

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_non_rfc3339_recorded_at_timestamp() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).recorded_at = "2026-07-28 12:34:56".to_owned();

        assert_invalid_baseline(baseline.validate());
    }

    #[test]
    fn rejects_non_utc_recorded_at_timestamp() {
        let mut baseline = example_baseline();
        benchmark_mut(&mut baseline).recorded_at = "2026-07-28T19:34:56+07:00".to_owned();

        assert_invalid_baseline(baseline.validate());
    }
}
