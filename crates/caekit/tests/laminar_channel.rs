//! Laminar channel: the analytical pressure loss and balances, second-order refinement, both
//! driving modes, mixed pressure references, and rejected assignments.

// Reference values in tests are independent of the library decision and result paths.
#![allow(clippy::disallowed_methods)]

mod common;

use caekit::physics::laminar_channel::{FlowDrive, LaminarChannel, Pressure, PressureAtPort};
use caekit::{LinearSettings, Model, Output, Physics, Quantity, StopReason, solve};
use caekit_numerics::verify::{
    AlgebraicCheck, BandCheck, TOLERANCE_DIVISOR, VerifyError, algebraic_check, band_check,
};
use common::{EXACT, close, invalid_at, region, scalar};

const INPUT: &[u8] = include_bytes!("../../../examples/laminar_channel.json");

// Allowances of the checks on the fixture mesh of 32 uniform cells and on the 64-cell mesh of the
// pressure-driven case. "Measured" is the largest |actual - expected| over the sites that use the
// constant (print it in `common::close` and run `cargo test -p caekit --test laminar_channel --
// --nocapture --test-threads=1`). Several allowances are the discretization error that the
// scheme has by construction on a uniform mesh of `n` cells (docs/physics/laminar-channel.md,
// "why the order is 2"): at a prescribed flow `Q` the loss is `Δp / (1 + 2 / n^2)`, so the loss
// error is `2 Δp / (n^2 + 2)` and, at a prescribed loss, the flow error is `2 Q / n^2`.

/// Pressure loss of the fixture in Pa. The error of 32 cells is `2 Δp / (n^2 + 2)` = 4.678e-3 Pa
/// for `Δp` = 2.4 Pa; 0.005 is 7% above it. Measured 4.68e-3 Pa.
const LOSS_TOL_PA: f64 = 0.005;
/// Volume flow rate of the fixture in m^3/s: the solve scales the velocity so that the flow is the
/// prescribed 1e-5 m^3/s, so only rounding remains; 1e-13 relative. Measured 1.7e-21, 1 ulp.
const FLOW_TOL_M3_S: f64 = 1.0e-18;
/// Mass flow rate in kg/s, density times the flow above: 1e-13 relative to 0.01 kg/s. Measured
/// 1.7e-18, 1 ulp.
const MASS_FLOW_TOL_KG_S: f64 = 1.0e-15;
/// Channel Reynolds number, a product and quotient of exact inputs and the flow: 5e-13 relative to
/// 200. Measured 2.8e-14, 1 ulp.
const REYNOLDS_TOL: f64 = 1.0e-10;
/// Mass balance in kg/s: inflow and outflow are the same product, 1e-13 relative to 0.01 kg/s.
/// Measured 0.
const MASS_BALANCE_TOL_KG_S: f64 = 1.0e-15;
/// Axial momentum balance in N. `balance_bounds.rs` bounds it for this model by 7.6e-17 N plus a
/// rounding allowance of 3.5e-17 N (its `--nocapture` output); 1e-12 N is 9e3 times that and
/// 4e-10 of the 2.4e-3 N pressure force. Measured 2.6e-18 N.
const MOMENTUM_BALANCE_TOL_N: f64 = 1.0e-12;
/// Relative allowance for the uniform-mesh pressure loss errors against their derived value. It
/// sits above the measured relative differences, at most 1.6e-12 (8 to 64 cells, from the
/// printed table), by 600. An error is the difference of two numbers near `Δp` = 2.4 Pa, so
/// a few ulp of 2.4 (4.4e-16 Pa each) show up relative to an error of 1.2e-3 Pa (64 cells) as
/// the measured 1.6e-12.
const DERIVED_ERROR_RELATIVE_TOL: f64 = 1.0e-9;
/// Band of the ratio of successive pressure loss errors on uniform meshes of 8, 16, 32 and 64
/// cells. The derived errors give `(4 n^2 + 2) / (n^2 + 2)` = 3.909, 3.977 and 3.994; the band
/// holds all three, with 0.1 below the coarsest and 0.1 above 4, and excludes first-order ratios
/// near 2 and no-decrease ratios near 1.
const UNIFORM_RATIO_BAND: std::ops::Range<f64> = 3.8..4.1;
/// Pressure loss of the pressure-driven case in Pa. The loss is the difference of two gauge
/// readings with the same reference, subtracted directly (`Pressure::difference_pa`), so
/// only the rounding of 2.4 Pa remains: 4e-13 relative. Measured 0.
const PRESSURE_DRIVE_LOSS_TOL_PA: f64 = 1.0e-12;
/// Volume flow rate of the pressure-driven case on 64 cells in m^3/s: the derived error
/// `2 Q / n^2` = 4.883e-9 m^3/s of `Q` = 1e-5 m^3/s, 2% above. Measured 4.88e-9.
const PRESSURE_DRIVE_FLOW_TOL_M3_S: f64 = 5.0e-9;
/// Velocity against the parabola on 64 cells in m/s. The wall cells leave a uniform offset
/// `g h^2 / (8 μ)` = 3.66e-6 m/s with `g` = 1.2 Pa/m, `h` = 0.01/64 m, `μ` = 1e-3 Pa s (derived in
/// the comment of the uniform refinement test); 4e-6 is 9% above. Measured 3.66e-6 m/s.
const PARABOLA_TOL_M_S: f64 = 4.0e-6;
/// Pressure loss of the 1e-12 Pa case in Pa: 1e-14 relative, 50 ulp of 1e-12. The loss is the
/// difference of two readings of different datums that `difference_pa` keeps exact. Measured 0.
const SMALL_LOSS_TOL_PA: f64 = 1.0e-26;
/// Relative allowance for the flow of the 1e-12 Pa case on 32 cells: the derived error `2 / n^2`
/// = 1.953e-3 of the flow; 0.003 is 1.5 times that. Measured 1.953e-3 relative.
const SMALL_FLOW_RELATIVE_TOL: f64 = 0.003;

