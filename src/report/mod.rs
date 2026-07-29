pub mod human;
pub mod json;

use serde::Serialize;

use crate::{domain::PlatformId, error::BenchguardError};

pub use human::HumanRenderer;
pub use json::JsonRenderer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportStatus {
    Ok,
    Regression,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BenchmarkStatus {
    Recorded,
    Baseline,
    Pass,
    Regression,
    Unbudgeted,
}

impl BenchmarkStatus {
    pub(crate) fn human_label(self) -> &'static str {
        match self {
            Self::Recorded => "RECORDED",
            Self::Baseline => "BASELINE",
            Self::Pass => "PASS",
            Self::Regression => "REGRESSION",
            Self::Unbudgeted => "UNBUDGETED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MetricUnit {
    #[serde(rename = "ns")]
    Nanoseconds,
    #[serde(rename = "bytes")]
    Bytes,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricReport {
    pub baseline: u64,
    pub current: Option<u64>,
    pub delta: Option<i128>,
    pub relative_delta_pct: Option<f64>,
    pub budget_pct: Option<f64>,
    pub absolute_floor: u64,
    pub status: BenchmarkStatus,
    pub unit: MetricUnit,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReportBenchmark {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub platform: PlatformId,
    pub baseline_median_ns: u64,
    pub current_median_ns: Option<u64>,
    pub delta_ns: Option<i128>,
    pub relative_delta_pct: Option<f64>,
    pub budget_pct: Option<f64>,
    pub absolute_floor_ns: u64,
    pub sample_count: u32,
    pub status: BenchmarkStatus,
    pub cpu_time: MetricReport,
    pub peak_memory: MetricReport,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReportError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReportWarning {
    pub code: String,
    pub message: String,
}

impl From<&BenchguardError> for ReportError {
    fn from(error: &BenchguardError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub status: ReportStatus,
    pub benchmarks: Vec<ReportBenchmark>,
    pub warnings: Vec<ReportWarning>,
    pub errors: Vec<ReportError>,
}

impl Report {
    pub fn operational_error(error: &BenchguardError) -> Self {
        Self {
            schema_version: 1,
            status: ReportStatus::Error,
            benchmarks: Vec::new(),
            warnings: Vec::new(),
            errors: vec![ReportError::from(error)],
        }
    }
}

pub trait ReportRenderer {
    fn render(&self, report: &Report) -> String;
}

#[cfg(test)]
mod tests {
    use crate::domain::PlatformId;

    use super::{
        BenchmarkStatus, HumanRenderer, JsonRenderer, MetricReport, MetricUnit, Report,
        ReportBenchmark, ReportRenderer, ReportStatus, ReportWarning,
    };

    fn regression_report() -> Report {
        Report {
            schema_version: 1,
            status: ReportStatus::Regression,
            benchmarks: vec![ReportBenchmark {
                name: "startup".to_owned(),
                program: "fixture".to_owned(),
                args: vec!["sleep-ms".to_owned(), "80".to_owned()],
                platform: PlatformId {
                    os: "windows".to_owned(),
                    arch: "x86_64".to_owned(),
                },
                baseline_median_ns: 10_000_000,
                current_median_ns: Some(80_000_000),
                delta_ns: Some(70_000_000),
                relative_delta_pct: Some(700.0),
                budget_pct: Some(10.0),
                absolute_floor_ns: 1_000_000,
                sample_count: 3,
                status: BenchmarkStatus::Regression,
                cpu_time: MetricReport {
                    baseline: 20_000_000,
                    current: Some(22_000_000),
                    delta: Some(2_000_000),
                    relative_delta_pct: Some(10.0),
                    budget_pct: Some(10.0),
                    absolute_floor: 1_000_000,
                    status: BenchmarkStatus::Pass,
                    unit: MetricUnit::Nanoseconds,
                },
                peak_memory: MetricReport {
                    baseline: 8_388_608,
                    current: Some(10_485_760),
                    delta: Some(2_097_152),
                    relative_delta_pct: Some(25.0),
                    budget_pct: Some(20.0),
                    absolute_floor: 1_048_576,
                    status: BenchmarkStatus::Regression,
                    unit: MetricUnit::Bytes,
                },
            }],
            warnings: vec![ReportWarning {
                code: "high_variability".to_owned(),
                message: "startup wall-time coefficient of variation is 11.00% (threshold: 10.00%)"
                    .to_owned(),
            }],
            errors: Vec::new(),
        }
    }

    // Catches a renderer that omits any threshold input or the final outcome
    // needed to explain a budget decision.
    #[test]
    fn human_report_explains_every_wall_time_threshold_input() {
        let output = HumanRenderer.render(&regression_report());

        for expected in [
            "startup",
            "REGRESSION",
            "baseline: 10 ms",
            "current median: 80 ms",
            "delta: +70 ms (+700.00%)",
            "budget: +10.00%",
            "floor: 1 ms",
            "samples: 3",
            "cpu time:",
            "baseline: 20 ms",
            "peak memory:",
            "baseline: 8 MiB",
            "current median: 10 MiB",
            "delta: +2 MiB (+25.00%)",
            "floor: 1 MiB",
            "warning [high_variability]",
        ] {
            assert!(output.contains(expected), "missing {expected:?}: {output}");
        }
    }

    // Catches shape drift, unsigned improvement deltas, or accidental
    // non-finite JSON values. Expected fields are literals from the contract.
    #[test]
    fn json_report_uses_the_stable_flat_benchmark_shape() {
        let mut report = regression_report();
        report.status = ReportStatus::Ok;
        report.benchmarks[0].current_median_ns = Some(0);
        report.benchmarks[0].delta_ns = Some(-10_000_000);
        report.benchmarks[0].relative_delta_pct = None;
        report.benchmarks[0].budget_pct = None;
        report.benchmarks[0].status = BenchmarkStatus::Unbudgeted;

        let json = JsonRenderer.render(&report);
        assert!(!json.contains("\u{1b}["));
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["status"], "ok");
        assert_eq!(
            value["warnings"],
            serde_json::json!([{
                "code": "high_variability",
                "message": "startup wall-time coefficient of variation is 11.00% (threshold: 10.00%)"
            }])
        );
        assert_eq!(value["errors"], serde_json::json!([]));
        assert_eq!(
            value["benchmarks"][0],
            serde_json::json!({
                "name": "startup",
                "program": "fixture",
                "args": ["sleep-ms", "80"],
                "platform": {"os": "windows", "arch": "x86_64"},
                "baseline_median_ns": 10_000_000,
                "current_median_ns": 0,
                "delta_ns": -10_000_000,
                "relative_delta_pct": null,
                "budget_pct": null,
                "absolute_floor_ns": 1_000_000,
                "sample_count": 3,
                "status": "unbudgeted",
                "cpu_time": {
                    "baseline": 20_000_000,
                    "current": 22_000_000,
                    "delta": 2_000_000,
                    "relative_delta_pct": 10.0,
                    "budget_pct": 10.0,
                    "absolute_floor": 1_000_000,
                    "status": "pass",
                    "unit": "ns"
                },
                "peak_memory": {
                    "baseline": 8_388_608,
                    "current": 10_485_760,
                    "delta": 2_097_152,
                    "relative_delta_pct": 25.0,
                    "budget_pct": 20.0,
                    "absolute_floor": 1_048_576,
                    "status": "regression",
                    "unit": "bytes"
                }
            })
        );
    }

    // Catches renaming or omitting any additive system-metric field, using an
    // unstable status spelling, or reporting scaled rather than base units.
    #[test]
    fn metric_report_and_warning_use_the_stable_json_contract() {
        let metric = MetricReport {
            baseline: 10,
            current: Some(12),
            delta: Some(2),
            relative_delta_pct: Some(20.0),
            budget_pct: Some(10.0),
            absolute_floor: 1,
            status: BenchmarkStatus::Regression,
            unit: MetricUnit::Bytes,
        };
        assert_eq!(
            serde_json::to_value(metric).unwrap(),
            serde_json::json!({
                "baseline": 10,
                "current": 12,
                "delta": 2,
                "relative_delta_pct": 20.0,
                "budget_pct": 10.0,
                "absolute_floor": 1,
                "status": "regression",
                "unit": "bytes"
            })
        );
        assert_eq!(
            serde_json::to_value(ReportWarning {
                code: "high_variability".to_owned(),
                message: "wall-time CV exceeded 10.0%".to_owned(),
            })
            .unwrap(),
            serde_json::json!({
                "code": "high_variability",
                "message": "wall-time CV exceeded 10.0%"
            })
        );
        assert_eq!(
            serde_json::to_value(MetricUnit::Nanoseconds).unwrap(),
            serde_json::json!("ns")
        );
    }
}
