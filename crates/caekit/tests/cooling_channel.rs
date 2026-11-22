//! Cooling channel: energy balances on nonmatching wall and coolant meshes, first-order
//! refinement of the outlet temperature, heat transfer in either direction, a coupling that
//! fails to converge, and rejected assignments.

// Reference values in tests are independent of the library decision and result paths.
#![allow(clippy::disallowed_methods)]

mod common;

use caekit::physics::cooling_channel::CoolingChannel;
use caekit::{Model, Output, Physics, Quantity, StopReason, solve};
use caekit_numerics::verify;
use common::{EXACT, close, invalid_at, scalar};

const INPUT: &[u8] = include_bytes!("../../../examples/cooling_channel.json");

// Allowances of the checks on the fixture meshes (31 wall and 48 coolant panels). "Measured" is
// the largest |actual - expected| over the sites that use the constant (print it in
// `common::close` and run `cargo test -p caekit --test cooling_channel -- --nocapture
// --test-threads=1`).

/// Outlet temperature against the continuum circuit solution in K. The difference is the
/// discretization error of the first-order upwind scheme. For 48 uniform coolant panels the
/// implicit-Euler solution gives `(T_hot - T_in) exp(-NTU) NTU^2 / (2 n)` = 0.343 K with
/// NTU = 1.7294 (derived: `(1 + NTU/n)^-n = exp(-NTU) (1 + NTU^2/(2 n))`). Measured 0.340 K, so
/// the allowance is 1.5 times the error that the scheme has by construction.
const OUTLET_TOL_K: f64 = 0.5;
/// Heat duty against `C (T_out - T_in)` in W: 5e-10 of the 2.13 kW duty. The two are the same
/// product up to rounding; measured 9.1e-13 W, 2 ulp (2^-41 = 4.5e-13).
const DUTY_TOL_W: f64 = 1.0e-6;
/// Energy balance imbalances in W. The coupled stage stops at the larger of 1e-10 W and 1e-10
/// times its initial residual norm, and `balance_bounds.rs` bounds three of the four balances by
/// 1.2e-7 to 1.9e-7 W for this model (its `--nocapture` output); 1e-6 W is 5 times the largest.
/// Measured 6.3e-8 W.
const BALANCE_TOL_W: f64 = 1.0e-6;
/// Hot-face temperature of the wall in K: a prescribed value, reproduced to rounding. 1e-12 K is
/// 17 ulp of 360 K (2^-44 = 5.7e-14). Measured 0.
const HOT_FACE_TOL_K: f64 = 1.0e-12;
/// Integral of the interface flux over the wall panels against the heat duty, in W. The sum of 31
/// products of about 2.1 kW in total has a rounding bound of `(m - 1) eps sum|v|` = 1.4e-11 W
/// (Higham, Accuracy and Stability of Numerical Algorithms, 2nd ed., section 4.2); 1e-9 W is 70
/// times that. Measured 4.5e-13 W.
const FLUX_SUM_TOL_W: f64 = 1.0e-9;
/// Order fitted to exact `h^2` and `h^0` data: the fit takes logarithms of values in (0, 1], whose
/// rounding is about 1e-16, so 1e-12 leaves four decades. Measured 0.
const FIT_TOL: f64 = 1.0e-12;
/// Band of the error ratio of successive uniform refinements of the outlet temperature. A first
/// order scheme gives 2; the pre-asymptotic term moves the ratio to 1.967187, 1.983352 and
/// 1.991616 (docs/physics/cooling-channel.md), whose distances from 2 (0.033, 0.017, 0.008)
/// halve per level as an `h^2` correction does. The band is 2 +- 0.1, three times the largest
/// of those distances and symmetric, so it still excludes second-order ratios near 4 and
/// no-decrease ratios near 1.
const OUTLET_RATIO_BAND: std::ops::Range<f64> = 1.9..2.1;

fn channel(model: &mut Model) -> &mut CoolingChannel {
    let Physics::CoolingChannel(case) = &mut model.physics else {
        panic!("expected the cooling channel fixture");
    };
    case
}