fn channel(model: &mut Model) -> &mut LaminarChannel {
    let Physics::LaminarChannel(case) = &mut model.physics else {
        panic!("expected the channel fixture");
    };
    case
}

fn uniform_gap(case: &mut LaminarChannel, n: usize) {
    case.mesh.vertices_m = (0..=n)
        .map(|i| case.geometry.gap_m * (i as f64 / n as f64))
        .collect();
}

fn pressure_drive(case: &mut LaminarChannel, outlet: Pressure, drop: f64) {
    case.drive = FlowDrive::Pressure {
        inlet: PressureAtPort {
            region: "inlet".to_owned(),
            pressure: Pressure::Gauge {
                value_pa: -5.0 + drop,
                reference_absolute_pa: 100_000.0,
            },
        },
        outlet: PressureAtPort {
            region: "outlet".to_owned(),
            pressure: outlet,
        },
    };
}

#[test]
fn solves_analytic_channel_pressure_loss_and_balances() {
    let report = solve(&Model::from_json(INPUT).unwrap()).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    // Plane Poiseuille relation, independent of the finite-volume system.
    let analytical = 12.0 * 1.0e-3 * 2.0 * 1.0e-5 / (0.1 * 0.01_f64.powi(3));
    close(
        scalar(&report, Output::PressureLoss),
        analytical,
        LOSS_TOL_PA,
    );
    close(
        scalar(&report, Output::VolumeFlowRate),
        1.0e-5,
        FLOW_TOL_M3_S,
    );
    close(
        scalar(&report, Output::MassFlowRate),
        0.01,
        MASS_FLOW_TOL_KG_S,
    );
    close(
        scalar(&report, Output::ChannelReynolds),
        200.0,
        REYNOLDS_TOL,
    );
    // The outlet pressure is the model's absolute input value, returned without arithmetic.
    close(
        region(&report, Output::AbsolutePressure, "outlet"),
        101_325.0,
        EXACT,
    );
    close(report.balances[0].imbalance, 0.0, MASS_BALANCE_TOL_KG_S);
    close(report.balances[1].imbalance, 0.0, MOMENTUM_BALANCE_TOL_N);
}

