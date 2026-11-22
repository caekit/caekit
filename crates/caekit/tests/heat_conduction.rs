//! Heat conduction: the linear analytical profile with its flux and boundary heat rates,
//! flux and convection conditions, weak convection, the refinement study with a uniform source
//! and its exactness trap, and rejected thermal inputs.

// Reference values in tests are independent of the library decision and result paths.
#![allow(clippy::disallowed_methods)]

// The shared block helpers also hold the interior sample rule, which this file replaced by its own
// degree-seven rule.
#[allow(dead_code)]
#[path = "common/block.rs"]
mod block;
mod common;

use caekit::physics::heat_conduction::{HeatConduction, ThermalBoundary};
use caekit::{Model, Output, Physics, Quantity, RunReport, SampleLocation, StopReason, solve};

use block::{interpolate, structured_block};
use caekit_numerics::verify;
use common::{close, invalid_at, region};

// Allowances of the affine cases. Linear tetrahedra represent an affine temperature profile
// exactly, so the solution differs from the analytical one only by the linear solve, which stops
// at the default relative residual 1e-10 of docs/model.md, and by binary64 rounding. "Measured" is
// the largest |actual - expected| over the sites that use the constant (print it in
// `common::close` and run `cargo test -p caekit --test heat_conduction -- --nocapture`).

/// Nodal temperature in K: 1e-10 of the 100 K span of the first case. Measured 0.
const AFFINE_TEMPERATURE_TOL_K: f64 = 1.0e-8;
/// Cell heat flux in W/m^2: 5e-11 of the 20 kW/m^2 of the first case, 1e-9 of the 1 kW/m^2 of the
/// second. Measured 1.1e-11 W/m^2.
const AFFINE_FLUX_TOL_W_M2: f64 = 1.0e-6;
/// Boundary heat rates and the energy imbalance in W: 5e-11 of the 200 W of the first case, 1e-9
/// of the 10 W of the second. Measured 1.6e-13 W.
const AFFINE_POWER_TOL_W: f64 = 1.0e-8;
/// Nodal temperature of the weak-convection case in K. The test must resolve the 2.5e-5 K drop
/// that the flux 0.1 W/m^2 makes across 0.1 m of 400 W/(m K), so the allowance is 4e-5 of that
/// drop and 3e-12 of 310 K. Measured 5.7e-14 K, 1 ulp of 310 K.
const WEAK_TEMPERATURE_TOL_K: f64 = 1.0e-9;
/// Cell heat flux of the weak-convection case in W/m^2: 1e-8 of the 0.1 W/m^2 flux. Measured
/// 5.5e-14 W/m^2.
const WEAK_FLUX_TOL_W_M2: f64 = 1.0e-9;
/// Convective heat rate of the weak-convection case in W: 1e-9 of 1e-3 W. Measured 4.3e-19 W,
/// 2 ulp of 1e-3.
const WEAK_POWER_TOL_W: f64 = 1.0e-12;
/// Generation, outflow and imbalance of the source case in W: 1e-8 of the 200 W that the source
/// generates, as docs/physics/heat-conduction.md states. Measured 6.4e-10 W for the outflow and
/// the imbalance (they are the solve residual of the free equations) and 1.4e-13 W for the
/// generation term.
const SOURCE_BALANCE_TOL_W: f64 = 2.0e-6;

fn example() -> Model {
    Model::from_json(include_bytes!("../../../examples/heat_conduction.json")).unwrap()
}

fn heat_case(model: &mut Model) -> &mut HeatConduction {
    let Physics::HeatConduction(case) = &mut model.physics else {
        panic!("expected heat conduction");
    };
    case
}

fn temperatures(report: &RunReport) -> (&[[f64; 3]], &[f64]) {
    let Quantity::Field {
        unit,
        location: SampleLocation::Node,
        positions_m,
        values,
    } = &report.quantities[&Output::SolidTemperature]
    else {
        panic!("expected nodal temperature");
    };
    assert_eq!(unit, "K");
    (positions_m, values)
}

fn fluxes(report: &RunReport) -> (&[[f64; 3]], &[[f64; 3]]) {
    let Quantity::VectorField {
        unit,
        location: SampleLocation::Cell,
        positions_m,
        values,
    } = &report.quantities[&Output::SolidHeatFlux]
    else {
        panic!("expected cell heat flux vectors");
    };
    assert_eq!(unit, "W/m^2");
    (positions_m, values)
}

