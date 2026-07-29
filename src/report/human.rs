use std::fmt::Write;

use super::{MetricReport, Report, ReportRenderer};

pub struct HumanRenderer;

impl ReportRenderer for HumanRenderer {
    fn render(&self, report: &Report) -> String {
        let mut output = String::new();

        for benchmark in &report.benchmarks {
            let _ = writeln!(
                output,
                "{}: {}",
                benchmark.name,
                benchmark.status.human_label()
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
            let _ = writeln!(output, "  baseline: {} ns", benchmark.baseline_median_ns);
            match benchmark.current_median_ns {
                Some(current) => {
                    let _ = writeln!(output, "  current median: {current} ns");
                }
                None => {
                    let _ = writeln!(output, "  current median: n/a");
                }
            }
            match (benchmark.delta_ns, benchmark.relative_delta_pct) {
                (Some(delta), Some(relative)) => {
                    let _ = writeln!(output, "  delta: {delta:+} ns ({relative:+.2}%)");
                }
                (Some(delta), None) => {
                    let _ = writeln!(output, "  delta: {delta:+} ns (n/a)");
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
            let _ = writeln!(output, "  floor: {} ns", benchmark.absolute_floor_ns);
            let _ = writeln!(output, "  samples: {}", benchmark.sample_count);
            render_metric(&mut output, "cpu time", &benchmark.cpu_time);
            render_metric(&mut output, "peak memory", &benchmark.peak_memory);
        }

        for warning in &report.warnings {
            let _ = writeln!(output, "warning [{}]: {}", warning.code, warning.message);
        }

        output
    }
}

fn render_metric(output: &mut String, label: &str, metric: &MetricReport) {
    let unit = match metric.unit {
        super::MetricUnit::Nanoseconds => "ns",
        super::MetricUnit::Bytes => "bytes",
    };
    let _ = writeln!(output, "  {label}:");
    let _ = writeln!(output, "    status: {}", metric.status.human_label());
    let _ = writeln!(output, "    baseline: {} {unit}", metric.baseline);
    match metric.current {
        Some(current) => {
            let _ = writeln!(output, "    current median: {current} {unit}");
        }
        None => {
            let _ = writeln!(output, "    current median: n/a");
        }
    }
    match (metric.delta, metric.relative_delta_pct) {
        (Some(delta), Some(relative)) => {
            let _ = writeln!(output, "    delta: {delta:+} {unit} ({relative:+.2}%)");
        }
        (Some(delta), None) => {
            let _ = writeln!(output, "    delta: {delta:+} {unit} (n/a)");
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
    let _ = writeln!(output, "    floor: {} {unit}", metric.absolute_floor);
}
