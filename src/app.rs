use std::{
    collections::BTreeMap,
    io::{self, IsTerminal},
    path::Path,
    time::Duration,
};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    baseline::{
        schema::{BaselineFileV1, BenchmarkV1, BudgetsV1, MetricAggregateV1, NoiseFloorsV1},
        store::BaselineStore,
    },
    cli::{CheckArgs, Cli, Command, ListArgs, OutputFormat, RecordArgs},
    comparison::{MetricOutcome, compare},
    domain::{Aggregate, PlatformId},
    error::{BenchguardError, ExitClass},
    report::{
        BenchmarkStatus, HumanRenderer, JsonRenderer, MetricReport, MetricUnit, Report,
        ReportBenchmark, ReportRenderer, ReportStatus, ReportWarning,
    },
    runner::{CommandSpec, RunConfig, run},
    stats::{
        aggregate, coefficient_of_variation_exceeds_ten_percent, coefficient_of_variation_pct,
    },
};

const WALL_NOISE_FLOOR_NS: u64 = 1_000_000;
const CPU_NOISE_FLOOR_NS: u64 = 1_000_000;
const MEMORY_NOISE_FLOOR_BYTES: u64 = 1_048_576;
const WALL_VARIABILITY_WARNING_PCT: f64 = 10.0;

pub fn execute(cli: Cli) -> Result<ExitClass, BenchguardError> {
    let format = cli.output_format();
    let color = cli.color.enabled(
        io::stdout().is_terminal(),
        std::env::var_os("NO_COLOR").is_some(),
        format,
    );
    match cli.command {
        Command::Record(args) => record(args, color),
        Command::Check(args) => check(args, color),
        Command::List(args) => list(args, color),
    }
}

fn record(args: RecordArgs, color: bool) -> Result<ExitClass, BenchguardError> {
    let mut baseline = load_for_record(&args.run.file)?;

    let spec = command_from_target(&args.target);
    let samples = run(
        &spec,
        &RunConfig {
            warmups: args.run.warmup,
            runs: args.run.runs,
            timeout: args.run.timeout,
        },
    )?;
    let wall_samples = samples
        .iter()
        .map(|sample| sample.wall_ns)
        .collect::<Vec<_>>();
    let wall_ns = aggregate(&wall_samples)?;
    let warnings = variability_warnings(&args.name, &wall_samples)?;
    let cpu_ns = aggregate(
        &samples
            .iter()
            .map(|sample| sample.cpu_ns)
            .collect::<Vec<_>>(),
    )?;
    let peak_memory_bytes = aggregate(
        &samples
            .iter()
            .map(|sample| sample.peak_memory_bytes)
            .collect::<Vec<_>>(),
    )?;
    let platform = current_platform();
    let benchmark = BenchmarkV1 {
        program: args.target[0].clone(),
        args: args.target[1..].to_vec(),
        recorded_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(BenchguardError::TimestampFormatting)?,
        platform: platform.clone(),
        benchguard_version: env!("CARGO_PKG_VERSION").to_owned(),
        warmups: args.run.warmup,
        runs: args.run.runs,
        timeout_ns: args.run.timeout.map(duration_ns).transpose()?,
        wall_ns: metric_aggregate(wall_ns.clone()),
        cpu_ns: metric_aggregate(cpu_ns.clone()),
        peak_memory_bytes: metric_aggregate(peak_memory_bytes.clone()),
        budgets: BudgetsV1 {
            wall_percent: args.max_time.map(|budget| budget.0),
            cpu_percent: args.max_cpu.map(|budget| budget.0),
            peak_memory_percent: args.max_memory.map(|budget| budget.0),
        },
        noise_floors: NoiseFloorsV1 {
            wall_ns: WALL_NOISE_FLOOR_NS,
            cpu_ns: CPU_NOISE_FLOOR_NS,
            peak_memory_bytes: MEMORY_NOISE_FLOOR_BYTES,
        },
    };

    baseline.benchmarks.insert(args.name.clone(), benchmark);
    BaselineStore::save_atomic(&args.run.file, &baseline)?;

    emit(
        args.format,
        color,
        &Report {
            schema_version: 1,
            status: ReportStatus::Ok,
            benchmarks: vec![ReportBenchmark {
                name: args.name,
                program: args.target[0].clone(),
                args: args.target[1..].to_vec(),
                platform,
                baseline_median_ns: wall_ns.median,
                current_median_ns: None,
                delta_ns: None,
                relative_delta_pct: None,
                budget_pct: args.max_time.map(|budget| budget.0),
                absolute_floor_ns: WALL_NOISE_FLOOR_NS,
                sample_count: wall_ns.sample_count,
                status: BenchmarkStatus::Recorded,
                cpu_time: metric_report(
                    cpu_ns.median,
                    None,
                    args.max_cpu.map(|budget| budget.0),
                    CPU_NOISE_FLOOR_NS,
                    BenchmarkStatus::Recorded,
                    MetricUnit::Nanoseconds,
                ),
                peak_memory: metric_report(
                    peak_memory_bytes.median,
                    None,
                    args.max_memory.map(|budget| budget.0),
                    MEMORY_NOISE_FLOOR_BYTES,
                    BenchmarkStatus::Recorded,
                    MetricUnit::Bytes,
                ),
            }],
            warnings,
            errors: Vec::new(),
        },
    );

    Ok(ExitClass::Success)
}

