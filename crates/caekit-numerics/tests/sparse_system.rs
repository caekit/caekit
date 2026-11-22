//! Sparse systems: verified residuals, elimination of prescribed unknowns, the distinction
//! between an iteration limit and a numerical breakdown, and inputs that must never report
//! convergence.

use caekit_numerics::{ConstrainedSystem, LinearSettings, SparseAssembly, StopReason, norm};

fn two_variable_system() -> SparseAssembly {
    let mut system = SparseAssembly::new(2);
    system.add(0, 0, 3.0);
    system.add(0, 0, 1.0);
    system.add(0, 1, 1.0);
    system.add(1, 0, 1.0);
    system.add(1, 1, 3.0);
    system
}

#[test]
fn solves_a_sparse_spd_system_with_a_verified_residual() {
    let (x, trace) =
        two_variable_system().solve(&[1.0, 2.0], LinearSettings::default(), "test", "1");
    assert_eq!(trace.stop_reason, StopReason::Converged);
    // Conjugate gradient terminates in at most n = 2 iterations for a 2 x 2 system, so the error
    // is rounding only: the condition number is (7 + sqrt 5)/(7 - sqrt 5) = 1.94 and the measured
    // errors of the two entries are 2.8e-17 and 0 (probe solving this system). 1e-12 leaves four
    // orders of margin and sits two below the 1e-10 default residual tolerance, which would
    // allow an error of up to 9.4e-11 if the iteration stopped early.
    assert!((x[0] - 1.0 / 11.0).abs() < 1.0e-12);
    assert!((x[1] - 7.0 / 11.0).abs() < 1.0e-12);
    let residual = [1.0 - (4.0 * x[0] + x[1]), 2.0 - (x[0] + 3.0 * x[1])];
    let final_norm = trace.final_true_residual_l2.unwrap();
    assert_eq!(final_norm, norm(&residual));
    assert!(final_norm <= trace.residual_threshold.unwrap());
}

#[test]
fn reports_a_positive_zero_threshold_for_a_zero_right_hand_side() {
    let settings = LinearSettings {
        absolute_residual_tolerance: -0.0,
        ..LinearSettings::default()
    };
    let (_, trace) = two_variable_system().solve(&[0.0, -0.0], settings, "test", "1");
    assert_eq!(trace.stop_reason, StopReason::Converged);
    assert_eq!(trace.residual_threshold.map(f64::to_bits), Some(0));
    assert_eq!(trace.final_true_residual_l2.map(f64::to_bits), Some(0));
}

#[test]
fn eliminates_prescribed_degrees_of_freedom_and_recovers_the_full_vector() {
    let mut system = ConstrainedSystem::new(vec![Some(2.0), None, Some(-4.0)], &[0.0, 3.0, 0.0]);
    for (i, row) in [[2.0, -1.0, 0.0], [-1.0, 2.0, -1.0], [0.0, -1.0, 2.0]]
        .into_iter()
        .enumerate()
    {
        for (j, value) in row.into_iter().enumerate() {
            system.add(i, j, value);
        }
    }
    let (x, trace) = system.solve(LinearSettings::default(), "test", "1");
    assert_eq!(trace.stop_reason, StopReason::Converged);
    assert_eq!(x, [2.0, 0.5, -4.0]);
}

#[test]
fn unattainable_true_residual_stops_after_repeated_roundoff_limited_corrections() {
    // The two adjacent binary64 values bracketing 29/7 both give a nonzero residual.
    let mut system = SparseAssembly::new(1);
    system.add(0, 0, 7.0);
    let settings = LinearSettings {
        // Below the unit roundoff of binary64 (1.1e-16), so no iterate can meet it: the residual
        // of the nearest representable x is at least 3.6e-15 against a threshold of 2.9e-19.
        relative_residual_tolerance: 1e-20,
        ..Default::default()
    };
    let (x, trace) = system.solve(&[29.0], settings, "test", "W");
    assert_eq!(trace.stop_reason, StopReason::NumericalBreakdown);
    assert!(trace.iterations < settings.max_iterations);
    assert_eq!(
        trace.final_true_residual_l2,
        Some((29.0 - 7.0 * x[0]).abs())
    );
    assert!(trace.final_true_residual_l2.unwrap() > trace.residual_threshold.unwrap());
    assert!(trace.diagnostic.unwrap().contains("stagnated"));
}