fn temperature(region: &str, temperature_k: f64) -> ThermalBoundary {
    ThermalBoundary::Temperature {
        region: region.to_owned(),
        temperature_k,
    }
}

#[test]
fn linear_temperature_profile_flux_and_reactions() {
    let report = solve(&example()).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    let [stage] = report.convergence.as_slice() else {
        panic!("expected one thermal stage");
    };
    assert_eq!(stage.residual_unit, "W");
    assert!(stage.final_true_residual_l2.unwrap() <= stage.residual_threshold.unwrap());
    let (nodes, values) = temperatures(&report);
    assert!(nodes.contains(&[0.05; 3]));
    for (&[x, _, _], &value) in nodes.iter().zip(values) {
        close(value, 400.0 - 1_000.0 * x, AFFINE_TEMPERATURE_TOL_K);
    }
    let (_, q) = fluxes(&report);
    assert_eq!(q.len(), 12);
    for v in q {
        for (&actual, expected) in v.iter().zip([20_000.0, 0.0, 0.0]) {
            close(actual, expected, AFFINE_FLUX_TOL_W_M2);
        }
    }
    for (name, expected) in [("x0", 200.0), ("x1", -200.0), ("y0", 0.0), ("z1", 0.0)] {
        close(
            region(&report, Output::BoundaryHeatRate, name),
            expected,
            AFFINE_POWER_TOL_W,
        );
    }
    assert_eq!(report.balances[0].name, "solid_energy");
    assert_eq!(report.balances[0].unit, "W");
    close(report.balances[0].imbalance, 0.0, AFFINE_POWER_TOL_W);
}

#[test]
fn prescribed_flux_and_convection_set_the_temperature_without_dirichlet() {
    let mut model = example();
    heat_case(&mut model).thermal.boundary_conditions = vec![
        ThermalBoundary::HeatFlux {
            region: "x0".to_owned(),
            inward_heat_flux_w_m2: 1_000.0,
        },
        ThermalBoundary::Convection {
            region: "x1".to_owned(),
            ambient_temperature_k: 300.0,
            film_coefficient_w_m2_k: 100.0,
        },
    ];
    let report = solve(&model).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    let (nodes, values) = temperatures(&report);
    for (&[x, _, _], &value) in nodes.iter().zip(values) {
        close(value, 315.0 - 50.0 * x, AFFINE_TEMPERATURE_TOL_K);
    }
    for v in fluxes(&report).1 {
        for (&actual, expected) in v.iter().zip([1_000.0, 0.0, 0.0]) {
            close(actual, expected, AFFINE_FLUX_TOL_W_M2);
        }
    }
    close(
        region(&report, Output::BoundaryHeatRate, "x0"),
        10.0,
        AFFINE_POWER_TOL_W,
    );
    close(
        region(&report, Output::BoundaryHeatRate, "x1"),
        -10.0,
        AFFINE_POWER_TOL_W,
    );
    close(report.balances[0].imbalance, 0.0, AFFINE_POWER_TOL_W);
}

#[test]
fn weak_convection_resolves_the_mean_temperature_and_small_gradient() {
    let mut model = example();
    let thermal = &mut heat_case(&mut model).thermal;
    thermal.material.thermal_conductivity_w_m_k = 400.0;
    thermal.boundary_conditions = vec![
        ThermalBoundary::HeatFlux {
            region: "x0".into(),
            inward_heat_flux_w_m2: 0.1,
        },
        ThermalBoundary::Convection {
            region: "x1".into(),
            ambient_temperature_k: 300.0,
            film_coefficient_w_m2_k: 0.01,
        },
    ];
    let report = solve(&model).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    let (nodes, values) = temperatures(&report);
    for (&[x, _, _], &value) in nodes.iter().zip(values) {
        close(
            value,
            310.0 + (0.1 - x) * 0.1 / 400.0,
            WEAK_TEMPERATURE_TOL_K,
        );
    }
    for flux in fluxes(&report).1 {
        close(flux[0], 0.1, WEAK_FLUX_TOL_W_M2);
    }
    close(
        region(&report, Output::BoundaryHeatRate, "x1"),
        -0.001,
        WEAK_POWER_TOL_W,
    );
}