/// Gap `H` in metres and pressure loss `Δp` in Pa of the study models.
const GAP_M: f64 = 0.01;
const DROP_PA: f64 = 2.4;
/// Refinement ratio of the study levels: each level halves the nominal spacing `H/n`.
const RATIO: f64 = 2.0;
/// Cell counts of the study levels, each twice the previous. Four is the fewest levels that
/// SPEC-VER-003 and `band_check` accept. The 8-cell level is pre-asymptotic and 256 cells cannot be re-solved
/// at a hundredth of the tolerance; both are measured by `levels_outside_the_study_are_measured`
/// and explained on the physics page.
const LEVELS: [usize; 4] = [16, 32, 64, 128];
/// Vertices `H φ(i/n)` of a smooth, asymmetric, non-uniform mesh. The mapping
/// `φ(s) = s + c (sin(2π s − θ) + sin θ)/(2π)` with `c = 0.6`, `θ = 0.7` has `φ(0) = 0`,
/// `φ(1) = 1` and `φ' = 1 + c cos(2π s − θ) > 0`, so cell sizes vary by a factor of 4 across
/// the gap. Every level uses the same mapping, which keeps the family self-similar.
fn graded_vertices(n: usize) -> Vec<f64> {
    let (c, theta) = (0.6, 0.7_f64);
    let tau = std::f64::consts::TAU;
    let mut vertices: Vec<f64> = (0..=n)
        .map(|i| {
            let s = i as f64 / n as f64;
            GAP_M * (s + c * ((tau * s - theta).sin() + theta.sin()) / tau)
        })
        .collect();
    vertices[0] = 0.0;
    vertices[n] = GAP_M;
    vertices
}

/// Mean over a cell `[a, b]` of the exact velocity `(Δp / (2 μ L)) y (H − y)` in m/s.
fn exact_cell_mean_velocity(a: f64, b: f64) -> f64 {
    let coefficient = (DROP_PA / 2.0) / (2.0 * 1.0e-3);
    let integral = GAP_M * (b * b - a * a) / 2.0 - (b.powi(3) - a.powi(3)) / 3.0;
    coefficient * integral / (b - a)
}

/// Pressure-loss error in Pa of the volume-flow reference model on the graded mesh of `n` cells,
/// against the plane Poiseuille loss `12 μ L Q / (width H³) = 2.4 Pa`, solved with the given
/// relative residual tolerance.
fn pressure_loss_error(n: usize, tolerance: f64) -> f64 {
    let mut model = Model::from_json(INPUT).unwrap();
    channel(&mut model).mesh.vertices_m = graded_vertices(n);
    model.linear_solver.relative_residual_tolerance = tolerance;
    let report = solve(&model).unwrap();
    assert_eq!(
        report.stop_reason,
        StopReason::Converged,
        "n = {n}, tolerance {tolerance:e}"
    );
    (DROP_PA - scalar(&report, Output::PressureLoss)).abs()
}

/// Discrete L² norm in m/s, `sqrt(Σ h_i (ū_i − u_i)²)` over the cells, of the difference between
/// the cell mean of the exact velocity and the solved cell value, in the pressure-driven model
/// on the graded mesh of `n` cells. It measures the finite-volume error against the exact
/// solution and not at the cell centres alone, so it is an integral norm.
fn velocity_l2_error(n: usize, tolerance: f64) -> f64 {
    let mut model = Model::from_json(INPUT).unwrap();
    let case = channel(&mut model);
    pressure_drive(
        case,
        Pressure::Gauge {
            value_pa: -5.0,
            reference_absolute_pa: 100_000.0,
        },
        DROP_PA,
    );
    let vertices = graded_vertices(n);
    case.mesh.vertices_m = vertices.clone();
    model.linear_solver.relative_residual_tolerance = tolerance;
    let report = solve(&model).unwrap();
    assert_eq!(
        report.stop_reason,
        StopReason::Converged,
        "n = {n}, tolerance {tolerance:e}"
    );
    let Quantity::Field { values, .. } = &report.quantities[&Output::AxialVelocity] else {
        panic!("expected a velocity field");
    };
    assert_eq!(values.len(), n);
    let sum: f64 = vertices
        .windows(2)
        .zip(values)
        .map(|(edge, &u)| {
            let difference = exact_cell_mean_velocity(edge[0], edge[1]) - u;
            (edge[1] - edge[0]) * difference * difference
        })
        .sum();
    sum.sqrt()
}

/// One refinement study of an error norm over self-similar levels, per SPEC-VER-003.
struct Study {
    label: &'static str,
    cells: Vec<usize>,
    errors: Vec<f64>,
    tight_errors: Vec<f64>,
    band: BandCheck,
    checks: Vec<AlgebraicCheck>,
}

