use serde::{Deserialize, Serialize};

use crate::{Error, error::positive};

/// Absolute pressure or a gauge reading with an explicitly specified absolute reference.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "reference", rename_all = "snake_case", deny_unknown_fields)]
pub enum Pressure {
    /// Pressure measured from vacuum; selected with `"reference": "absolute"` in JSON.
    Absolute {
        /// Absolute pressure in Pa. Must be positive and finite.
        value_pa: f64,
    },
    /// Reading relative to a stated datum; selected with `"reference": "gauge"` in JSON.
    Gauge {
        /// Gauge reading in Pa relative to `reference_absolute_pa`. It may be zero or negative
        /// provided the absolute pressure `reference_absolute_pa + value_pa` is positive and finite.
        value_pa: f64,
        /// Absolute pressure of the gauge datum in Pa. Must be positive and finite.
        reference_absolute_pa: f64,
    },
}

impl Pressure {
    fn parts(self) -> (f64, f64) {
        match self {
            Self::Absolute { value_pa } => (0.0, value_pa),
            Self::Gauge {
                value_pa,
                reference_absolute_pa,
            } => (reference_absolute_pa, value_pa),
        }
    }

    /// Returns the absolute pressure in Pa: the value itself, or the gauge datum plus the reading.
    pub fn absolute_pa(self) -> f64 {
        let (reference, value) = self.parts();
        reference + value
    }

    /// Returns `self` minus `other` in Pa.
    ///
    /// When both pressures have the same datum (both absolute, or equal gauge references) the
    /// readings are subtracted directly. Otherwise each datum-plus-reading sum is formed together
    /// with its rounding error and the two errors enter the difference, so small differences
    /// between readings are preserved even when the absolute pressures round to the same binary64
    /// value.
    pub fn difference_pa(self, other: Self) -> f64 {
        let (reference, value) = self.parts();
        let (other_reference, other_value) = other.parts();
        if reference == other_reference {
            return value - other_value;
        }
        let (a, a_error) = sum_with_roundoff(reference, value);
        let (b, b_error) = sum_with_roundoff(other_reference, other_value);
        (a - b) + (a_error - b_error)
    }

    /// Checks that the pressure is physically admissible.
    ///
    /// A gauge datum must be positive and finite, reported at `{path}.reference_absolute_pa`. For
    /// both variants the absolute pressure must be positive and finite, reported at
    /// `{path}.value_pa`. `path` is the JSON path of this object and failures are
    /// [`Error::InvalidModel`].
    pub fn validate(self, path: &str) -> Result<(), Error> {
        if let Self::Gauge {
            reference_absolute_pa,
            ..
        } = self
        {
            positive(
                &format!("{path}.reference_absolute_pa"),
                reference_absolute_pa,
            )?;
        }
        positive(&format!("{path}.value_pa"), self.absolute_pa())
    }
}

// FastTwoSum recovers the part of a gauge reading lost when adding its datum.
// The subtraction must use the operand with the larger magnitude.
fn sum_with_roundoff(a: f64, b: f64) -> (f64, f64) {
    let (large, small) = if a.abs() >= b.abs() { (a, b) } else { (b, a) };
    let sum = large + small;
    (sum, small - (sum - large))
}

#[cfg(test)]
mod tests {
    use super::Pressure;

    #[test]
    fn different_gauge_datums_retain_small_residuals_in_either_order() {
        let a = Pressure::Gauge {
            value_pa: 1e-12,
            reference_absolute_pa: 101325.0,
        };
        let b = Pressure::Gauge {
            value_pa: -1.0,
            reference_absolute_pa: 101326.0,
        };
        assert_eq!(a.difference_pa(b), 1e-12);
        assert_eq!(b.difference_pa(a), -1e-12);
    }
}