fn continuum_outlet() -> f64 {
    let per_area_resistance = 0.004 / 15.0 + 1.0 / 400.0;
    let ntu: f64 = (2.0 * 0.1 / per_area_resistance) / (0.01 * 4180.0);
    360.0 - (360.0 - 298.0) * (-ntu).exp()
}

#[test]
fn solves_nonmatching_meshes_and_closes_energy_balances() {
    let report = solve(&Model::from_json(INPUT).unwrap()).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    assert_eq!(report.geometry.len(), 2);
    assert_eq!(
        report
            .meshes
            .iter()
            .map(|m| m.cell_count)
            .collect::<Vec<_>>(),
        [32, 31, 3, 48]
    );
    assert_eq!(report.convergence.len(), 2);
    assert!(report.convergence[1].iterations > 1);
    let outlet = scalar(&report, Output::OutletTemperature);
    close(outlet, continuum_outlet(), OUTLET_TOL_K);
    close(
        scalar(&report, Output::HeatTransferRate),
        0.01 * 4180.0 * (outlet - 298.0),
        DUTY_TOL_W,
    );

    for name in [
        "wall_energy",
        "interface_energy",
        "coolant_energy",
        "system_energy",
    ] {
        let balance = report.balances.iter().find(|b| b.name == name).unwrap();
        close(balance.imbalance, 0.0, BALANCE_TOL_W);
    }

    let Quantity::Field {
        positions_m,
        values,
        ..
    } = &report.quantities[&Output::WallTemperature]
    else {
        panic!("expected world-space wall samples");
    };
    assert_eq!(positions_m.len(), 31 * 4);
    assert_eq!(values.len(), positions_m.len());
    for (samples, temperatures) in positions_m.chunks_exact(4).zip(values.chunks_exact(4)) {
        assert_eq!(samples[0][1], -0.004);
        assert_eq!(samples[3][1], 0.0);
        close(temperatures[0], 360.0, HOT_FACE_TOL_K);
        assert!(temperatures.windows(2).all(|p| p[0] > p[1]));
    }

    let mut model = Model::from_json(INPUT).unwrap();
    let geometry = channel(&mut model);
    let wall_edges = &geometry.wall.axial_mesh.vertices_m;
    let Quantity::Field { values: fluxes, .. } = &report.quantities[&Output::InterfaceHeatFlux]
    else {
        panic!("expected interface flux field");
    };
    let integrated: f64 = wall_edges
        .windows(2)
        .zip(fluxes)
        .map(|(edge, flux)| (edge[1] - edge[0]) * 0.1 * flux)
        .sum();
    close(
        integrated,
        scalar(&report, Output::HeatTransferRate),
        FLUX_SUM_TOL_W,
    );
}

#[test]
fn outlet_temperature_error_halves_on_matching_uniform_meshes() {
    let mut model = Model::from_json(INPUT).unwrap();
    let exact = continuum_outlet();
    let mut errors = Vec::new();
    for n in [12, 24, 48, 96] {
        let case = channel(&mut model);
        let vertices: Vec<_> = (0..=n).map(|i| 2.0 * i as f64 / n as f64).collect();
        case.wall.axial_mesh.vertices_m.clone_from(&vertices);
        case.coolant.axial_mesh.vertices_m = vertices;

        let report = solve(&model).unwrap();
        assert_eq!(report.stop_reason, StopReason::Converged);
        errors.push(exact - scalar(&report, Output::OutletTemperature));
    }
    for pair in errors.windows(2) {
        println!("outlet error ratio {:.6}", pair[0] / pair[1]);
        assert!(
            OUTLET_RATIO_BAND.contains(&(pair[0] / pair[1])),
            "{errors:?}"
        );
    }
}

// The order study of the coolant temperature (SPEC-VER-002 and SPEC-VER-003). The outlet
// temperature of the test above is one value at the end of the channel; this study measures
// the whole axial profile in an L2 norm, on nonmatching, nonuniform and self-similar meshes.