#[test]
fn shared_prescribed_nodes_contribute_once_to_the_energy_balance() {
    let mut model = example();
    let case = heat_case(&mut model);
    let left = case.mesh.boundary_regions[0].faces.pop().unwrap();
    case.mesh.boundary_regions.push(caekit::FaceRegion {
        name: "x0_split".to_owned(),
        faces: vec![left],
    });
    case.thermal
        .boundary_conditions
        .push(temperature("x0_split", 400.0));
    let report = solve(&model).unwrap();
    for (name, expected) in [("x0", 100.0), ("x0_split", 100.0), ("x1", -200.0)] {
        close(
            region(&report, Output::BoundaryHeatRate, name),
            expected,
            AFFINE_POWER_TOL_W,
        );
    }
    close(report.balances[0].imbalance, 0.0, AFFINE_POWER_TOL_W);
}

// Refinement study of the source case (SPEC-VER-002, SPEC-VER-003). The exact solution of
// `-k T'' = Q` on `0 < x < L` with `T(0) = T0` and `T(L) = T1` is quadratic in x, so the
// piecewise-linear solution cannot represent it, and both norms below are integral norms.

const LENGTH: f64 = 0.1;
const SOURCE: f64 = 200_000.0;
const K: f64 = 20.0;
const T_AT_X0: f64 = 400.0;
const T_AT_X1: f64 = 300.0;
/// Relative solver tolerance of the study solve, the model default.
const STUDY_TOLERANCE: f64 = 1.0e-10;
/// Refinement ratio of consecutive levels.
const RATIO: f64 = 2.0;
/// Divisions per axis of the levels, coarse to fine. Each level halves `h = LENGTH / n`.
const LEVELS: [usize; 4] = [2, 4, 8, 16];
/// Largest displacement of a free vertex coordinate in units of `h`. The six-tetrahedron split
/// of a cube has the smallest volume `h^3/6`; the solver rejects a mesh with an inverted cell,
/// so every level of the study also proves that this amplitude keeps all cells positive.
const PERTURBATION: f64 = 0.15;
/// Formal orders of piecewise-linear elements: `p = 2` in L2 and `p = 1` in the energy norm for
/// a solution in `H^2` (Ciarlet 2002, Thm. 3.2.1 and the Aubin-Nitsche argument).
const L2_ORDER: f64 = 2.0;
const ENERGY_ORDER: f64 = 1.0;

fn exact_temperature(x: f64, source: f64) -> f64 {
    T_AT_X0 + (T_AT_X1 - T_AT_X0) * x / LENGTH + source * x * (LENGTH - x) / (2.0 * K)
}

fn exact_flux_x(x: f64, source: f64) -> f64 {
    K * (T_AT_X0 - T_AT_X1) / LENGTH + source * (x - LENGTH / 2.0)
}

/// Deterministic value in `[-1, 1)` from a splitmix64 hash of `key`.
fn hash_unit(key: u64) -> f64 {
    let mut z = key.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 52) as f64 - 1.0
}

/// The structured block with every vertex moved along each axis on which it is not on the
/// block surface, so a vertex on a face slides inside that face and the surface stays planar.
fn perturbed_block(divisions: usize) -> caekit::TetraMesh {
    let mut mesh = structured_block([LENGTH; 3], divisions);
    let h = LENGTH / divisions as f64;
    for (index, vertex) in mesh.vertices_m.iter_mut().enumerate() {
        for (axis, coordinate) in vertex.iter_mut().enumerate() {
            if *coordinate != 0.0 && *coordinate != LENGTH {
                *coordinate += PERTURBATION * h * hash_unit((3 * index + axis) as u64);
            }
        }
    }
    mesh
}

/// Gauss-Legendre rule of four points on `[0, 1]`, exact to degree seven.
fn gauss_legendre_4() -> [(f64, f64); 4] {
    let s = (6.0_f64 / 5.0).sqrt();
    let inner = (3.0 / 7.0 - 2.0 / 7.0 * s).sqrt();
    let outer = (3.0 / 7.0 + 2.0 / 7.0 * s).sqrt();
    let w_inner = (18.0 + 30.0_f64.sqrt()) / 36.0;
    let w_outer = (18.0 - 30.0_f64.sqrt()) / 36.0;
    [
        ((1.0 - outer) / 2.0, w_outer / 2.0),
        ((1.0 - inner) / 2.0, w_inner / 2.0),
        ((1.0 + inner) / 2.0, w_inner / 2.0),
        ((1.0 + outer) / 2.0, w_outer / 2.0),
    ]
}

