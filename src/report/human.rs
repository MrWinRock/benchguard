use std::fmt::Write;

use super::{BenchmarkStatus, MetricReport, Report, ReportRenderer};

pub struct HumanRenderer {
    color: bool,
}

const RESET: &str = "\x1b[0m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";

const TIME_UNITS: &[(u128, &str)] = &[
    (1_000_000_000, "s"),
    (1_000_000, "ms"),
    (1_000, "µs"),
    (1, "ns"),
];
const MEMORY_UNITS: &[(u128, &str)] = &[
    (1_073_741_824, "GiB"),
    (1_048_576, "MiB"),
    (1_024, "KiB"),
    (1, "bytes"),
];

impl ReportRenderer for HumanRenderer {
    fn render(&self, report: &Report) -> String {
        let mut output = String::new();

        for benchmark in &report.benchmarks {
            let _ = writeln!(
                output,
                "{}: {}",
                self.style(CYAN, &benchmark.name),
                self.status(benchmark.status)
            );
            let _ = writeln!(
                output,
                "  command: {:?} {:?}",
                benchmark.program, benchmark.args
            );
            let _ = writeln!(
                output,
                "  platform: {}/{}",
                benchmark.platform.os, benchmark.platform.arch
            );
            let _ = writeln!(
                output,
                "  baseline: {}",
                format_unsigned(benchmark.baseline_median_ns, super::MetricUnit::Nanoseconds)
            );
            match benchmark.current_median_ns {
                Some(current) => {
                    let _ = writeln!(
                        output,
                        "  current median: {}",
                        format_unsigned(current, super::MetricUnit::Nanoseconds)
                    );
                }
                None => {
                    let _ = writeln!(output, "  current median: n/a");
                }
            }
            match (benchmark.delta_ns, benchmark.relative_delta_pct) {
                (Some(delta), Some(relative)) => {
                    let _ = writeln!(
                        output,
                        "  delta: {} ({relative:+.2}%)",
                        format_signed(delta, super::MetricUnit::Nanoseconds)
                    );
                }
                (Some(delta), None) => {
                    let _ = writeln!(
                        output,
                        "  delta: {} (n/a)",
                        format_signed(delta, super::MetricUnit::Nanoseconds)
                    );
                }
                (None, _) => {
                    let _ = writeln!(output, "  delta: n/a");
                }
            }
            match benchmark.budget_pct {
                Some(budget) => {
                    let _ = writeln!(output, "  budget: +{budget:.2}%");
                }
                None => {
                    let _ = writeln!(output, "  budget: none");
                }
            }
            let _ = writeln!(
                output,
                "  floor: {}",
                format_unsigned(benchmark.absolute_floor_ns, super::MetricUnit::Nanoseconds)
            );
            let _ = writeln!(output, "  samples: {}", benchmark.sample_count);
            self.render_metric(&mut output, "cpu time", &benchmark.cpu_time);
            self.render_metric(&mut output, "peak memory", &benchmark.peak_memory);
        }

        for warning in &report.warnings {
            let _ = writeln!(
                output,
                "{} [{}]: {}",
                self.style(YELLOW, "warning"),
                warning.code,
                warning.message
            );
        }

        output
    }
}

impl HumanRenderer {
    pub const fn new(color: bool) -> Self {
        Self { color }
    }

    fn style(&self, color: &str, text: &str) -> String {
        if self.color {
            format!("{color}{text}{RESET}")
        } else {
            text.to_owned()
        }
    }

    fn status(&self, status: BenchmarkStatus) -> String {
        let color = match status {
            BenchmarkStatus::Recorded | BenchmarkStatus::Pass => GREEN,
            BenchmarkStatus::Regression => RED,
            BenchmarkStatus::Unbudgeted => YELLOW,
            BenchmarkStatus::Baseline => CYAN,
        };
        self.style(color, status.human_label())
    }