impl Study {
    /// Solves every level twice (the model tolerance and a hundredth of it) and applies the band
    /// rule of blueprint 11.2 (`verify::band_check`) to the errors of the model tolerance.
    /// `measure(n, tolerance)` returns the error norm of the level with `n` cells.
    fn run(
        label: &'static str,
        expected: f64,
        cells: &[usize],
        measure: impl Fn(usize, f64) -> f64,
    ) -> Result<Self, VerifyError> {
        let tolerance = LinearSettings::default().relative_residual_tolerance;
        let tight = tolerance / TOLERANCE_DIVISOR;
        let cells = cells.to_vec();
        let errors: Vec<f64> = cells.iter().map(|&n| measure(n, tolerance)).collect();
        let tight_errors: Vec<f64> = cells.iter().map(|&n| measure(n, tight)).collect();
        // Only the ratio of spacings enters the order, so the nominal spacing H/n is enough.
        let spacings: Vec<f64> = cells.iter().map(|&n| GAP_M / n as f64).collect();
        let band = band_check(&spacings, &errors, expected)?;
        let checks = errors
            .iter()
            .zip(&tight_errors)
            .map(|(&e, &t)| algebraic_check(e, t, RATIO))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            label,
            cells,
            errors,
            tight_errors,
            band,
            checks,
        })
    }

    fn print(&self) {
        let band = &self.band;
        println!("study {}", self.label);
        println!("  n      error            error(tol/100)   rel.change  pair.order");
        for (i, &n) in self.cells.iter().enumerate() {
            let pair = i
                .checked_sub(1)
                .map_or(String::new(), |j| format!("{:.6}", band.pairwise_orders[j]));
            println!(
                "  {n:<5}  {:.6e}  {:.6e}  {:.3e}  {pair}",
                self.errors[i], self.tight_errors[i], self.checks[i].relative_change
            );
        }
        println!(
            "  fitted order {:.6}, expected {}, spread {:.6}, half-width {:.6}, band [{:.6}, {:.6}], shrink rho {:.4}, finest distance {:.6}, budget {:.4e}",
            band.fitted_order,
            band.formal_order,
            band.spread,
            band.half_width,
            band.band[0],
            band.band[1],
            band.contraction,
            band.finest_distance,
            self.checks[0].budget
        );
    }

    /// Succeeds when the study meets SPEC-VER-003; otherwise names what failed.
    fn verdict(&self) -> Result<(), String> {
        if let Some(failure) = self.band.failure() {
            return Err(failure);
        }
        for (n, check) in self.cells.iter().zip(&self.checks) {
            if let Some(warning) = check.warning() {
                return Err(format!("{n} cells: {warning}"));
            }
        }
        Ok(())
    }
}

#[test]
fn grid_refinement_recovers_second_order_pressure_loss() {
    let study = Study::run(
        "pressure loss, graded mesh",
        2.0,
        &LEVELS,
        pressure_loss_error,
    )
    .unwrap();
    study.print();
    study.verdict().unwrap();

    // The uniform family keeps its pairwise ratios of the earlier form of this study.
    let mut model = Model::from_json(INPUT).unwrap();
    let errors: Vec<_> = [8, 16, 32, 64]
        .iter()
        .map(|&n| {
            uniform_gap(channel(&mut model), n);
            let report = solve(&model).unwrap();
            (2.4 - scalar(&report, Output::PressureLoss)).abs()
        })
        .collect();
    for pair in errors.windows(2) {
        assert!(
            UNIFORM_RATIO_BAND.contains(&(pair[0] / pair[1])),
            "{errors:?}"
        );
    }

    // On a uniform mesh the interior equations hold exactly for the parabola, and the wall cells
    // leave a force residual of -g h / 4 that the wall stiffness 2 μ / h turns into a uniform
    // velocity offset g h² / (8 μ). The flow response of the unit gradient is the midpoint sum
    // of the parabola, which exceeds the integral by g H h² / (24 μ), plus that offset times H.
    // At a prescribed flow the loss is therefore Δp / (1 + 2 / n²) and its error is exactly
    // 2 Δp / (n² + 2) (physics page). `DERIVED_ERROR_RELATIVE_TOL` bounds the difference.
    for (&n, &error) in [8, 16, 32, 64].iter().zip(&errors) {
        let derived = 2.0 * DROP_PA / ((n * n) as f64 + 2.0);
        println!(
            "uniform n {n}: error {error:.12e}, derived {derived:.12e}, relative difference {:.3e}",
            (error - derived).abs() / derived
        );
        close(error, derived, derived * DERIVED_ERROR_RELATIVE_TOL);
    }
}