/// Integral of `f` over one tetrahedron by the collapsed (Duffy) four-point Gauss rule per
/// axis. The integrands of the study are polynomials of degree four at most, and the collapse
/// raises the degree in the first coordinate by two, so the rule (degree seven) is exact. The
/// blueprint asks for degree `2p + 3 = 7` at `p = 2`. `f` receives the barycentric weights.
fn integrate_cell(corners: [[f64; 3]; 4], f: impl Fn([f64; 4], [f64; 3]) -> f64) -> f64 {
    let e = |i: usize| std::array::from_fn::<f64, 3, _>(|a| corners[i][a] - corners[0][a]);
    let (a, b, c) = (e(1), e(2), e(3));
    let six_volume = (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0]))
        .abs();
    let rule = gauss_legendre_4();
    let mut sum = 0.0;
    for (u, wu) in rule {
        for (v, wv) in rule {
            for (w, ww) in rule {
                let (l1, l2, l3) = (u, v * (1.0 - u), w * (1.0 - u) * (1.0 - v));
                let weights = [1.0 - l1 - l2 - l3, l1, l2, l3];
                let point = std::array::from_fn(|axis| {
                    (0..4).map(|i| weights[i] * corners[i][axis]).sum::<f64>()
                });
                let jacobian = (1.0 - u) * (1.0 - u) * (1.0 - v);
                sum += wu * wv * ww * jacobian * f(weights, point);
            }
        }
    }
    six_volume * sum
}

/// One solved level of a study.
struct Level {
    divisions: usize,
    spacing_m: f64,
    /// L2 norm of the temperature error in K·m^(3/2).
    l2_error: f64,
    /// Energy norm `sqrt(integral of |q - q_h|^2 / k)` of the error in sqrt(W·K).
    energy_error: f64,
    /// The same two norms from the solve with the tolerance divided by `TOLERANCE_DIVISOR`.
    tight_errors: [f64; 2],
}

fn solve_level(divisions: usize, source: f64, tolerance: f64) -> (RunReport, Vec<[usize; 4]>) {
    let mut model = example();
    model.linear_solver.relative_residual_tolerance = tolerance;
    let case = heat_case(&mut model);
    case.mesh = perturbed_block(divisions);
    case.thermal.volumetric_heat_source_w_m3 = source;
    case.thermal.boundary_conditions = vec![temperature("x0", T_AT_X0), temperature("x1", T_AT_X1)];
    let cells = case.mesh.cells.clone();
    let report = solve(&model).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    (report, cells)
}

/// The two error norms of a solved level against the analytical profile.
fn error_norms(report: &RunReport, cells: &[[usize; 4]], source: f64) -> [f64; 2] {
    let (nodes, values) = temperatures(report);
    let (_, q) = fluxes(report);
    let (mut l2_squared, mut energy_squared) = (0.0, 0.0);
    for (&cell, &q_h) in cells.iter().zip(q) {
        let corners = cell.map(|i| nodes[i]);
        let t_h = cell.map(|i| values[i]);
        l2_squared += integrate_cell(corners, |weights, point| {
            (interpolate(t_h, weights) - exact_temperature(point[0], source)).powi(2)
        });
        energy_squared += integrate_cell(corners, |_, point| {
            ((q_h[0] - exact_flux_x(point[0], source)).powi(2) + q_h[1].powi(2) + q_h[2].powi(2))
                / K
        });
    }
    [l2_squared.sqrt(), energy_squared.sqrt()]
}