/// Length of the channel in m; the fixture's `physics.flow.geometry.length_m`.
const LENGTH_M: f64 = 2.0;
/// Refinement ratio of the axial meshes between successive levels: every level has twice the
/// cells of the previous one on both the wall and the coolant mesh.
const RATIO: f64 = 2.0;
/// Number of levels solved and reported. The band rule fits the finest three; the others show
/// how the pairwise orders approach the formal order (see the physics page).
const LEVELS: usize = 7;
/// Cells of the coarsest wall and coolant meshes. The 8 to 12 ratio keeps the two meshes
/// nonmatching, with interface overlaps of unequal size, at every level.
const COARSEST_WALL_CELLS: usize = 8;
const COARSEST_COOLANT_CELLS: usize = 12;
/// The fixture omits `physics.coupling_solver`, so the defaults of `docs/model.md` apply: a
/// relative tolerance of 1e-10 and an absolute tolerance of 1e-10 W. The tight solve of a level
/// divides both by `verify::TOLERANCE_DIVISOR`, so that the relative one still governs.
const FIXTURE_RELATIVE_TOLERANCE: f64 = 1.0e-10;
/// The absolute coupling tolerance of the same defaults in W, divided with the relative one.
const FIXTURE_ABSOLUTE_TOLERANCE_W: f64 = 1.0e-10;
/// Formal order of the first-order upwind enthalpy flux (see the physics page).
const FORMAL_ORDER: f64 = 1.0;

/// The analytic coolant temperature `T(x) = T_hot - (T_hot - T_in) exp(-rate x)` of the thermal
/// circuit (see the physics page), with the fixture's synthetic inputs.
struct Profile {
    hot_k: f64,
    inlet_k: f64,
    rate_per_m: f64,
}

impl Profile {
    fn new(hot_k: f64) -> Self {
        let per_area_resistance = 0.004 / 15.0 + 1.0 / 400.0;
        let capacity_flow = 0.01 * 4180.0;
        Self {
            hot_k,
            inlet_k: 298.0,
            rate_per_m: 0.1 / (per_area_resistance * capacity_flow),
        }
    }

    fn at(&self, x: f64) -> f64 {
        self.hot_k - (self.hot_k - self.inlet_k) * (-self.rate_per_m * x).exp()
    }
}

/// Vertices of a mesh of `n` cells over the channel, `LENGTH_M * map(i / n)`; the end vertices
/// are exact. A `map` with a derivative in `(0, 2)` and `map(0) = 0`, `map(1) = 1` gives a mesh
/// whose cell sizes follow the same smooth distribution at every `n`, so the family is
/// self-similar and its largest cell is proportional to `1/n`.
fn mapped_vertices(n: usize, map: impl Fn(f64) -> f64) -> Vec<f64> {
    (0..=n)
        .map(|i| match i {
            0 => 0.0,
            i if i == n => LENGTH_M,
            i => LENGTH_M * map(i as f64 / n as f64),
        })
        .collect()
}

/// Cells graded linearly: `g' = 1 + 0.6 (1 - 2 xi)` runs from 1.6 to 0.4, so the coolant cells
/// near the inlet are four times the size of those at the outlet.
fn coolant_map(xi: f64) -> f64 {
    xi + 0.6 * xi * (1.0 - xi)
}

/// Cells perturbed by a sine: `g' = 1 + 0.4 cos(2 pi xi)` runs from 0.6 to 1.4.
fn wall_map(xi: f64) -> f64 {
    xi + 0.4 * (2.0 * std::f64::consts::PI * xi).sin() / (2.0 * std::f64::consts::PI)
}

/// One level of the study.
struct Level {
    spacing: f64,
    wall_cells: usize,
    coolant_cells: usize,
    /// RMS error of the coolant temperature over the channel in K, at the model's tolerance.
    error_k: f64,
    /// The same norm from the solve at the tolerances divided by `TOLERANCE_DIVISOR`.
    tight_error_k: f64,
    iterations: usize,
    tight_iterations: usize,
}

