use std::{collections::BTreeMap, hint::black_box};

use benchguard::{
    baseline::schema::{BaselineFileV1, BenchmarkV1, BudgetsV1, MetricAggregateV1, NoiseFloorsV1},
    domain::PlatformId,
    stats,
};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

fn benchmark_aggregation(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("aggregate");
    for sample_count in [10_usize, 100, 10_000] {
        let samples = (0..sample_count)
            .map(|index| u64::try_from(index % 1_000).unwrap() + 1)
            .collect::<Vec<_>>();
        group.bench_with_input(
            BenchmarkId::from_parameter(sample_count),
            &samples,
            |bencher, samples| {
                bencher.iter(|| stats::aggregate(black_box(samples)).unwrap());
            },
        );
    }
    group.finish();
}

fn benchmark_serialization(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("serialize_baseline");
    for entry_count in [1_usize, 100] {
        let baseline = baseline_with_entries(entry_count);
        group.bench_with_input(
            BenchmarkId::from_parameter(entry_count),
            &baseline,
            |bencher, baseline| {
                bencher.iter(|| serde_json::to_vec(black_box(baseline)).unwrap());
            },
        );
    }
    group.finish();
}

fn baseline_with_entries(entry_count: usize) -> BaselineFileV1 {
    let benchmarks = (0..entry_count)
        .map(|index| (format!("benchmark-{index}"), benchmark()))
        .collect::<BTreeMap<_, _>>();
    BaselineFileV1 {
        schema_version: 1,
        benchmarks,
    }
}

fn benchmark() -> BenchmarkV1 {
    BenchmarkV1 {
        program: "benchguard-fixture".to_owned(),
        args: vec!["sleep-ms".to_owned(), "10".to_owned()],
        recorded_at: "2026-07-28T00:00:00Z".to_owned(),
        platform: PlatformId {
            os: "linux".to_owned(),
            arch: "x86_64".to_owned(),
        },
        benchguard_version: "0.1.0".to_owned(),
        warmups: 2,
        runs: 10,
        timeout_ns: Some(1_000_000_000),
        wall_ns: metric(),
        cpu_ns: metric(),
        peak_memory_bytes: metric(),
        budgets: BudgetsV1 {
            wall_percent: Some(10.0),
            cpu_percent: Some(10.0),
            peak_memory_percent: Some(10.0),
        },
        noise_floors: NoiseFloorsV1 {
            wall_ns: 1_000_000,
            cpu_ns: 1_000_000,
            peak_memory_bytes: 1_048_576,
        },
    }
}

fn metric() -> MetricAggregateV1 {
    MetricAggregateV1 {
        median: 10_000_000,
        mean: 10_100_000,
        standard_deviation: 100_000,
        min: 9_900_000,
        max: 10_300_000,
        p50: 10_000_000,
        p95: 10_300_000,
        sample_count: 10,
    }
}

criterion_group!(core, benchmark_aggregation, benchmark_serialization);
criterion_main!(core);