fn run_study(source: f64) -> Vec<Level> {
    LEVELS
        .into_iter()
        .map(|divisions| {
            let (report, cells) = solve_level(divisions, source, STUDY_TOLERANCE);
            let [l2_error, energy_error] = error_norms(&report, &cells, source);
            if source != 0.0 {
                // The balance of the source case, kept from the earlier study: generation and
                // outflow equal `Q V` and the imbalance vanishes.
                let generation = source * LENGTH.powi(3);
                let outgoing = -["x0", "x1"]
                    .map(|name| region(&report, Output::BoundaryHeatRate, name))
                    .iter()
                    .sum::<f64>();
                close(
                    report.balances[0].terms["volumetric_generation"],
                    generation,
                    SOURCE_BALANCE_TOL_W,
                );
                close(outgoing, generation, SOURCE_BALANCE_TOL_W);
                close(report.balances[0].imbalance, 0.0, SOURCE_BALANCE_TOL_W);
            }
            let (tight, cells) = solve_level(
                divisions,
                source,
                STUDY_TOLERANCE / verify::TOLERANCE_DIVISOR,
            );
            Level {
                divisions,
                spacing_m: LENGTH / divisions as f64,
                l2_error,
                energy_error,
                tight_errors: error_norms(&tight, &cells, source),
            }
        })
        .collect()
}

/// Spacings and errors of one norm, coarse to fine, from the study or the check solve.
fn norm_series(levels: &[Level], norm: usize, tight: bool) -> (Vec<f64>, Vec<f64>) {
    let h = levels.iter().map(|l| l.spacing_m).collect();
    let e = levels
        .iter()
        .map(|l| match (norm, tight) {
            (0, false) => l.l2_error,
            (_, false) => l.energy_error,
            (_, true) => l.tight_errors[norm],
        })
        .collect();
    (h, e)
}

/// Outcome of one norm of a study: the band rule and every check that failed, empty when the
/// study passes.
struct Verdict {
    band: Option<verify::BandCheck>,
    failures: Vec<String>,
}

/// Judge the series of one norm against the band rule of blueprint 11.2
/// (`verify::band_check`), the monotone sequence and the algebraic budget. An input that the
/// rule cannot take counts as a failure.
fn judge(levels: &[Level], norm: usize, expected: f64) -> Verdict {
    assert!(
        levels.len() >= verify::MIN_STUDY_LEVELS,
        "SPEC-VER-003 asks for four levels"
    );
    let (h, e) = norm_series(levels, norm, false);
    let mut failures = Vec::new();
    let band = match verify::band_check(&h, &e, expected) {
        Ok(band) => {
            failures.extend(band.failure());
            Some(band)
        }
        Err(error) => {
            failures.push(format!("no band check: {error}"));
            None
        }
    };
    // Errors are listed fine to coarse for the classification.
    let fine_to_coarse: Vec<f64> = e.iter().rev().copied().collect();
    match verify::classify(&fine_to_coarse) {
        Ok(verify::Behaviour::Monotone) => {}
        Ok(other) => failures.push(format!("error sequence is {other}, not monotone")),
        Err(error) => failures.push(format!("error sequence not classified: {error}")),
    }
    let ratio = h[0] / h[1];
    for (level, (&error, &tight)) in levels
        .iter()
        .zip(e.iter().zip(&norm_series(levels, norm, true).1))
    {
        match verify::algebraic_check(error, tight, ratio) {
            Ok(check) if check.within_budget() => {}
            Ok(check) => failures.push(format!(
                "{} divisions: {}",
                level.divisions,
                check.warning().unwrap()
            )),
            Err(error) => failures.push(format!(
                "{} divisions: algebraic check failed: {error}",
                level.divisions
            )),
        }
    }
    Verdict { band, failures }
}

/// The study with the source, run once and shared by the tests that need it.
fn reference_study() -> &'static [Level] {
    static STUDY: std::sync::OnceLock<Vec<Level>> = std::sync::OnceLock::new();
    STUDY.get_or_init(|| run_study(SOURCE))
}