/// `sqrt(1/L * integral of (T_h - T)^2 dx)` in K, with `T_h` the piecewise-constant coolant
/// temperature on the cells of `vertices_m`. The integrand is smooth on each cell, and the
/// 5-point Gauss-Legendre rule (degree 9) is exact to far below the discretization error.
fn rms_error_k(profile: &Profile, vertices_m: &[f64], values_k: &[f64]) -> f64 {
    const NODES: [f64; 5] = [
        0.0,
        0.538_469_310_105_683_1,
        -0.538_469_310_105_683_1,
        0.906_179_845_938_664,
        -0.906_179_845_938_664,
    ];
    const WEIGHTS: [f64; 5] = [
        0.568_888_888_888_888_9,
        0.478_628_670_499_366_5,
        0.478_628_670_499_366_5,
        0.236_926_885_056_189_1,
        0.236_926_885_056_189_1,
    ];
    let mut sum = 0.0;
    for (edge, value) in vertices_m.windows(2).zip(values_k) {
        let (centre, half) = (0.5 * (edge[0] + edge[1]), 0.5 * (edge[1] - edge[0]));
        for (node, weight) in NODES.iter().zip(WEIGHTS) {
            let difference = value - profile.at(centre + half * node);
            sum += half * weight * difference * difference;
        }
    }
    (sum / LENGTH_M).sqrt()
}

/// Solves the model on the meshes of one level, with the coupling tolerances of the fixture
/// divided by `divisor`, and returns the RMS error of the coolant temperature, in K, and the
/// number of coupling sweeps.
fn level_error_k(
    model: &mut Model,
    profile: &Profile,
    wall_cells: usize,
    coolant_cells: usize,
    divisor: f64,
) -> (f64, usize) {
    let case = channel(model);
    case.wall.axial_mesh.vertices_m = mapped_vertices(wall_cells, wall_map);
    case.coolant.axial_mesh.vertices_m = mapped_vertices(coolant_cells, coolant_map);
    case.coupling_solver.relative_residual_tolerance = FIXTURE_RELATIVE_TOLERANCE / divisor;
    case.coupling_solver.absolute_residual_tolerance_w = FIXTURE_ABSOLUTE_TOLERANCE_W / divisor;
    let coolant = case.coolant.axial_mesh.vertices_m.clone();

    let report = solve(model).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    let Quantity::Field { values, .. } = &report.quantities[&Output::FluidBulkTemperature] else {
        panic!("expected the coolant temperature field");
    };
    (
        rms_error_k(profile, &coolant, values),
        report.convergence[1].iterations,
    )
}

/// Runs every level at the fixture's tolerance and again at the tightened one.
fn run_levels(hot_k: f64) -> Vec<Level> {
    let mut model = Model::from_json(INPUT).unwrap();
    let settings = channel(&mut model).coupling_solver;
    assert_eq!(
        settings.relative_residual_tolerance,
        FIXTURE_RELATIVE_TOLERANCE
    );
    assert_eq!(
        settings.absolute_residual_tolerance_w,
        FIXTURE_ABSOLUTE_TOLERANCE_W
    );
    channel(&mut model).wall.hot_boundary.temperature_k = hot_k;
    let profile = Profile::new(hot_k);
    (0..LEVELS)
        .map(|level| {
            let scale = 1_usize << level;
            let (wall_cells, coolant_cells) =
                (COARSEST_WALL_CELLS * scale, COARSEST_COOLANT_CELLS * scale);
            let (error_k, iterations) =
                level_error_k(&mut model, &profile, wall_cells, coolant_cells, 1.0);
            let (tight_error_k, tight_iterations) = level_error_k(
                &mut model,
                &profile,
                wall_cells,
                coolant_cells,
                verify::TOLERANCE_DIVISOR,
            );
            Level {
                spacing: 1.0 / scale as f64,
                wall_cells,
                coolant_cells,
                error_k,
                tight_error_k,
                iterations,
                tight_iterations,
            }
        })
        .collect()
}