#[test]
fn distinguishes_an_iteration_limit_from_a_numerical_breakdown() {
    let one_iteration = LinearSettings {
        max_iterations: 1,
        ..LinearSettings::default()
    };
    let (_, limited) = two_variable_system().solve(&[1.0, 2.0], one_iteration, "test", "1");
    assert_eq!(limited.stop_reason, StopReason::IterationLimit);
    assert_eq!(limited.iterations, 1);
    assert!(limited.final_true_residual_l2.unwrap() > limited.residual_threshold.unwrap());

    let mut singular = SparseAssembly::new(1);
    singular.add(0, 0, 0.0);
    let (_, invalid) = singular.solve(&[1.0], LinearSettings::default(), "test", "1");
    assert_eq!(invalid.stop_reason, StopReason::NumericalBreakdown);
    assert!(invalid.final_true_residual_l2.is_none());
}

#[test]
fn rejects_an_indefinite_search_direction_with_a_verified_residual() {
    let mut system = SparseAssembly::new(2);
    for (row, entries) in [[1.0, 2.0], [2.0, 1.0]].into_iter().enumerate() {
        for (column, value) in entries.into_iter().enumerate() {
            system.add(row, column, value);
        }
    }
    // The Jacobi diagonal is one and the RHS is in the eigenspace with
    // eigenvalue -1, so the first search direction gives p^T A p = -2.
    let (x, trace) = system.solve(&[1.0, -1.0], LinearSettings::default(), "test", "1");
    assert_eq!(x, [0.0, 0.0]);
    assert_eq!(trace.iterations, 0);
    assert_eq!(trace.stop_reason, StopReason::NumericalBreakdown);
    assert_eq!(trace.final_true_residual_l2, Some(2.0_f64.sqrt()));
    assert!(trace.final_true_residual_l2.unwrap() > trace.residual_threshold.unwrap());
}

#[test]
fn inconsistent_dimensions_and_indices_never_report_convergence() {
    for rhs in [&[][..], &[0.0][..], &[0.0; 3][..]] {
        let (_, trace) = two_variable_system().solve(rhs, LinearSettings::default(), "test", "1");
        assert_eq!(trace.stop_reason, StopReason::NumericalBreakdown);
    }
    for (row, column) in [(0, 2), (2, 0)] {
        let mut system = two_variable_system();
        system.add(row, column, 1.0);
        let (_, trace) = system.solve(&[0.0; 2], LinearSettings::default(), "test", "1");
        assert_eq!(trace.stop_reason, StopReason::NumericalBreakdown);
    }
    for loads in [&[][..], &[0.0; 3][..]] {
        let system = ConstrainedSystem::new(vec![Some(1.0), None], loads);
        let (_, trace) = system.solve(LinearSettings::default(), "test", "1");
        assert_eq!(trace.stop_reason, StopReason::NumericalBreakdown);
    }
    let mut system = ConstrainedSystem::new(vec![Some(0.0)], &[0.0]);
    system.add(0, 1, 1.0);
    let (_, trace) = system.solve(LinearSettings::default(), "test", "1");
    assert_eq!(trace.stop_reason, StopReason::NumericalBreakdown);
}

#[test]
fn fully_prescribed_systems_still_check_for_nonfinite_input() {
    for (displacement, load, stiffness) in [
        (f64::NAN, 0.0, 1.0),
        (0.0, f64::INFINITY, 1.0),
        (0.0, 0.0, f64::NAN),
    ] {
        let mut system = ConstrainedSystem::new(vec![Some(displacement)], &[load]);
        system.add(0, 0, stiffness);
        let (_, trace) = system.solve(LinearSettings::default(), "test", "1");
        assert_eq!(trace.stop_reason, StopReason::NumericalBreakdown);
    }
}

#[test]
fn invalid_stopping_settings_cannot_accept_the_zero_iterate() {
    for settings in [
        LinearSettings {
            // The open upper end of the accepted interval (0, 1); validation rejects it.
            relative_residual_tolerance: 1.0,
            ..Default::default()
        },
        LinearSettings {
            absolute_residual_tolerance: f64::INFINITY,
            ..Default::default()
        },
        LinearSettings {
            relative_residual_tolerance: f64::NAN,
            ..Default::default()
        },
        LinearSettings {
            max_iterations: 0,
            ..Default::default()
        },
    ] {
        let (_, trace) = two_variable_system().solve(&[0.0, 0.0], settings, "test", "1");
        assert_eq!(trace.stop_reason, StopReason::NumericalBreakdown);
    }
}