fn check(args: CheckArgs, color: bool) -> Result<ExitClass, BenchguardError> {
    let baseline = BaselineStore::load(&args.file)?;
    let stored = baseline
        .benchmarks
        .get(&args.name)
        .ok_or_else(|| BenchguardError::BenchmarkNotFound(args.name.clone()))?;
    let platform = current_platform();
    if stored.platform != platform {
        return Err(BenchguardError::IncompatiblePlatform {
            baseline_os: stored.platform.os.clone(),
            baseline_arch: stored.platform.arch.clone(),
            current_os: platform.os,
            current_arch: platform.arch,
        });
    }
    let (program, command_args) = if args.target.is_empty() {
        (stored.program.clone(), stored.args.clone())
    } else {
        (args.target[0].clone(), args.target[1..].to_vec())
    };
    let timeout = args
        .timeout
        .or_else(|| stored.timeout_ns.map(Duration::from_nanos));
    let samples = run(
        &CommandSpec::new(&program, &command_args),
        &RunConfig {
            warmups: args.warmup.unwrap_or(stored.warmups),
            runs: args.runs.unwrap_or(stored.runs),
            timeout,
        },
    )?;
    let wall_samples = samples
        .iter()
        .map(|sample| sample.wall_ns)
        .collect::<Vec<_>>();
    let wall_ns = aggregate(&wall_samples)?;
    let warnings = variability_warnings(&args.name, &wall_samples)?;
    let cpu_ns = aggregate(
        &samples
            .iter()
            .map(|sample| sample.cpu_ns)
            .collect::<Vec<_>>(),
    )?;
    let peak_memory_bytes = aggregate(
        &samples
            .iter()
            .map(|sample| sample.peak_memory_bytes)
            .collect::<Vec<_>>(),
    )?;
    let wall_budget_pct = args
        .max_time
        .map(|budget| budget.0)
        .or(stored.budgets.wall_percent);
    let cpu_budget_pct = args
        .max_cpu
        .map(|budget| budget.0)
        .or(stored.budgets.cpu_percent);
    let memory_budget_pct = args
        .max_memory
        .map(|budget| budget.0)
        .or(stored.budgets.peak_memory_percent);
    let wall_outcome = compare(
        wall_ns.median,
        stored.wall_ns.median,
        wall_budget_pct,
        stored.noise_floors.wall_ns,
    );
    let cpu_outcome = compare(
        cpu_ns.median,
        stored.cpu_ns.median,
        cpu_budget_pct,
        stored.noise_floors.cpu_ns,
    );
    let memory_outcome = compare(
        peak_memory_bytes.median,
        stored.peak_memory_bytes.median,
        memory_budget_pct,
        stored.noise_floors.peak_memory_bytes,
    );
    let benchmark_status = combined_benchmark_status(&[wall_outcome, cpu_outcome, memory_outcome]);
    let (report_status, exit_class) = if benchmark_status == BenchmarkStatus::Regression {
        (ReportStatus::Regression, ExitClass::Regression)
    } else {
        (ReportStatus::Ok, ExitClass::Success)
    };
    let delta_ns = i128::from(wall_ns.median) - i128::from(stored.wall_ns.median);
    let relative_delta_pct = (stored.wall_ns.median != 0)
        .then(|| (delta_ns as f64 / stored.wall_ns.median as f64) * 100.0);

    emit(
        args.format,
        color,
        &Report {
            schema_version: 1,
            status: report_status,
            benchmarks: vec![ReportBenchmark {
                name: args.name,
                program,
                args: command_args,
                platform,
                baseline_median_ns: stored.wall_ns.median,
                current_median_ns: Some(wall_ns.median),
                delta_ns: Some(delta_ns),
                relative_delta_pct,
                budget_pct: wall_budget_pct,
                absolute_floor_ns: stored.noise_floors.wall_ns,
                sample_count: wall_ns.sample_count,
                status: benchmark_status,
                cpu_time: checked_metric_report(
                    stored.cpu_ns.median,
                    cpu_ns.median,
                    cpu_budget_pct,
                    stored.noise_floors.cpu_ns,
                    MetricUnit::Nanoseconds,
                ),
                peak_memory: checked_metric_report(
                    stored.peak_memory_bytes.median,
                    peak_memory_bytes.median,
                    memory_budget_pct,
                    stored.noise_floors.peak_memory_bytes,
                    MetricUnit::Bytes,
                ),
            }],
            warnings,
            errors: Vec::new(),
        },
    );

    Ok(exit_class)
}