/// What the study of a family of levels concludes.
struct Assessment {
    /// The band rule of blueprint section 11.2 (`verify::band_check`) over the levels.
    band: verify::BandCheck,
    /// Largest relative change of an error norm between the two solves of a level.
    worst_algebraic_change: f64,
    algebraic_checks: Vec<verify::AlgebraicCheck>,
}

/// Applies the band rule to the levels and checks the algebraic error of each. Fails, instead of
/// reporting an order, for errors that are zero or not finite.
fn assess(levels: &[Level]) -> Result<Assessment, verify::VerifyError> {
    let spacings: Vec<f64> = levels.iter().map(|l| l.spacing).collect();
    let errors: Vec<f64> = levels.iter().map(|l| l.error_k).collect();
    let band = verify::band_check(&spacings, &errors, FORMAL_ORDER)?;
    let algebraic_checks = levels
        .iter()
        .map(|l| verify::algebraic_check(l.error_k, l.tight_error_k, RATIO))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Assessment {
        band,
        worst_algebraic_change: algebraic_checks
            .iter()
            .map(|c| c.relative_change)
            .fold(0.0, f64::max),
        algebraic_checks,
    })
}

#[test]
fn thermal_mesh_refinement_recovers_first_order_enthalpy_advection() {
    let levels = run_levels(360.0);
    let assessment = assess(&levels).unwrap();
    let band = &assessment.band;
    const { assert!(LEVELS >= verify::MIN_STUDY_LEVELS) };

    println!(
        "level  wall  coolant  spacing  iterations  rms_error_K  pairwise_order  tight_rms_error_K  tight_iterations  relative_change"
    );
    for (i, (level, check)) in levels.iter().zip(&assessment.algebraic_checks).enumerate() {
        let order = i.checked_sub(1).map_or("-".to_owned(), |k| {
            format!("{:.6}", band.pairwise_orders[k])
        });
        println!(
            "{i:>5}  {:>4}  {:>7}  {:>7.5}  {:>10}  {:>11.6e}  {order:>14}  {:>17.6e}  {:>16}  {:>15.3e}",
            level.wall_cells,
            level.coolant_cells,
            level.spacing,
            level.iterations,
            level.error_k,
            level.tight_error_k,
            level.tight_iterations,
            check.relative_change,
        );
    }
    let spacings: Vec<f64> = levels.iter().map(|l| l.spacing).collect();
    let errors: Vec<f64> = levels.iter().map(|l| l.error_k).collect();
    println!(
        "fitted order over the finest three levels {:.6} (over all {LEVELS}, for comparison: {:.6})",
        band.fitted_order,
        verify::fitted_order(&spacings, &errors).unwrap()
    );
    println!(
        "spread s of the finest two pairwise orders {:.6}, band half-width {:.6}, band [{:.6}, {:.6}]",
        band.spread, band.half_width, band.band[0], band.band[1]
    );
    println!(
        "shrink factor rho {:.4}, finest distance {:.6}",
        band.contraction, band.finest_distance
    );
    println!(
        "algebraic budget {:.6e}, worst relative change {:.3e}",
        assessment.algebraic_checks[0].budget, assessment.worst_algebraic_change
    );

    assert!(band.passes(), "{:?}", band.failure());
    for check in &assessment.algebraic_checks {
        assert!(check.within_budget(), "{:?}", check.warning());
    }
    // The errors decrease at every level: the study measures discretization error.
    assert!(levels.windows(2).all(|p| p[1].error_k < p[0].error_k));
}

