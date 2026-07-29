use crate::{domain::Aggregate, error::BenchguardError};

pub fn aggregate(samples: &[u64]) -> Result<Aggregate, BenchguardError> {
    if samples.is_empty() {
        return Err(BenchguardError::EmptySamples);
    }

    let sample_count =
        u32::try_from(samples.len()).map_err(|_| BenchguardError::NumericOverflow)?;
    let count = u128::try_from(samples.len()).map_err(|_| BenchguardError::NumericOverflow)?;
    let sum = samples.iter().try_fold(0_u128, |sum, &sample| {
        sum.checked_add(u128::from(sample))
            .ok_or(BenchguardError::NumericOverflow)
    })?;
    let mean_u128 = sum / count;
    let mean = u64::try_from(mean_u128).map_err(|_| BenchguardError::NumericOverflow)?;

    let squared_scaled_deviations = samples.iter().try_fold(0_u128, |accumulator, &sample| {
        let scaled_sample = u128::from(sample)
            .checked_mul(count)
            .ok_or(BenchguardError::NumericOverflow)?;
        let deviation = scaled_sample.abs_diff(sum);
        let squared_deviation = deviation
            .checked_mul(deviation)
            .ok_or(BenchguardError::NumericOverflow)?;
        accumulator
            .checked_add(squared_deviation)
            .ok_or(BenchguardError::NumericOverflow)
    })?;
    let variance = squared_scaled_deviations / count / count / count;
    let standard_deviation = integer_square_root(variance);

    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    let median = if sorted.len() % 2 == 0 {
        let lower = u128::from(sorted[middle - 1]);
        let upper = u128::from(sorted[middle]);
        u64::try_from((lower + upper) / 2).map_err(|_| BenchguardError::NumericOverflow)?
    } else {
        sorted[middle]
    };

    Ok(Aggregate {
        median,
        mean,
        standard_deviation,
        min: sorted[0],
        max: sorted[sorted.len() - 1],
        p50: nearest_rank(&sorted, 50),
        p95: nearest_rank(&sorted, 95),
        sample_count,
    })
}

pub fn coefficient_of_variation_pct(samples: &[u64]) -> Result<f64, BenchguardError> {
    let moments = exact_cv_moments(samples)?;
    if moments.sum.is_zero() {
        return Ok(0.0);
    }

    Ok(moments.dispersion.to_f64().sqrt() / moments.sum.to_f64() * 100.0)
}

pub(crate) fn coefficient_of_variation_exceeds_ten_percent(
    samples: &[u64],
) -> Result<bool, BenchguardError> {
    let moments = exact_cv_moments(samples)?;
    if moments.sum.is_zero() {
        return Ok(false);
    }

    Ok(moments.dispersion.checked_mul(WideUint::from_u64(100))? > moments.sum_squared)
}

fn exact_cv_moments(samples: &[u64]) -> Result<ExactCvMoments, BenchguardError> {
    if samples.is_empty() {
        return Err(BenchguardError::EmptySamples);
    }

    let count = u64::try_from(samples.len()).map_err(|_| BenchguardError::NumericOverflow)?;
    let mut sum = WideUint::ZERO;
    let mut sum_of_squares = WideUint::ZERO;
    for &sample in samples {
        sum = sum.checked_add(WideUint::from_u64(sample))?;
        let square = u128::from(sample) * u128::from(sample);
        sum_of_squares = sum_of_squares.checked_add(WideUint::from_u128(square))?;
    }

    let sum_squared = sum.checked_mul(sum)?;
    let dispersion = sum_of_squares
        .checked_mul(WideUint::from_u64(count))?
        .checked_sub(sum_squared)?;
    Ok(ExactCvMoments {
        sum,
        sum_squared,
        dispersion,
    })
}

struct ExactCvMoments {
    sum: WideUint,
    sum_squared: WideUint,
    dispersion: WideUint,
}

// Five 64-bit limbs cover the largest exact CV comparison on supported
// 64-bit targets: 100 * n * sum(x^2) is strictly below 2^263.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WideUint([u64; 5]);

impl WideUint {
    const ZERO: Self = Self([0; 5]);

    fn from_u64(value: u64) -> Self {
        Self([value, 0, 0, 0, 0])
    }

    fn from_u128(value: u128) -> Self {
        Self([value as u64, (value >> 64) as u64, 0, 0, 0])
    }

    fn is_zero(self) -> bool {
        self == Self::ZERO
    }

    fn checked_add(self, other: Self) -> Result<Self, BenchguardError> {
        let mut result = [0; 5];
        let mut carry = 0_u128;
        for (index, output) in result.iter_mut().enumerate() {
            let total = u128::from(self.0[index]) + u128::from(other.0[index]) + carry;
            *output = total as u64;
            carry = total >> 64;
        }
        if carry == 0 {
            Ok(Self(result))
        } else {
            Err(BenchguardError::NumericOverflow)
        }
    }

    fn checked_sub(self, other: Self) -> Result<Self, BenchguardError> {
        let mut result = [0; 5];
        let mut borrow = false;
        for (index, output) in result.iter_mut().enumerate() {
            let (difference, first_borrow) = self.0[index].overflowing_sub(other.0[index]);
            let (difference, second_borrow) = difference.overflowing_sub(u64::from(borrow));
            *output = difference;
            borrow = first_borrow || second_borrow;
        }
        if borrow {
            Err(BenchguardError::NumericOverflow)
        } else {
            Ok(Self(result))
        }
    }

