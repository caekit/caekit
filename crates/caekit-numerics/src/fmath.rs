//! Portable binary64 operations for numerical decisions.

/// Computes the Euclidean norm with a scaled sum of squares in iterator order.
///
/// Returns positive zero for an empty or all-zero input. NaN propagates and
/// infinite inputs give positive infinity in the absence of NaN. Scaling
/// avoids overflow and underflow from squaring the original components.
pub fn norm2(values: impl Iterator<Item = f64> + Clone) -> f64 {
    let mut scale = 0.0_f64;
    for value in values.clone() {
        if value.is_nan() {
            return value;
        }
        scale = scale.max(value.abs());
    }
    if scale == 0.0 || scale.is_infinite() {
        return scale;
    }
    let sum: f64 = values
        .map(|value| {
            let scaled = value / scale;
            scaled * scaled
        })
        .sum();
    scale * sum.sqrt()
}

#[cfg(test)]
mod tests {
    use super::norm2;

    #[test]
    fn scales_before_squaring() {
        assert_eq!(norm2([3.0, 4.0].into_iter()), 5.0);
        let tiny = f64::from_bits(1);
        assert_eq!(
            norm2([tiny, tiny * 2.0, tiny * 2.0].into_iter()),
            tiny * 3.0
        );
        assert_eq!(norm2([f64::MAX].into_iter()), f64::MAX);
    }

    #[test]
    fn handles_zero_and_non_finite_values() {
        assert_eq!(norm2(std::iter::empty()).to_bits(), 0);
        assert_eq!(norm2([-0.0, 0.0].into_iter()).to_bits(), 0);
        assert_eq!(norm2([1.0, f64::NEG_INFINITY].into_iter()), f64::INFINITY);
        assert!(norm2([f64::INFINITY, f64::NAN].into_iter()).is_nan());
    }
}