fn list(args: ListArgs, color: bool) -> Result<ExitClass, BenchguardError> {
    let baseline = BaselineStore::load(&args.file)?;
    let benchmarks = baseline
        .benchmarks
        .into_iter()
        .map(|(name, benchmark)| ReportBenchmark {
            name,
            program: benchmark.program,
            args: benchmark.args,
            platform: benchmark.platform,
            baseline_median_ns: benchmark.wall_ns.median,
            current_median_ns: None,
            delta_ns: None,
            relative_delta_pct: None,
            budget_pct: benchmark.budgets.wall_percent,
            absolute_floor_ns: benchmark.noise_floors.wall_ns,
            sample_count: benchmark.wall_ns.sample_count,
            status: BenchmarkStatus::Baseline,
            cpu_time: metric_report(
                benchmark.cpu_ns.median,
                None,
                benchmark.budgets.cpu_percent,
                benchmark.noise_floors.cpu_ns,
                BenchmarkStatus::Baseline,
                MetricUnit::Nanoseconds,
            ),
            peak_memory: metric_report(
                benchmark.peak_memory_bytes.median,
                None,
                benchmark.budgets.peak_memory_percent,
                benchmark.noise_floors.peak_memory_bytes,
                BenchmarkStatus::Baseline,
                MetricUnit::Bytes,
            ),
        })
        .collect();

    emit(
        args.format,
        color,
        &Report {
            schema_version: 1,
            status: ReportStatus::Ok,
            benchmarks,
            warnings: Vec::new(),
            errors: Vec::new(),
        },
    );

    Ok(ExitClass::Success)
}

fn command_from_target(target: &[String]) -> CommandSpec {
    CommandSpec::new(&target[0], &target[1..])
}

fn load_for_record(path: &Path) -> Result<BaselineFileV1, BenchguardError> {
    match BaselineStore::load(path) {
        Ok(baseline) => Ok(baseline),
        Err(BenchguardError::BaselineIo { source, .. })
            if source.kind() == io::ErrorKind::NotFound =>
        {
            Ok(BaselineFileV1 {
                schema_version: 1,
                benchmarks: BTreeMap::new(),
            })
        }
        Err(error) => Err(error),
    }
}

fn metric_aggregate(aggregate: Aggregate) -> MetricAggregateV1 {
    MetricAggregateV1 {
        median: aggregate.median,
        mean: aggregate.mean,
        standard_deviation: aggregate.standard_deviation,
        min: aggregate.min,
        max: aggregate.max,
        p50: aggregate.p50,
        p95: aggregate.p95,
        sample_count: aggregate.sample_count,
    }
}

fn metric_report(
    baseline: u64,
    current: Option<u64>,
    budget_pct: Option<f64>,
    absolute_floor: u64,
    status: BenchmarkStatus,
    unit: MetricUnit,
) -> MetricReport {
    let delta = current.map(|value| i128::from(value) - i128::from(baseline));
    let relative_delta_pct =
        delta.and_then(|value| (baseline != 0).then(|| value as f64 / baseline as f64 * 100.0));
    MetricReport {
        baseline,
        current,
        delta,
        relative_delta_pct,
        budget_pct,
        absolute_floor,
        status,
        unit,
    }
}

fn checked_metric_report(
    baseline: u64,
    current: u64,
    budget_pct: Option<f64>,
    absolute_floor: u64,
    unit: MetricUnit,
) -> MetricReport {
    let outcome = compare(current, baseline, budget_pct, absolute_floor);
    metric_report(
        baseline,
        Some(current),
        budget_pct,
        absolute_floor,
        metric_status(outcome),
        unit,
    )
}

fn metric_status(outcome: MetricOutcome) -> BenchmarkStatus {
    match outcome {
        MetricOutcome::Pass => BenchmarkStatus::Pass,
        MetricOutcome::Regression => BenchmarkStatus::Regression,
        MetricOutcome::Unbudgeted => BenchmarkStatus::Unbudgeted,
    }
}

fn combined_benchmark_status(outcomes: &[MetricOutcome]) -> BenchmarkStatus {
    if outcomes.contains(&MetricOutcome::Regression) {
        BenchmarkStatus::Regression
    } else if outcomes.contains(&MetricOutcome::Pass) {
        BenchmarkStatus::Pass
    } else {
        BenchmarkStatus::Unbudgeted
    }
}

fn duration_ns(duration: Duration) -> Result<u64, BenchguardError> {
    u64::try_from(duration.as_nanos()).map_err(|_| BenchguardError::NumericOverflow)
}