#[test]
fn grid_refinement_recovers_second_order_velocity_in_the_l2_norm() {
    let study = Study::run(
        "velocity L2 norm, graded mesh",
        2.0,
        &LEVELS,
        velocity_l2_error,
    )
    .unwrap();
    study.print();
    study.verdict().unwrap();
}

/// Measures the two levels that the study leaves out, so that the page can say what they would
/// change. The band rule fits the finest three levels only, so the pre-asymptotic 8-cell level
/// adds a pairwise order and leaves the fit, the spread and the band unchanged; the 256-cell
/// level reaches the rounding floor of the algebraic check (blueprint 10.2).
#[test]
fn levels_outside_the_study_are_measured() {
    let with_coarse = [8, 16, 32, 64, 128];
    let study = Study::run(
        "pressure loss with the 8-cell level",
        2.0,
        &with_coarse,
        pressure_loss_error,
    )
    .unwrap();
    study.print();
    println!("  verdict with the 8-cell level: {:?}", study.verdict());
    let four = Study::run(
        "pressure loss, graded mesh",
        2.0,
        &LEVELS,
        pressure_loss_error,
    )
    .unwrap();
    // Same finest three levels and the same operations: the values are bit-identical, not close.
    assert_eq!(study.band.fitted_order, four.band.fitted_order);
    assert_eq!(study.band.spread, four.band.spread);
    assert_eq!(study.band.half_width, four.band.half_width);
    assert_eq!(study.band.contraction, four.band.contraction);
    assert_eq!(study.verdict(), four.verdict());
    // The coarse pair is farther from 2, by the shrink factor of the next pair.
    let [coarse, next, ..] = study.band.pairwise_orders[..] else {
        panic!("five levels give four pairwise orders");
    };
    println!(
        "  8 to 16 cells: pairwise order {coarse:.6}, distance to 2 {:.6}, shrink to the next pair {:.4}",
        2.0 - coarse,
        (2.0 - coarse) / (2.0 - next)
    );

    let mut model = Model::from_json(INPUT).unwrap();
    channel(&mut model).mesh.vertices_m = graded_vertices(256);
    let tolerance = LinearSettings::default().relative_residual_tolerance;
    for tolerance in [tolerance, tolerance / TOLERANCE_DIVISOR] {
        model.linear_solver.relative_residual_tolerance = tolerance;
        let report = solve(&model).unwrap();
        println!(
            "256 cells, tolerance {tolerance:e}: {:?}",
            report.stop_reason
        );
    }
    model.linear_solver.relative_residual_tolerance = tolerance;
    assert_eq!(solve(&model).unwrap().stop_reason, StopReason::Converged);
    model.linear_solver.relative_residual_tolerance = tolerance / TOLERANCE_DIVISOR;
    assert_ne!(solve(&model).unwrap().stop_reason, StopReason::Converged);
}

/// Error measure of a level: `(cells, relative solver tolerance) -> error norm`.
type TrapMeasure<'a> = &'a dyn Fn(usize, f64) -> f64;

/// Trap checks: a study of a quantity that carries no discretization error must fail. A study
/// that passed here would mean the rule cannot tell a refinement order from round-off. The
/// quantities scatter, so their spread is large and the band is wide; the rule still rejects
/// them, by the shrink rate when not by the band, and the test requires `passes()` to be false.
#[test]
fn the_study_fails_for_quantities_the_scheme_reproduces_exactly() {
    // Quantities that the scheme reproduces to round-off at every level: at a prescribed volume
    // flow it scales the velocity so that the integral equals the prescribed rate, and the
    // axial momentum balance closes by construction of the wall drags.
    let run = |n: usize, tolerance: f64| {
        let mut model = Model::from_json(INPUT).unwrap();
        channel(&mut model).mesh.vertices_m = graded_vertices(n);
        model.linear_solver.relative_residual_tolerance = tolerance;
        solve(&model).unwrap()
    };
    let traps: [(&str, TrapMeasure); 2] = [
        ("trap: volume flow at prescribed flow", &|n, tolerance| {
            (1.0e-5 - scalar(&run(n, tolerance), Output::VolumeFlowRate)).abs()
        }),
        ("trap: axial momentum imbalance", &|n, tolerance| {
            run(n, tolerance).balances[1].imbalance.abs()
        }),
    ];
    for (label, measure) in traps {
        let outcome = Study::run(label, 2.0, &LEVELS, measure);
        match &outcome {
            Ok(study) => {
                study.print();
                // The rule alone rejects the study; this file sets no width limit of its own.
                assert!(!study.band.passes(), "{label}");
            }
            Err(error) => println!("{label}: the fit refuses the errors: {error}"),
        }
        let rejected = outcome
            .map_err(|error| error.to_string())
            .and_then(|study| study.verdict());
        println!("{label}: verdict {rejected:?}");
        assert!(rejected.is_err(), "{label}");
    }

    // The only source is the driving pressure gradient. Removing it leaves the zero solution,
    // and validation refuses a drive without a positive pressure loss.
    let mut model = Model::from_json(INPUT).unwrap();
    let case = channel(&mut model);
    pressure_drive(
        case,
        Pressure::Gauge {
            value_pa: -5.0,
            reference_absolute_pa: 100_000.0,
        },
        0.0,
    );
    let error = solve(&model).unwrap_err();
    println!("trap: zero drive: {error}");
    assert!(matches!(error, caekit::Error::InvalidModel { .. }));
}

