use caekit::{Error, Output, Quantity, RunReport};

#[allow(dead_code)]
pub fn invalid_at(error: Error, expected_path: &str, expected_message: &str) {
    let Error::InvalidModel { path, message } = error else {
        panic!("expected an invalid model at {expected_path}, got {error}");
    };
    assert_eq!(path, expected_path);
    assert!(message.contains(expected_message), "{path}: {message}");
}

/// Tolerance of a comparison that must hold bit for bit. A site that uses it states, next to the
/// call, why no rounding can enter.
#[allow(dead_code)]
pub const EXACT: f64 = 0.0;

/// Asserts `|actual - expected| <= tolerance`.
///
/// Every call passes a named constant whose documentation gives the reason of the value, as
/// AGENTS.md section 4 requires; a constant with a measured margin quotes the largest difference
/// that the sites using it produced. To measure it, print `(actual - expected).abs()` here
/// temporarily and run the test with `-- --nocapture --test-threads=1`.
pub fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual:.16e}, expected {expected:.16e}, tolerance {tolerance:.3e}"
    );
}

#[allow(dead_code)]
pub fn scalar(report: &RunReport, output: Output) -> f64 {
    match &report.quantities[&output] {
        Quantity::Scalar { value, .. } => *value,
        quantity => panic!("expected a scalar, got {quantity:?}"),
    }
}

#[allow(dead_code)]
pub fn region(report: &RunReport, output: Output, region: &str) -> f64 {
    match &report.quantities[&output] {
        Quantity::ByRegion { values, .. } => values[region],
        quantity => panic!("expected region values, got {quantity:?}"),
    }
}