    fn checked_mul(self, other: Self) -> Result<Self, BenchguardError> {
        let mut result = [0_u64; 5];
        for left_index in 0..5 {
            if self.0[left_index] != 0 && other.0[(5 - left_index)..].iter().any(|&limb| limb != 0)
            {
                return Err(BenchguardError::NumericOverflow);
            }

            let mut carry = 0_u128;
            for right_index in 0..(5 - left_index) {
                let output_index = left_index + right_index;
                let total = u128::from(self.0[left_index]) * u128::from(other.0[right_index])
                    + u128::from(result[output_index])
                    + carry;
                result[output_index] = total as u64;
                carry = total >> 64;
            }
            if carry != 0 {
                return Err(BenchguardError::NumericOverflow);
            }
        }
        Ok(Self(result))
    }

    fn to_f64(self) -> f64 {
        const LIMB_SCALE: f64 = 18_446_744_073_709_551_616.0;
        self.0
            .iter()
            .rev()
            .fold(0.0, |value, &limb| value * LIMB_SCALE + limb as f64)
    }
}

impl Ord for WideUint {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.iter().rev().cmp(other.0.iter().rev())
    }
}

impl PartialOrd for WideUint {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn nearest_rank(sorted: &[u64], percentile: u128) -> u64 {
    let count = u128::try_from(sorted.len()).expect("slice length always fits in u128");
    let rank = (percentile * count).div_ceil(100);
    let index = usize::try_from(rank - 1).expect("rank is derived from the slice length");
    sorted[index]
}

fn integer_square_root(value: u128) -> u64 {
    let mut lower = 0_u128;
    let mut upper = u128::from(u64::MAX);

    while lower < upper {
        let middle = lower + (upper - lower).div_ceil(2);
        if middle <= value / middle {
            lower = middle;
        } else {
            upper = middle - 1;
        }
    }

    u64::try_from(lower).expect("square root is bounded by u64::MAX")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::BenchguardError;

    // Catches an implementation that skips sorting, uses an averaged p50,
    // chooses the wrong nearest-rank p95, or uses sample rather than
    // population standard deviation. For [10, 20, 30, 40], the population
    // variance is (225 + 25 + 25 + 225) / 4 = 125, so floor(sqrt(125)) = 11.
    #[test]
    fn aggregates_unsorted_integer_samples() {
        let result = aggregate(&[30, 10, 20, 40]).unwrap();
        assert_eq!(result.median, 25);
        assert_eq!(result.mean, 25);
        assert_eq!(result.standard_deviation, 11);
        assert_eq!(result.min, 10);
        assert_eq!(result.max, 40);
        assert_eq!(result.p50, 20);
        assert_eq!(result.p95, 40);
        assert_eq!(result.sample_count, 4);
    }

    // Catches an implementation that centers population variance on the
    // truncated aggregate mean. The exact mean is 2/3 and the population
    // variance is ((-2/3)^2 + (-2/3)^2 + (4/3)^2) / 3 = 8/9, so the
    // required integer standard deviation is floor(sqrt(8/9)) = 0.
    #[test]
    fn uses_the_true_population_mean_for_non_integral_means() {
        let result = aggregate(&[0, 0, 2]).unwrap();

        assert_eq!(result.mean, 0);
        assert_eq!(result.standard_deviation, 0);
    }

    // Catches an implementation that returns a fabricated zero-valued
    // aggregate instead of reporting that the statistic is undefined.
    #[test]
    fn rejects_empty_samples() {
        assert!(matches!(aggregate(&[]), Err(BenchguardError::EmptySamples)));
    }

    // Catches integer-mean truncation, sample-standard-deviation division,
    // and an inclusive variability threshold. [90, 110] has arithmetic mean
    // 100 and population standard deviation 10; [89, 111] has deviation 11.
    #[test]
    fn coefficient_of_variation_uses_true_population_math_at_ten_percent() {
        assert_eq!(coefficient_of_variation_pct(&[90, 110]).unwrap(), 10.0);
        assert!(coefficient_of_variation_pct(&[89, 111]).unwrap() > 10.0);
    }

    // Catches division by zero producing NaN and empty input being silently
    // treated as a stable benchmark.
    #[test]
    fn coefficient_of_variation_handles_zero_mean_and_empty_samples() {
        assert_eq!(coefficient_of_variation_pct(&[0, 0, 0]).unwrap(), 0.0);
        assert!(matches!(
            coefficient_of_variation_pct(&[]),
            Err(BenchguardError::EmptySamples)
        ));
    }

    // Catches converting samples to f64 before measuring their spread. These
    // adjacent integers collapse to the same f64 near u64::MAX, but their
    // mathematical coefficient of variation is positive.
    #[test]
    fn coefficient_of_variation_preserves_adjacent_large_integer_distinctions() {
        let coefficient = coefficient_of_variation_pct(&[u64::MAX - 1, u64::MAX]).unwrap();

        assert!(
            coefficient > 0.0,
            "adjacent integer samples became identical"
        );
    }

    // Catches checked multiplication discarding a nonzero partial product
    // above the fifth limb instead of reporting numeric overflow.
    #[test]
    fn wide_multiplication_rejects_discarded_high_limb_products() {
        let highest_limb = WideUint([0, 0, 0, 0, 1]);
        let one_limb_shift = WideUint([0, 1, 0, 0, 0]);

        assert!(matches!(
            highest_limb.checked_mul(one_limb_shift),
            Err(BenchguardError::NumericOverflow)
        ));
    }
}