#[test]
fn pressure_drive_checks_parabolic_profile_and_gauge_reference() {
    let mut model = Model::from_json(INPUT).unwrap();
    let case = channel(&mut model);
    let outlet = Pressure::Gauge {
        value_pa: -5.0,
        reference_absolute_pa: 100_000.0,
    };
    pressure_drive(case, outlet, 2.4);
    uniform_gap(case, 64);
    let report = solve(&model).unwrap();
    close(
        scalar(&report, Output::PressureLoss),
        2.4,
        PRESSURE_DRIVE_LOSS_TOL_PA,
    );
    close(
        scalar(&report, Output::VolumeFlowRate),
        1.0e-5,
        PRESSURE_DRIVE_FLOW_TOL_M3_S,
    );
    // 100000 + (-5) adds two integers that binary64 represents exactly.
    close(
        region(&report, Output::AbsolutePressure, "outlet"),
        99_995.0,
        EXACT,
    );

    let Quantity::Field {
        positions_m,
        values,
        ..
    } = &report.quantities[&Output::AxialVelocity]
    else {
        panic!("expected a velocity field");
    };
    for (&[x, y, z], &velocity) in positions_m.iter().zip(values) {
        assert_eq!((x, z), (1.0, 0.0));
        let expected = (2.4 / 2.0) / (2.0 * 1.0e-3) * y * (0.01 - y);
        close(velocity, expected, PARABOLA_TOL_M_S);
    }
}

#[test]
fn mixed_pressure_references_preserve_a_small_driving_gradient() {
    let mut model = Model::from_json(INPUT).unwrap();
    let case = channel(&mut model);
    case.drive = FlowDrive::Pressure {
        inlet: PressureAtPort {
            region: "inlet".into(),
            pressure: Pressure::Gauge {
                value_pa: 1e-12,
                reference_absolute_pa: 101325.0,
            },
        },
        outlet: PressureAtPort {
            region: "outlet".into(),
            pressure: Pressure::Absolute { value_pa: 101325.0 },
        },
    };
    let report = solve(&model).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    close(
        scalar(&report, Output::PressureLoss),
        1e-12,
        SMALL_LOSS_TOL_PA,
    );
    let exact_flow = 1e-12 * 0.1 * 0.01_f64.powi(3) / (12.0 * 1e-3 * 2.0);
    close(
        scalar(&report, Output::VolumeFlowRate),
        exact_flow,
        exact_flow * SMALL_FLOW_RELATIVE_TOL,
    );
}

#[test]
fn pressure_reference_and_wall_assignments_are_checked() {
    let mut model = Model::from_json(INPUT).unwrap();
    let case = channel(&mut model);
    pressure_drive(
        case,
        Pressure::Gauge {
            value_pa: -100_001.0,
            reference_absolute_pa: 100_000.0,
        },
        2.4,
    );
    invalid_at(
        solve(&model).unwrap_err(),
        "physics.drive.outlet.pressure.value_pa",
        "must be positive and finite",
    );

    let mut model = Model::from_json(INPUT).unwrap();
    channel(&mut model).walls[0].region = "old_wall".to_owned();
    invalid_at(
        solve(&model).unwrap_err(),
        "physics.walls",
        "each transverse mesh endpoint",
    );
}