#[test]
fn uniform_source_converges_inside_elements_and_leaves_the_solid() {
    let levels = reference_study();
    let budget = verify::algebraic_error_budget(RATIO).unwrap();
    eprintln!("Source study, perturbed levels (amplitude {PERTURBATION} h), r = {RATIO}");
    eprintln!("| n | h (m) | L2 error (K m^1.5) | energy error (sqrt(W K)) |");
    for level in levels {
        eprintln!(
            "| {} | {:.6} | {:.6e} | {:.6e} |",
            level.divisions, level.spacing_m, level.l2_error, level.energy_error
        );
    }
    eprintln!("Algebraic budget delta = {budget:.6e}");
    for (norm, name, expected) in [(0, "L2", L2_ORDER), (1, "energy", ENERGY_ORDER)] {
        let (_, e) = norm_series(levels, norm, false);
        let verdict = judge(levels, norm, expected);
        let band = verdict
            .band
            .as_ref()
            .expect("the study has a valid band input");
        eprintln!(
            "{name}: pairwise orders {:.6?}, fitted order (finest three) {:.6}",
            band.pairwise_orders, band.fitted_order
        );
        eprintln!(
            "{name}: spread s {:.6}, half-width {:.6}, band [{:.6}, {:.6}], shrink factor rho {:.4}",
            band.spread, band.half_width, band.band[0], band.band[1], band.contraction
        );
        for (level, (&error, &tight)) in levels
            .iter()
            .zip(e.iter().zip(&norm_series(levels, norm, true).1))
        {
            let check = verify::algebraic_check(error, tight, RATIO).unwrap();
            eprintln!(
                "{name}, {} divisions: relative change {:.3e} (budget {:.3e})",
                level.divisions, check.relative_change, check.budget
            );
        }
        assert!(
            verdict.failures.is_empty(),
            "{name}: {:?}",
            verdict.failures
        );
    }
}

#[test]
fn the_source_study_rejects_the_exact_linear_solution() {
    // Without the source the exact solution is the linear profile, which linear tetrahedra
    // represent exactly: the errors are solver and rounding noise, not an order. The rule is
    // the one of the study (`verify::band_check`); its band comes from the trap's own spread.
    let trap = run_study(0.0);
    eprintln!("Trap study without the source");
    eprintln!("| n | L2 error (K m^1.5) | energy error (sqrt(W K)) |");
    for level in &trap {
        eprintln!(
            "| {} | {:.6e} | {:.6e} |",
            level.divisions, level.l2_error, level.energy_error
        );
    }
    for (norm, name, expected) in [(0, "L2", L2_ORDER), (1, "energy", ENERGY_ORDER)] {
        let verdict = judge(&trap, norm, expected);
        eprintln!("{name} trap: {:?}", verdict.failures);
        assert!(
            verdict.failures.iter().any(|f| f.contains("fitted order")),
            "{name}: the band must reject the exact case, got {:?}",
            verdict.failures
        );
    }
}

#[test]
fn validation_rejects_ambiguous_or_unphysical_thermal_inputs() {
    for (conditions, field, message) in [
        (vec![], "", "determine absolute temperature"),
        (
            vec![
                ThermalBoundary::Insulation {
                    region: "x0".to_owned(),
                },
                ThermalBoundary::HeatFlux {
                    region: "x1".to_owned(),
                    inward_heat_flux_w_m2: 5.0,
                },
            ],
            "",
            "determine absolute temperature",
        ),
        (
            vec![temperature("missing", 300.0)],
            "[0].region",
            "unknown exterior region",
        ),
        (
            vec![temperature("x0", 300.0), temperature("x0", 400.0)],
            "[1].region",
            "already has a thermal boundary condition",
        ),
        (
            vec![temperature("x0", 300.0), temperature("y0", 400.0)],
            "[1].temperature_k",
            "conflicting prescribed temperatures at vertex",
        ),
        (
            vec![temperature("x0", 0.0)],
            "[0].temperature_k",
            "must be positive and finite",
        ),
        (
            vec![ThermalBoundary::Convection {
                region: "x1".to_owned(),
                ambient_temperature_k: 300.0,
                film_coefficient_w_m2_k: 0.0,
            }],
            "[0].film_coefficient_w_m2_k",
            "must be positive and finite",
        ),
    ] {
        let mut model = example();
        heat_case(&mut model).thermal.boundary_conditions = conditions;
        let data = serde_json::to_vec(&model).unwrap();
        for error in [
            Model::from_json(&data).unwrap_err(),
            model.validate().unwrap_err(),
            solve(&model).unwrap_err(),
        ] {
            invalid_at(
                error,
                &format!("physics.thermal.boundary_conditions{field}"),
                message,
            );
        }
    }
    let mut model = example();
    heat_case(&mut model)
        .thermal
        .material
        .thermal_conductivity_w_m_k = -1.0;
    invalid_at(
        model.validate().unwrap_err(),
        "physics.thermal.material.thermal_conductivity_w_m_k",
        "must be positive and finite",
    );
}