    fn render_metric(&self, output: &mut String, label: &str, metric: &MetricReport) {
        let _ = writeln!(output, "  {label}:");
        let _ = writeln!(output, "    status: {}", self.status(metric.status));
        let _ = writeln!(
            output,
            "    baseline: {}",
            format_unsigned(metric.baseline, metric.unit)
        );
        match metric.current {
            Some(current) => {
                let _ = writeln!(
                    output,
                    "    current median: {}",
                    format_unsigned(current, metric.unit)
                );
            }
            None => {
                let _ = writeln!(output, "    current median: n/a");
            }
        }
        match (metric.delta, metric.relative_delta_pct) {
            (Some(delta), Some(relative)) => {
                let _ = writeln!(
                    output,
                    "    delta: {} ({relative:+.2}%)",
                    format_signed(delta, metric.unit)
                );
            }
            (Some(delta), None) => {
                let _ = writeln!(
                    output,
                    "    delta: {} (n/a)",
                    format_signed(delta, metric.unit)
                );
            }
            (None, _) => {
                let _ = writeln!(output, "    delta: n/a");
            }
        }
        match metric.budget_pct {
            Some(budget) => {
                let _ = writeln!(output, "    budget: +{budget:.2}%");
            }
            None => {
                let _ = writeln!(output, "    budget: none");
            }
        }
        let _ = writeln!(
            output,
            "    floor: {}",
            format_unsigned(metric.absolute_floor, metric.unit)
        );
    }
}

fn format_unsigned(value: u64, unit: super::MetricUnit) -> String {
    format_magnitude(value.into(), unit)
}

fn format_signed(value: i128, unit: super::MetricUnit) -> String {
    let sign = if value < 0 { '-' } else { '+' };
    format!("{sign}{}", format_magnitude(value.unsigned_abs(), unit))
}

fn format_magnitude(value: u128, unit: super::MetricUnit) -> String {
    let (threshold, suffix) = unit_table(unit)
        .iter()
        .find(|(threshold, _)| value >= *threshold)
        .or_else(|| unit_table(unit).last())
        .expect("unit tables include a base unit");
    let whole = value / threshold;
    let hundredths = (value % threshold) * 100 / threshold;

    if hundredths == 0 {
        format!("{whole} {suffix}")
    } else if hundredths % 10 == 0 {
        format!("{whole}.{} {suffix}", hundredths / 10)
    } else {
        format!("{whole}.{hundredths:02} {suffix}")
    }
}

fn unit_table(unit: super::MetricUnit) -> &'static [(u128, &'static str)] {
    match unit {
        super::MetricUnit::Nanoseconds => TIME_UNITS,
        super::MetricUnit::Bytes => MEMORY_UNITS,
    }
}

#[cfg(test)]
mod tests {
    use super::{format_signed, format_unsigned};
    use crate::report::MetricUnit;

    // Catches selecting the wrong threshold or leaving raw base units in
    // human-readable reports.
    #[test]
    fn adaptive_units_use_the_largest_fitting_threshold() {
        for (value, unit, expected) in [
            (999, MetricUnit::Nanoseconds, "999 ns"),
            (0, MetricUnit::Nanoseconds, "0 ns"),
            (1_000, MetricUnit::Nanoseconds, "1 µs"),
            (1_500, MetricUnit::Nanoseconds, "1.5 µs"),
            (1_000_000, MetricUnit::Nanoseconds, "1 ms"),
            (1_500_000_000, MetricUnit::Nanoseconds, "1.5 s"),
            (1_023, MetricUnit::Bytes, "1023 bytes"),
            (1_024, MetricUnit::Bytes, "1 KiB"),
            (1_572_864, MetricUnit::Bytes, "1.5 MiB"),
            (1_610_612_736, MetricUnit::Bytes, "1.5 GiB"),
        ] {
            assert_eq!(format_unsigned(value, unit), expected);
        }
    }

    // Catches dropping signs or scaling deltas differently from metric values.
    #[test]
    fn adaptive_units_preserve_signed_deltas() {
        assert_eq!(
            format_signed(-1_500_000, MetricUnit::Nanoseconds),
            "-1.5 ms"
        );
        assert_eq!(format_signed(1_572_864, MetricUnit::Bytes), "+1.5 MiB");
        assert_eq!(format_signed(0, MetricUnit::Nanoseconds), "+0 ns");
    }
}