#[test]
fn the_order_study_rejects_a_solution_the_scheme_represents_exactly() {
    // With the hot face at the inlet temperature the coolant stays at 298 K, a constant that the
    // piecewise-constant space represents exactly: the study of the same meshes has no error to
    // fit and must fail instead of reporting an order. The cooling channel has no source term;
    // the exchange with the wall is what makes the solution non-constant.
    let levels = run_levels(298.0);
    assert!(levels.iter().all(|l| l.error_k == 0.0));
    let outcome = assess(&levels);
    println!(
        "isothermal trap: errors {:?} K, study outcome {:?}",
        levels.iter().map(|l| l.error_k).collect::<Vec<_>>(),
        outcome.as_ref().err()
    );
    assert_eq!(
        outcome.err(),
        Some(verify::VerifyError::NotPositive {
            quantity: "error",
            index: 0
        })
    );

    // The band is two-sided: errors that fall as h^2 (a scheme better than the formal order) and
    // errors that do not fall (no convergence) are both rejected as a fit outside the band.
    let spacings: Vec<f64> = (0..LEVELS).map(|k| 0.5_f64.powi(k as i32)).collect();
    for (exponent, expected) in [(2, 2.0), (0, 0.0)] {
        let errors: Vec<f64> = spacings.iter().map(|h| h.powi(exponent)).collect();
        let check = verify::band_check(&spacings, &errors, FORMAL_ORDER).unwrap();
        close(check.fitted_order, expected, FIT_TOL);
        println!(
            "synthetic errors h^{exponent}: fitted order {:.6}, half-width {:.6}, outcome {:?}",
            check.fitted_order,
            check.half_width,
            check.failure()
        );
        assert!(!check.passes());
        assert!(check.failure().unwrap().starts_with("fitted order"));
    }
}

#[test]
fn transfers_heat_in_either_direction_and_has_zero_isothermal_flux() {
    let mut model = Model::from_json(INPUT).unwrap();
    channel(&mut model).wall.hot_boundary.temperature_k = 280.0;
    let report = solve(&model).unwrap();
    assert!(scalar(&report, Output::HeatTransferRate) < 0.0);
    // The coolant is heated from 298 K towards the 280 K wall and cools: it ends between them.
    assert!((280.0..298.0).contains(&scalar(&report, Output::OutletTemperature)));

    channel(&mut model).wall.hot_boundary.temperature_k = 298.0;
    let report = solve(&model).unwrap();
    // Both wall and coolant are at 298 K, so no temperature difference drives any arithmetic:
    // the outlet is the inlet value and the duty is exactly zero (measured 0 for both).
    close(scalar(&report, Output::OutletTemperature), 298.0, EXACT);
    close(scalar(&report, Output::HeatTransferRate), 0.0, EXACT);
    assert_eq!(report.convergence[1].iterations, 0);
}

#[test]
fn stops_with_trace_if_coupling_fails_to_converge() {
    let mut model = Model::from_json(INPUT).unwrap();
    // One Gauss-Seidel sweep cannot reach the 1e-10 relative threshold of the fixture.
    channel(&mut model).coupling_solver.max_iterations = 1;
    let report = solve(&model).unwrap();
    assert_eq!(report.stop_reason, StopReason::IterationLimit);
    assert_eq!(report.convergence.len(), 2);
    assert_eq!(report.convergence[0].stop_reason, StopReason::Converged);
    assert_eq!(
        report.convergence[1].stop_reason,
        StopReason::IterationLimit
    );
    assert!(report.quantities.is_empty());
    assert!(report.balances.is_empty());
    assert!(
        report.convergence[1].final_true_residual_l2.unwrap()
            > report.convergence[1].residual_threshold.unwrap()
    );
}

#[test]
fn rejects_a_stale_surface_assignment_and_wrong_axial_ports() {
    let mut model = Model::from_json(INPUT).unwrap();
    channel(&mut model).interface.fluid_region = "deleted_wall".to_owned();
    invalid_at(
        model.validate().unwrap_err(),
        "physics.interface.fluid_region",
        "wall",
    );

    let mut model = Model::from_json(INPUT).unwrap();
    channel(&mut model).coolant.axial_mesh.start_region = "outlet".to_owned();
    invalid_at(
        model.validate().unwrap_err(),
        "physics.coolant.axial_mesh",
        "endpoint region names must differ",
    );
}