fn current_platform() -> PlatformId {
    PlatformId {
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
    }
}

fn emit(format: OutputFormat, color: bool, report: &Report) {
    let rendered = match format {
        OutputFormat::Human => HumanRenderer::new(color).render(report),
        OutputFormat::Json => JsonRenderer.render(report),
    };
    print!("{rendered}");
}

fn variability_warnings(
    benchmark_name: &str,
    wall_samples: &[u64],
) -> Result<Vec<ReportWarning>, BenchguardError> {
    if !coefficient_of_variation_exceeds_ten_percent(wall_samples)? {
        return Ok(Vec::new());
    }

    let coefficient = coefficient_of_variation_pct(wall_samples)?;
    Ok(vec![ReportWarning {
        code: "high_variability".to_owned(),
        message: format!(
            "{benchmark_name} wall-time coefficient of variation is {coefficient:.2}% \
             (threshold: {WALL_VARIABILITY_WARNING_PCT:.2}%)"
        ),
    }])
}

#[cfg(test)]
mod tests {
    use crate::{
        comparison::MetricOutcome,
        report::{BenchmarkStatus, MetricUnit},
    };

    use super::{checked_metric_report, combined_benchmark_status, variability_warnings};

    // Catches warning on equality, warning suppression above the threshold,
    // or routing the diagnostic anywhere except the stable warning object.
    #[test]
    fn wall_variability_warning_is_strictly_above_ten_percent() {
        assert!(
            variability_warnings("startup", &[90, 110])
                .unwrap()
                .is_empty()
        );

        let warnings = variability_warnings("startup", &[89, 111]).unwrap();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "high_variability");
        assert_eq!(
            warnings[0].message,
            "startup wall-time coefficient of variation is 11.00% (threshold: 10.00%)"
        );
    }

    // Catches using the rounded display percentage for the fixed warning
    // decision. For two values, population SD is half their difference:
    // [9k, 11k] is exactly 10%, while [9k - 1, 11k] is strictly above 10%.
    #[test]
    fn large_wall_variability_warning_keeps_the_exact_ten_percent_boundary() {
        const K: u64 = 1_676_976_733_973_595_591;
        let exact = [9 * K, 11 * K];
        let just_over = [9 * K - 1, 11 * K];

        assert!(variability_warnings("exact", &exact).unwrap().is_empty());
        assert_eq!(
            variability_warnings("just-over", &just_over).unwrap().len(),
            1
        );
        assert!(variability_warnings("zero", &[0, 0]).unwrap().is_empty());
        assert!(matches!(
            variability_warnings("empty", &[]),
            Err(crate::error::BenchguardError::EmptySamples)
        ));
    }

    // Catches applying different threshold rules to CPU/memory reports,
    // treating equality as regression, or fabricating a percentage from a
    // zero baseline.
    #[test]
    fn checked_metric_reports_use_strict_relative_and_floor_thresholds() {
        let relative_equal =
            checked_metric_report(100, 110, Some(10.0), 5, MetricUnit::Nanoseconds);
        assert_eq!(relative_equal.status, BenchmarkStatus::Pass);

        let floor_equal = checked_metric_report(100, 105, Some(1.0), 5, MetricUnit::Nanoseconds);
        assert_eq!(floor_equal.status, BenchmarkStatus::Pass);

        let regression = checked_metric_report(100, 111, Some(10.0), 5, MetricUnit::Nanoseconds);
        assert_eq!(regression.status, BenchmarkStatus::Regression);

        let zero_baseline = checked_metric_report(0, 2, Some(0.0), 1, MetricUnit::Bytes);
        assert_eq!(zero_baseline.status, BenchmarkStatus::Regression);
        assert_eq!(zero_baseline.delta, Some(2));
        assert_eq!(zero_baseline.relative_delta_pct, None);
    }

    // Catches last-metric-wins aggregation or classifying a mixed
    // pass/unbudgeted result as wholly unbudgeted.
    #[test]
    fn combined_status_regresses_when_any_configured_metric_regresses() {
        assert_eq!(
            combined_benchmark_status(&[
                MetricOutcome::Pass,
                MetricOutcome::Unbudgeted,
                MetricOutcome::Regression,
            ]),
            BenchmarkStatus::Regression
        );
        assert_eq!(
            combined_benchmark_status(&[
                MetricOutcome::Unbudgeted,
                MetricOutcome::Pass,
                MetricOutcome::Unbudgeted,
            ]),
            BenchmarkStatus::Pass
        );
        assert_eq!(
            combined_benchmark_status(&[
                MetricOutcome::Unbudgeted,
                MetricOutcome::Unbudgeted,
                MetricOutcome::Unbudgeted,
            ]),
            BenchmarkStatus::Unbudgeted
        );
    }
}
