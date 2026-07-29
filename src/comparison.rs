#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricOutcome {
    Pass,
    Regression,
    Unbudgeted,
}

pub fn compare(
    current: u64,
    baseline: u64,
    relative_budget: Option<f64>,
    absolute_floor: u64,
) -> MetricOutcome {
    let Some(limit) = relative_budget else {
        return MetricOutcome::Unbudgeted;
    };

    let absolute_delta = current.saturating_sub(baseline);
    let relative_delta_pct = if baseline == 0 {
        if current == 0 { 0.0 } else { f64::INFINITY }
    } else {
        (absolute_delta as f64 / baseline as f64) * 100.0
    };
    let regressed = relative_delta_pct > limit && absolute_delta > absolute_floor;

    if regressed {
        MetricOutcome::Regression
    } else {
        MetricOutcome::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::{MetricOutcome, compare};

    // Catches using OR instead of AND for the relative and absolute thresholds,
    // treating equality as a regression, or enforcing a budget that was not set.
    #[test]
    fn regression_requires_relative_and_absolute_limits() {
        assert_eq!(compare(111, 100, Some(10.0), 5), MetricOutcome::Regression);
        assert_eq!(compare(104, 100, Some(1.0), 5), MetricOutcome::Pass);
        assert_eq!(compare(200, 100, None, 5), MetricOutcome::Unbudgeted);
    }

    // Catches inclusive threshold comparisons. A result must exceed both
    // limits, not merely equal either one.
    #[test]
    fn equality_at_either_threshold_passes() {
        assert_eq!(compare(110, 100, Some(10.0), 5), MetricOutcome::Pass);
        assert_eq!(compare(105, 100, Some(1.0), 5), MetricOutcome::Pass);
    }

    // Catches division-by-zero handling that classifies an unchanged zero
    // baseline as a regression or misses a real increase from zero.
    #[test]
    fn zero_baselines_have_defined_threshold_behavior() {
        assert_eq!(compare(0, 0, Some(0.0), 1), MetricOutcome::Pass);
        assert_eq!(compare(2, 0, Some(0.0), 1), MetricOutcome::Regression);
    }
}
