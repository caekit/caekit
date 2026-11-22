//! Thermoelastic solid: free and restrained thermal expansion, loads resolved at the scale
//! of the external load, refinement under a temperature gradient, combined mechanical loads,
//! recovery of plain mechanics, and stage traces at an iteration limit.

// Reference values in tests are independent of the library decision and result paths.
#![allow(clippy::disallowed_methods)]

#[path = "common/block.rs"]
mod block;
mod common;

use caekit::physics::elastic_solid::{SolidBoundary, SolidMechanics};
use caekit::physics::heat_conduction::ThermalBoundary;
use caekit::physics::thermoelastic_solid::ThermoelasticSolid;
use caekit::{
    FaceRegion, Model, Output, Physics, Quantity, RunReport, SampleLocation, StopReason, TetraMesh,
    solve,
};
use caekit_numerics::verify;
use caekit_types::Pressure;

use block::{TETRA_SAMPLES, interpolate, structured_block};
use common::{close, invalid_at, region, scalar};

const EXPANSION: f64 = 12.0e-6 * 50.0;

// Allowances of the checks on the fixture and the smaller cases. Linear tetrahedra represent the
// uniform and affine fields of these cases exactly, so the solution differs from the analytical
// one only by the linear solves, which stop at the default relative residual 1e-10 of
// docs/model.md, and by binary64 rounding. The free expansion of the 1 m bar at 50 K is
// `EXPANSION` = 6e-4 m; held at 120 MPa it gives 12 kN and 3.6 J. "Measured" is the largest
// |actual - expected| over the sites that use the constant (print it in `common::close` and run
// `cargo test -p caekit --test thermoelastic_solid -- --nocapture --test-threads=1`).

/// Mechanical absolute residual tolerance of the restraint test in N, an input. The stage
/// threshold is the larger of it and the relative part (docs/model.md), so it is at least this.
const MECHANICAL_ABSOLUTE_TOLERANCE_N: f64 = 1.0e-8;
/// Thermal absolute residual tolerance of the restraint test in W, an input. The thermal stage
/// threshold of that test equals it.
const THERMAL_ABSOLUTE_TOLERANCE_W: f64 = 1.0e-12;
/// Relative residual tolerance in the loose solve of the thin bar, an input. The default 1e-10
/// does not converge on this bar (the test takes its else branch today: no quantities and a
/// diagnostic that names the mean ratio); 1e-7 does.
const THIN_BAR_RELATIVE_TOLERANCE: f64 = 1.0e-7;
/// Axial stress of the 1 Pa traction case in Pa: 1e-8 relative. Measured 1.7e-13 Pa.
const TRACTION_STRESS_TOL_PA: f64 = 1e-8;
/// The same stress on the thin bar at the default tolerance in Pa: 1e-9 relative, 10 times the
/// default relative residual tolerance 1e-10, the rule of [`LOOSE_STRESS_TOL_PA`] applied to the
/// default solve. Not exercised by the current build, which takes the `numerical_breakdown`
/// branch on native and WASI alike, so no measurement exists. The earlier value 1e-7 followed no
/// rule and was tightened to this one; if a future solver converges here and misses it,
/// investigate the conditioning of the thin bar rather than widen it.
const THIN_BAR_STRESS_TOL_PA: f64 = 1e-9;
/// The same stress at the loose tolerance in Pa: 1e-6 relative, 10 times
/// [`THIN_BAR_RELATIVE_TOLERANCE`]. Measured 3.0e-9 Pa.
const LOOSE_STRESS_TOL_PA: f64 = 1e-6;
/// Reaction of the thin bar at the loose tolerance in N: 1e-6 relative to its 1e-8 N. Measured
/// 3.0e-17 N.
const LOOSE_REACTION_TOL_N: f64 = 1e-14;
/// Nodal displacement of the free and restrained bars in m: 8e-9 of the 6e-4 m expansion that the
/// restraint cancels and 6e-7 of the 7.8e-6 m lateral displacement. Measured 3.4e-21 m.
const EXPANSION_DISPLACEMENT_TOL_M: f64 = 5.0e-12;
/// Stress components in Pa: 4e-10 of the 120 MPa restrained stress and 6.7e-10 of the 75 MPa
/// hydrostatic stress of the single cell; components expected to vanish use the same allowance.
/// Measured 8.9e-8 Pa for the restrained bar, 1.5e-8 Pa for the single cell and 0 for the free
/// expansion.
const STRESS_TOL_PA: f64 = 0.05;
/// Support reactions, end forces and force imbalances in N: 4e-10 of the 12 kN restrained force.
/// `balance_bounds.rs` bounds the force imbalances of this fixture by 8.9e-10 N (its
/// `--nocapture` output), 5.6e3 times below. Measured 2.2e-10 N.
const FORCE_TOL_N: f64 = 5.0e-6;
/// Thermal energy imbalance in W, the allowance of the affine conduction cases. Measured 0: the
/// true residual of the single free equation is zero (`balance_bounds.rs` output).
const THERMAL_BALANCE_TOL_W: f64 = 1.0e-8;
/// Elastic energy of the free bar in J: 3e-11 of the 3.6 J of the restrained bar. Measured 0.
const FREE_ENERGY_TOL_J: f64 = 1.0e-10;
/// Elastic energy of the restrained bar in J: 2.8e-9 of 3.6 J. Measured 8.9e-16 J.
const RESTRAINED_ENERGY_TOL_J: f64 = 1.0e-8;
/// Nodal temperature of the gradient cases in K: 4 times 5e-9 K, the relative 1e-10 times the
/// 50 K span. Measured 2.1e-9 K.
const GRADIENT_TEMPERATURE_TOL_K: f64 = 2.0e-8;
/// Rounding of the quadrature rule check relative to the exact integral, floored at 1e-3 so that
/// small integrals get an absolute 1e-18: 4.5 eps. The largest of the six monomials is the
/// constant one, whose sum of 125 weights is 4 ulp of 1/6 off. Measured 6.7e-16, so the margin is
/// only 1.5.
const QUADRATURE_RELATIVE_TOL: f64 = 1.0e-15;
/// Floor of the exact integral in the quadrature check, so that the smaller monomial integrals
/// (down to 6.6e-6) are compared with an absolute 1e-18 instead of a relative one that rounding of
/// a few ulp would exceed.
const QUADRATURE_FLOOR: f64 = 1.0e-3;
/// Newton step at which the Gauss-Legendre nodes count as found: below one ulp of a root in
/// [0.5, 1) (1.1e-16). The 100 iterations allowed are far more than needed: 5 at most.
const NEWTON_STEP_TOL: f64 = 1.0e-16;
/// Error of the uniform-temperature trap relative to the error of the finest real level: the
/// trap errors are rounding and solver noise. Measured 3.8e-9 (L2) and 1.5e-9 (H1), so 1e-6 is
/// 260 times above.
const TRAP_ERROR_FRACTION: f64 = 1.0e-6;
/// Smallest ratio of the interior RMS displacement errors of successive uniform levels of 2, 4
/// and 8 divisions. Second order gives 4 and first order 2; 2.7 is an observed order of 1.43
/// (2^1.43), between them. Measured 3.67 and 3.82.
const GRADIENT_RMS_RATIO_MIN: f64 = 2.7;
/// Displacement at the internal sections and interior RMS error of the 8-division level in m:
/// 0.9% of the 557 um tip displacement that the profile reaches (docs/physics/thermoelastic-solid.md).
/// Measured 8.6e-9 m at the sections and 1.7e-6 m for the RMS error, which is the
/// discretization error of 8 divisions.
const GRADIENT_SECTION_TOL_M: f64 = 5.0e-6;
/// Heat rates of the single restrained cell in W: 2e-11 of the 500 W. Measured 1.1e-13 W.
const SINGLE_CELL_POWER_TOL_W: f64 = 1.0e-8;
/// Elastic energy of the single restrained cell in J: 2.2e-12 of its 4500 J. Measured 2.7e-12 J.
const SINGLE_CELL_ENERGY_TOL_J: f64 = 1.0e-8;
/// Displacement of the bar under pressure, body force and expansion in m: 8e-5 of the 6e-4 m
/// expansion and 4% of the 1.25e-6 m load displacement. It covers the discretization error of
/// 4 divisions: measured 9.3e-9 m, unchanged to 4 digits when the solve tolerance falls from 1e-10
/// to 1e-12 (scratch probe), so it is no solver error.
const LOADED_DISPLACEMENT_TOL_M: f64 = 5.0e-8;
/// Applied force term of that case in N: 1e-10 of 1e4 N. Measured 7.3e-12 N, 4 ulp.
const LOADED_APPLIED_TOL_N: f64 = 1.0e-6;
/// Reaction of that case in N: 1e-8 of 1e4 N. Measured 4.0e-10 N.
const LOADED_REACTION_TOL_N: f64 = 1.0e-4;

fn example() -> Model {
    Model::from_json(include_bytes!("../../../examples/thermoelastic_solid.json")).unwrap()
}

fn coupled_case(model: &mut Model) -> &mut ThermoelasticSolid {
    let Physics::ThermoelasticSolid(case) = &mut model.physics else {
        panic!("expected thermoelastic solid");
    };
    case
}

fn displacement(region: &str, components_m: [Option<f64>; 3]) -> SolidBoundary {
    SolidBoundary::Displacement {
        region: region.to_owned(),
        components_m,
    }
}

fn temperature(region: &str, temperature_k: f64) -> ThermalBoundary {
    ThermalBoundary::Temperature {
        region: region.to_owned(),
        temperature_k,
    }
}

fn nodal_vector<'a>(
    report: &'a RunReport,
    output: Output,
    expected_unit: &str,
) -> (&'a [[f64; 3]], &'a [[f64; 3]]) {
    let Quantity::VectorField {
        unit,
        location: SampleLocation::Node,
        positions_m,
        values,
    } = &report.quantities[&output]
    else {
        panic!("expected nodal vector");
    };
    assert_eq!(unit, expected_unit);
    (positions_m, values)
}

fn stresses(report: &RunReport) -> &[[[f64; 3]; 3]] {
    let Quantity::TensorField {
        unit,
        location: SampleLocation::Cell,
        values,
        ..
    } = &report.quantities[&Output::CauchyStress]
    else {
        panic!("expected centroid stress tensor");
    };
    assert_eq!(unit, "Pa");
    values
}

fn close_tensor(actual: &[[f64; 3]; 3], expected: [[f64; 3]; 3], tolerance: f64) {
    for (&actual, expected) in actual.iter().flatten().zip(expected.into_iter().flatten()) {
        close(actual, expected, tolerance);
    }
}

fn assert_converged(report: &RunReport) {
    assert_eq!(report.stop_reason, StopReason::Converged);
    let [thermal, mechanical] = report.convergence.as_slice() else {
        panic!("expected thermal and mechanical stages");
    };
    for (stage, name, unit) in [
        (thermal, "solid_heat_conduction", "W"),
        (mechanical, "solid_elastic_equilibrium", "N"),
    ] {
        assert_eq!(stage.stage, name);
        assert_eq!(stage.residual_unit, unit);
        assert_eq!(stage.stop_reason, StopReason::Converged);
        assert!(stage.final_true_residual_l2.unwrap() <= stage.residual_threshold.unwrap());
    }
}

#[test]
fn direct_mechanics_api_checks_absolute_temperature_before_any_shortcut() {
    let mut model = example();
    let case = coupled_case(&mut model);
    let checked = case.mesh.checked("mesh").unwrap();
    let solid = case.mechanical.prepare(&checked).unwrap();
    for alpha in [case.expansion.linear_expansion_coefficient_per_k, 0.0] {
        let mut expansion = case.expansion;
        expansion.linear_expansion_coefficient_per_k = alpha;
        for (datum, offset) in [
            (300.0, -301.0),
            (300.0, -300.0),
            (f64::MAX, f64::MAX),
            (300.0, f64::NAN),
        ] {
            let mut offsets = vec![0.0; case.mesh.vertices_m.len()];
            offsets[2] = offset;
            let err = solid
                .solve_thermoelastic(Default::default(), &[], expansion, datum, &offsets)
                .err()
                .expect("invalid absolute temperature must fail");
            invalid_at(err, "solid temperature at vertex 2", "positive and finite");
        }
    }
}

#[test]
fn small_axial_load_is_resolved_after_removing_uniform_free_expansion() {
    let mut model = example();
    let case = coupled_case(&mut model);
    case.mechanical.boundary_conditions.pop();
    case.mechanical
        .boundary_conditions
        .push(SolidBoundary::Traction {
            region: "x1".into(),
            components_pa: [1.0, 0.0, 0.0],
        });
    let report = solve(&model).unwrap();
    assert_converged(&report);
    for tensor in stresses(&report) {
        close(tensor[0][0], 1.0, TRACTION_STRESS_TOL_PA);
    }

    // The fixture becomes a 1 m long bar with a 0.1 mm square section.
    for point in &mut coupled_case(&mut model).mesh.vertices_m {
        point[1] *= 0.01;
        point[2] *= 0.01;
    }
    let report = solve(&model).unwrap();
    if report.stop_reason == StopReason::Converged {
        for tensor in stresses(&report) {
            close(tensor[0][0], 1.0, THIN_BAR_STRESS_TOL_PA);
        }
    } else {
        assert!(report.quantities.is_empty());
        assert!(
            report.convergence[1]
                .diagnostic
                .as_ref()
                .unwrap()
                .contains("mean ratio")
        );
    }
    model.linear_solver.relative_residual_tolerance = THIN_BAR_RELATIVE_TOLERANCE;
    let report = solve(&model).unwrap();
    assert_converged(&report);
    for tensor in stresses(&report) {
        close(tensor[0][0], 1.0, LOOSE_STRESS_TOL_PA);
    }
    let balance = report
        .balances
        .iter()
        .find(|b| b.name == "solid_force_x")
        .unwrap();
    close(balance.terms["reaction"], -1e-8, LOOSE_REACTION_TOL_N);
}

#[test]
fn coupled_solve_checks_reactions_at_the_scale_of_the_external_load() {
    let mut model = example();
    let case = coupled_case(&mut model);
    case.mechanical.boundary_conditions.pop();
    case.mechanical
        .boundary_conditions
        .push(displacement("y1", [None, Some(0.0), None]));
    case.mechanical
        .boundary_conditions
        .push(SolidBoundary::Traction {
            region: "x1".into(),
            components_pa: [1.0, 0.0, 0.0],
        });
    for point in &mut case.mesh.vertices_m {
        point[1] *= 0.01;
        point[2] *= 0.01;
    }
    let report = solve(&model).unwrap();
    let trace = &report.convergence[1];
    assert_eq!(trace.stop_reason, StopReason::NumericalBreakdown);
    assert!(trace.final_true_residual_l2.unwrap() <= trace.residual_threshold.unwrap());
    let diagnostic = trace.diagnostic.as_ref().unwrap();
    assert!(
        diagnostic.contains("external-load balance tolerance") && diagnostic.contains("mean ratio")
    );
    assert!(report.quantities.is_empty() && report.balances.is_empty());
}

#[test]
fn free_expansion_uses_a_temperature_field_even_without_thermal_outputs() {
    let mut model = example();
    coupled_case(&mut model)
        .mechanical
        .boundary_conditions
        .pop();
    model.outputs.retain(|output| {
        !matches!(
            output,
            Output::SolidTemperature | Output::SolidHeatFlux | Output::BoundaryHeatRate
        )
    });
    let report = solve(&model).unwrap();
    assert_converged(&report);
    let (nodes, values) = nodal_vector(&report, Output::DisplacementVector, "m");
    for (&point, u) in nodes.iter().zip(values) {
        for (&measured, expected) in u.iter().zip(point.map(|x| EXPANSION * x)) {
            close(measured, expected, EXPANSION_DISPLACEMENT_TOL_M);
        }
    }
    for stress in stresses(&report) {
        close_tensor(stress, [[0.0; 3]; 3], STRESS_TOL_PA);
    }
    for &value in nodal_vector(&report, Output::NodalReaction, "N")
        .1
        .iter()
        .flatten()
    {
        close(value, 0.0, FORCE_TOL_N);
    }
    close(
        scalar(&report, Output::ElasticEnergy),
        0.0,
        FREE_ENERGY_TOL_J,
    );
}

#[test]
fn axial_restraint_gives_uniaxial_compression_and_opposite_reactions() {
    let mut model = example();
    model.linear_solver.absolute_residual_tolerance = MECHANICAL_ABSOLUTE_TOLERANCE_N;
    coupled_case(&mut model)
        .thermal_solver
        .absolute_residual_tolerance = THERMAL_ABSOLUTE_TOLERANCE_W;
    let report = solve(&model).unwrap();
    assert_converged(&report);
    assert_eq!(
        report.convergence[0].residual_threshold,
        Some(THERMAL_ABSOLUTE_TOLERANCE_W)
    );
    assert!(report.convergence[1].residual_threshold.unwrap() >= MECHANICAL_ABSOLUTE_TOLERANCE_N);
    assert_eq!(report.geometry.len(), 1);
    assert_eq!(report.geometry[0].id, "restrained-thermal-bar");
    assert_eq!(report.meshes.len(), 1);
    let (nodes, values) = nodal_vector(&report, Output::DisplacementVector, "m");
    for (&[_, y, z], u) in nodes.iter().zip(values) {
        for (&measured, expected) in u
            .iter()
            .zip([0.0, 1.3 * EXPANSION * y, 1.3 * EXPANSION * z])
        {
            close(measured, expected, EXPANSION_DISPLACEMENT_TOL_M);
        }
    }
    for stress in stresses(&report) {
        close_tensor(
            stress,
            [[-120.0e6, 0.0, 0.0], [0.0; 3], [0.0; 3]],
            STRESS_TOL_PA,
        );
    }
    let (nodes, values) = nodal_vector(&report, Output::NodalReaction, "N");
    for (x, expected) in [(0.0, 12_000.0), (1.0, -12_000.0)] {
        let end_force: f64 = nodes
            .iter()
            .zip(values)
            .filter(|(node, _)| node[0] == x)
            .map(|(_, reaction)| reaction[0])
            .sum();
        close(end_force, expected, FORCE_TOL_N);
    }
    close(
        scalar(&report, Output::ElasticEnergy),
        3.6,
        RESTRAINED_ENERGY_TOL_J,
    );
    for balance in &report.balances {
        close(
            balance.imbalance,
            0.0,
            if balance.unit == "W" {
                THERMAL_BALANCE_TOL_W
            } else {
                FORCE_TOL_N
            },
        );
    }
}

// ---- Order study (SPEC-VER-002, SPEC-VER-003) ------------------------------------------------
//
// The exact solution, the levels, the band and the trap are derived in
// docs/physics/thermoelastic-solid.md; the numbers there come from this test.

/// Divisions per axis of the four self-similar levels; each halves the spacing (`r = 2`).
const STUDY_DIVISIONS: [usize; 4] = [2, 4, 8, 16];
const STUDY_RATIO: f64 = 2.0;
const STUDY_BLOCK_M: [f64; 3] = [1.0, 0.5, 0.5];
/// Temperature of the plane `x = 0` and reference temperature of the example, in K.
const STUDY_NEAR_K: f64 = 300.0;
/// Temperature of the plane `x = 1 m` with the gradient `g = 50 K/m`, in K.
const STUDY_FAR_K: f64 = 350.0;
/// Amplitude per axis, in m, of the smooth displacement that moves the interior vertices.
const STUDY_PERTURBATION_M: [f64; 3] = [0.04, 0.02, 0.02];
/// The band of `verify::band_check` is never allowed to be wider than the gap between
/// neighbouring formal orders. It only narrows the acceptance of the one rule of blueprint 11.2
/// (ruling R-9); it never widens the band.
///
/// The two norms have formal orders 2 and 1. A half-width of one half keeps the bands of the
/// two norms disjoint, so a study that measured the wrong norm or lost one order is rejected.
const STUDY_MAX_HALF_WIDTH: f64 = 0.5;

/// Block `STUDY_BLOCK_M` with a structured tetrahedral mesh whose interior vertices are moved
/// by a fixed smooth field that vanishes on the boundary.
///
/// Every level is the image of the uniform level under the same smooth map, so the family is
/// self-similar while its cells are non-uniform and not aligned with the solution. Boundary
/// vertices stay on their planes, so the regions and every boundary condition are unchanged.
fn perturbed_block(divisions: usize) -> TetraMesh {
    let mut mesh = structured_block(STUDY_BLOCK_M, divisions);
    for vertex in &mut mesh.vertices_m {
        if (0..3).any(|axis| vertex[axis] == 0.0 || vertex[axis] == STUDY_BLOCK_M[axis]) {
            continue;
        }
        let bump: f64 = (0..3)
            .map(|axis| (std::f64::consts::PI * vertex[axis] / STUDY_BLOCK_M[axis]).sin())
            .product();
        for axis in 0..3 {
            vertex[axis] += STUDY_PERTURBATION_M[axis] * bump;
        }
    }
    mesh
}

fn determinant(m: [[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn inverse(m: [[f64; 3]; 3], det: f64) -> [[f64; 3]; 3] {
    [
        [
            m[1][1] * m[2][2] - m[1][2] * m[2][1],
            m[0][2] * m[2][1] - m[0][1] * m[2][2],
            m[0][1] * m[1][2] - m[0][2] * m[1][1],
        ],
        [
            m[1][2] * m[2][0] - m[1][0] * m[2][2],
            m[0][0] * m[2][2] - m[0][2] * m[2][0],
            m[0][2] * m[1][0] - m[0][0] * m[1][2],
        ],
        [
            m[1][0] * m[2][1] - m[1][1] * m[2][0],
            m[0][1] * m[2][0] - m[0][0] * m[2][1],
            m[0][0] * m[1][1] - m[0][1] * m[1][0],
        ],
    ]
    .map(|row| row.map(|entry| entry / det))
}

/// Gauss-Legendre nodes and weights on `[0, 1]`, found by Newton iteration on `P_n`.
fn gauss_legendre(n: usize) -> Vec<(f64, f64)> {
    let legendre = |x: f64| {
        let (mut previous, mut current) = (1.0, x);
        for k in 2..=n {
            let k = k as f64;
            (previous, current) = (
                current,
                ((2.0 * k - 1.0) * x * current - (k - 1.0) * previous) / k,
            );
        }
        (current, n as f64 * (x * current - previous) / (x * x - 1.0))
    };
    (0..n)
        .map(|i| {
            let mut x = (std::f64::consts::PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
            for _ in 0..100 {
                let (p, dp) = legendre(x);
                let step = p / dp;
                x -= step;
                if step.abs() < NEWTON_STEP_TOL {
                    break;
                }
            }
            let (_, dp) = legendre(x);
            ((1.0 + x) / 2.0, 1.0 / ((1.0 - x * x) * dp * dp))
        })
        .collect()
}

/// Points and weights of a degree-seven rule on the reference tetrahedron (weights sum to 1/6).
///
/// Blueprint 11.2 asks for degree `2p + 3` with the largest formal order `p = 2` of the study.
/// The collapsed-coordinate map `x = u`, `y = v(1 - u)`, `z = w(1 - u)(1 - v)` multiplies a
/// degree-`d` integrand by a Jacobian of degree two in `u`, so `n` Gauss-Legendre points per
/// direction integrate degree `d` exactly when `d + 2 <= 2n - 1`: `n = 5` gives `d = 7`.
fn tetra_rule() -> Vec<([f64; 3], f64)> {
    let line = gauss_legendre(5);
    let mut rule = Vec::new();
    for &(u, wu) in &line {
        for &(v, wv) in &line {
            for &(w, ww) in &line {
                rule.push((
                    [u, v * (1.0 - u), w * (1.0 - u) * (1.0 - v)],
                    wu * wv * ww * (1.0 - u).powi(2) * (1.0 - v),
                ));
            }
        }
    }
    rule
}

#[test]
fn the_study_quadrature_integrates_degree_seven_monomials_exactly() {
    // Exact value of the integral of x^a y^b z^c over the reference tetrahedron:
    // a! b! c! / (a + b + c + 3)!.
    let factorial = |n: u32| (1..=n).map(f64::from).product::<f64>();
    let rule = tetra_rule();
    for (a, b, c) in [
        (0, 0, 0),
        (7, 0, 0),
        (3, 2, 2),
        (0, 4, 3),
        (2, 2, 3),
        (1, 1, 5),
    ] {
        let exact = factorial(a) * factorial(b) * factorial(c) / factorial(a + b + c + 3);
        let rule_value: f64 = rule
            .iter()
            .map(|(x, w)| w * x[0].powi(a as i32) * x[1].powi(b as i32) * x[2].powi(c as i32))
            .sum();
        assert!(
            (rule_value - exact).abs() <= QUADRATURE_RELATIVE_TOL * exact.max(QUADRATURE_FLOOR),
            "x^{a} y^{b} z^{c}: rule {rule_value:.17e}, exact {exact:.17e}"
        );
    }
}

#[test]
fn the_perturbed_family_keeps_every_cell_orientation_and_the_boundary_planes() {
    for divisions in STUDY_DIVISIONS {
        let uniform = structured_block(STUDY_BLOCK_M, divisions);
        let moved = perturbed_block(divisions);
        let signed_volume = |mesh: &TetraMesh, cell: [usize; 4]| {
            let p = cell.map(|i| mesh.vertices_m[i]);
            determinant(std::array::from_fn(|row| {
                std::array::from_fn(|column| p[column + 1][row] - p[0][row])
            }))
        };
        for &cell in &uniform.cells {
            let before = signed_volume(&uniform, cell);
            let after = signed_volume(&moved, cell);
            assert!(before * after > 0.0, "cell {cell:?} changed orientation");
        }
        assert_eq!(uniform.cells, moved.cells);
        for (before, after) in uniform.vertices_m.iter().zip(&moved.vertices_m) {
            let on_boundary =
                (0..3).any(|axis| before[axis] == 0.0 || before[axis] == STUDY_BLOCK_M[axis]);
            assert_eq!(on_boundary, before == after);
        }
    }
}

/// Error norms of the displacement against the exact solution, over the whole block.
#[derive(Clone, Copy, Debug)]
struct Norms {
    /// `sqrt(integral |u_h - u|^2 dV)`, in m^(5/2).
    l2: f64,
    /// `sqrt(integral |grad(u_h - u)|_F^2 dV)`, in m^(3/2).
    h1: f64,
}

/// Exact displacement `u = (u_x(x), 0, 0)` and its derivative for the plane temperatures.
///
/// With sliding walls the lateral strains vanish and the free far end gives `sigma_xx = 0`,
/// so `eps_xx = (1 + nu)/(1 - nu) alpha (T - T_ref)` and `u_x` integrates it from `x = 0`.
fn exact_profile(near_k: f64, far_k: f64, alpha: f64, nu: f64) -> impl Fn(f64) -> (f64, f64) {
    let factor = (1.0 + nu) / (1.0 - nu) * alpha;
    let offset = near_k - 300.0;
    let gradient = (far_k - near_k) / STUDY_BLOCK_M[0];
    move |x| {
        (
            factor * (offset * x + gradient * x * x / 2.0),
            factor * (offset + gradient * x),
        )
    }
}

fn error_norms(
    mesh: &TetraMesh,
    displacement: &[[f64; 3]],
    exact: impl Fn(f64) -> (f64, f64),
) -> Norms {
    let rule = tetra_rule();
    let (mut l2, mut h1) = (0.0, 0.0);
    for &cell in &mesh.cells {
        let p = cell.map(|i| mesh.vertices_m[i]);
        let u = cell.map(|i| displacement[i]);
        // x = p0 + J xi, so d xi_i / d x_b = (J^-1)[i][b] and the gradient of the linear
        // interpolant is constant on the cell.
        let jacobian: [[f64; 3]; 3] =
            std::array::from_fn(|row| std::array::from_fn(|column| p[column + 1][row] - p[0][row]));
        let det = determinant(jacobian);
        let inverse = inverse(jacobian, det);
        let mut gradient = [[0.0; 3]; 3];
        for (a, row) in gradient.iter_mut().enumerate() {
            for (b, entry) in row.iter_mut().enumerate() {
                *entry = (0..3)
                    .map(|i| (u[i + 1][a] - u[0][a]) * inverse[i][b])
                    .sum();
            }
        }
        for &(xi, weight) in &rule {
            let barycentric = [1.0 - xi[0] - xi[1] - xi[2], xi[0], xi[1], xi[2]];
            let point: [f64; 3] =
                std::array::from_fn(|a| (0..4).map(|k| barycentric[k] * p[k][a]).sum());
            let value: [f64; 3] =
                std::array::from_fn(|a| (0..4).map(|k| barycentric[k] * u[k][a]).sum());
            let (exact_ux, exact_dux) = exact(point[0]);
            let error = [value[0] - exact_ux, value[1], value[2]];
            let mut gradient_error = gradient;
            gradient_error[0][0] -= exact_dux;
            let volume = weight * det.abs();
            l2 += volume * error.iter().map(|e| e * e).sum::<f64>();
            h1 += volume * gradient_error.iter().flatten().map(|e| e * e).sum::<f64>();
        }
    }
    Norms {
        l2: l2.sqrt(),
        h1: h1.sqrt(),
    }
}

/// Solves one level and measures its error norms.
///
/// `tolerance_divisor` divides the relative residual tolerance of both stages.
fn study_level(divisions: usize, near_k: f64, far_k: f64, tolerance_divisor: f64) -> Norms {
    let mut model = example();
    let nu = {
        let case = coupled_case(&mut model);
        case.mesh = perturbed_block(divisions);
        case.thermal.boundary_conditions =
            vec![temperature("x0", near_k), temperature("x1", far_k)];
        case.mechanical.boundary_conditions = vec![
            displacement("x0", [Some(0.0), None, None]),
            displacement("y0", [None, Some(0.0), None]),
            displacement("y1", [None, Some(0.0), None]),
            displacement("z0", [None, None, Some(0.0)]),
            displacement("z1", [None, None, Some(0.0)]),
        ];
        case.thermal_solver.relative_residual_tolerance /= tolerance_divisor;
        case.mechanical.material.poisson_ratio
    };
    model.linear_solver.relative_residual_tolerance /= tolerance_divisor;
    let alpha = coupled_case(&mut model)
        .expansion
        .linear_expansion_coefficient_per_k;
    let report = solve(&model).unwrap();
    assert_converged(&report);
    let (points, displacement) = nodal_vector(&report, Output::DisplacementVector, "m");
    let Quantity::Field {
        values: temperatures,
        ..
    } = &report.quantities[&Output::SolidTemperature]
    else {
        panic!("expected nodal temperatures");
    };
    // The linear temperature is in the discrete space, so the thermal stage adds no error.
    for (&[x, _, _], &t) in points.iter().zip(temperatures) {
        close(t, near_k + (far_k - near_k) * x, GRADIENT_TEMPERATURE_TOL_K);
    }
    let mesh = &coupled_case(&mut model).mesh;
    error_norms(mesh, displacement, exact_profile(near_k, far_k, alpha, nu))
}

/// A norm of the study: its name, its formal order and the way to pick it from the levels.
type NormStudy = (&'static str, f64, fn(&Norms) -> f64);

#[test]
fn a_temperature_gradient_drives_the_local_displacement_and_converges_with_refinement() {
    let spacings: Vec<f64> = STUDY_DIVISIONS
        .iter()
        .map(|&d| STUDY_BLOCK_M[0] / d as f64)
        .collect();
    assert!(STUDY_DIVISIONS.len() >= 4, "SPEC-VER-003 needs four levels");
    for pair in STUDY_DIVISIONS.windows(2) {
        assert_eq!(pair[1] as f64 / pair[0] as f64, STUDY_RATIO);
    }
    let levels: Vec<Norms> = STUDY_DIVISIONS
        .iter()
        .map(|&d| study_level(d, STUDY_NEAR_K, STUDY_FAR_K, 1.0))
        .collect();
    let tight: Vec<Norms> = STUDY_DIVISIONS
        .iter()
        .map(|&d| study_level(d, STUDY_NEAR_K, STUDY_FAR_K, verify::TOLERANCE_DIVISOR))
        .collect();

    let budget = verify::algebraic_error_budget(STUDY_RATIO).unwrap();
    eprintln!("algebraic error budget delta = dp ln r / 2 = {budget:.6e} (dp = 0.01, r = 2)");
    eprintln!(
        "divisions | h_m | cells | L2 error | L2 error, tol/100 | change | H1 error | H1 error, tol/100 | change"
    );
    for (i, &divisions) in STUDY_DIVISIONS.iter().enumerate() {
        let l2 = verify::algebraic_check(levels[i].l2, tight[i].l2, STUDY_RATIO).unwrap();
        let h1 = verify::algebraic_check(levels[i].h1, tight[i].h1, STUDY_RATIO).unwrap();
        eprintln!(
            "{divisions} | {:.6} | {} | {:.6e} | {:.6e} | {:.3e} | {:.6e} | {:.6e} | {:.3e}",
            spacings[i],
            6 * divisions.pow(3),
            levels[i].l2,
            tight[i].l2,
            l2.relative_change,
            levels[i].h1,
            tight[i].h1,
            h1.relative_change,
        );
        for check in [l2, h1] {
            assert!(check.within_budget(), "{}", check.warning().unwrap());
        }
    }

    let norms: [NormStudy; 2] = [("L2", 2.0, |n| n.l2), ("H1", 1.0, |n| n.h1)];
    // Trap: a uniform temperature has no gradient, so u = c alpha (T - T_ref) x is affine and
    // linear tetrahedra represent it exactly. The levels measure only roundoff and the
    // algebraic error, and the band of the real study must reject them.
    let trap: Vec<Norms> = STUDY_DIVISIONS
        .iter()
        .map(|&d| study_level(d, STUDY_FAR_K, STUDY_FAR_K, 1.0))
        .collect();
    for (i, level) in trap.iter().enumerate() {
        eprintln!(
            "trap (uniform {STUDY_FAR_K} K), {} divisions: L2 {:.6e}, H1 {:.6e}",
            STUDY_DIVISIONS[i], level.l2, level.h1
        );
    }

    for (name, formal, pick) in norms {
        let errors: Vec<f64> = levels.iter().map(pick).collect();
        let band = verify::band_check(&spacings, &errors, formal).unwrap();
        eprintln!(
            "{name}: fitted order {:.6}, pairwise {:.6?}, spread {:.6}, band {formal} +- {:.6} = [{:.6}, {:.6}], contraction {:.4}, finest distance {:.6}, all-levels fit {:.6}",
            band.fitted_order,
            band.pairwise_orders,
            band.spread,
            band.half_width,
            band.band[0],
            band.band[1],
            band.contraction,
            band.finest_distance,
            verify::fitted_order(&spacings, &errors).unwrap()
        );
        assert!(band.half_width <= STUDY_MAX_HALF_WIDTH);
        if let Some(failure) = band.failure() {
            panic!("{name}: {failure}");
        }
        // The same rule rejects the exactness case: its errors are roundoff, not a rate. A
        // pass of the rule on the trap's own errors, or a trap fit inside the band of the real
        // study, would let a model that the discretization reproduces exactly pass as an
        // order study.
        let trap_errors: Vec<f64> = trap.iter().map(pick).collect();
        let trap_band = verify::band_check(&spacings, &trap_errors, formal).unwrap();
        eprintln!(
            "{name} trap: fitted order {:.6}, spread {:.6}, own band {formal} +- {:.6}, contraction {:.4}, verdict {:?}",
            trap_band.fitted_order,
            trap_band.spread,
            trap_band.half_width,
            trap_band.contraction,
            trap_band.failure()
        );
        assert!(
            trap_band
                .failure()
                .is_some_and(|failure| failure.starts_with("fitted order")),
            "{name}: the band rule accepted the exact case"
        );
        assert!(
            (trap_band.fitted_order - formal).abs() > band.half_width,
            "{name}: the trap fit lies inside the band of the real study"
        );
        // The error of the trap is also far below the real discretization error.
        assert!(trap_errors[3] < TRAP_ERROR_FRACTION * errors[3]);
    }
}

#[test]
fn uniform_block_matches_the_gradient_displacement_at_the_interior_sections() {
    let nu = 0.3;
    let scale = (1.0 + nu) / (1.0 - nu) * EXPANSION / 2.0;
    let exact = |x: f64| scale * x.powi(2);
    let mut previous = None;
    for divisions in [2, 4, 8] {
        let mut model = example();
        let case = coupled_case(&mut model);
        case.mesh = structured_block([1.0, 0.5, 0.5], divisions);
        case.thermal.boundary_conditions = vec![temperature("x0", 300.0), temperature("x1", 350.0)];
        case.mechanical.boundary_conditions = vec![
            displacement("x0", [Some(0.0), None, None]),
            displacement("y0", [None, Some(0.0), None]),
            displacement("y1", [None, Some(0.0), None]),
            displacement("z0", [None, None, Some(0.0)]),
            displacement("z1", [None, None, Some(0.0)]),
        ];
        let report = solve(&model).unwrap();
        assert_converged(&report);
        let (points, displacement) = nodal_vector(&report, Output::DisplacementVector, "m");
        let Quantity::Field {
            values: temperature,
            ..
        } = &report.quantities[&Output::SolidTemperature]
        else {
            panic!("expected nodal temperatures");
        };
        for (&[x, _, _], &t) in points.iter().zip(temperature) {
            close(t, 300.0 + 50.0 * x, GRADIENT_TEMPERATURE_TOL_K);
        }
        let cells = &coupled_case(&mut model).mesh.cells;
        let mut squared_error = 0.0;
        for &cell in cells {
            for weights in TETRA_SAMPLES {
                let x = interpolate(cell.map(|i| points[i][0]), weights);
                let ux = interpolate(cell.map(|i| displacement[i][0]), weights);
                squared_error += (ux - exact(x)).powi(2);
            }
        }
        let rms = (squared_error / (cells.len() * TETRA_SAMPLES.len()) as f64).sqrt();
        eprintln!(
            "Temperature gradient, {divisions} divisions per axis: {rms:.8e} m interior displacement RMS error"
        );
        if let Some(coarse) = previous {
            assert!(rms < coarse / GRADIENT_RMS_RATIO_MIN);
        }
        previous = Some(rms);
        if divisions == 8 {
            for x in [0.25, 0.5, 0.75] {
                let node = points
                    .iter()
                    .position(|point| *point == [x, 0.25, 0.25])
                    .unwrap();
                close(displacement[node][0], exact(x), GRADIENT_SECTION_TOL_M);
            }
        }
    }
    assert!(previous.unwrap() < GRADIENT_SECTION_TOL_M);
}

#[test]
fn fully_restrained_tetrahedron_integrates_the_quadratic_thermal_energy() {
    let mut model = example();
    let case = coupled_case(&mut model);
    case.mesh = TetraMesh {
        vertices_m: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        cells: vec![[0, 1, 2, 3]],
        boundary_regions: [
            ("x0", [0, 2, 3]),
            ("y0", [0, 1, 3]),
            ("z0", [0, 1, 2]),
            ("slant", [1, 2, 3]),
        ]
        .into_iter()
        .map(|(region, face)| FaceRegion {
            name: region.to_owned(),
            faces: vec![face],
        })
        .collect(),
    };
    // For T = 300 + 50x and k = 20, the inward flux on the slant is 1000/sqrt(3).
    case.thermal.boundary_conditions = vec![
        temperature("x0", 300.0),
        ThermalBoundary::HeatFlux {
            region: "slant".to_owned(),
            inward_heat_flux_w_m2: 1_000.0 / 3.0_f64.sqrt(),
        },
    ];
    case.mechanical.boundary_conditions = vec![
        displacement("x0", [Some(0.0); 3]),
        displacement("slant", [Some(0.0); 3]),
    ];
    let report = solve(&model).unwrap();
    assert_converged(&report);
    close(
        region(&report, Output::BoundaryHeatRate, "slant"),
        500.0,
        SINGLE_CELL_POWER_TOL_W,
    );
    close(
        region(&report, Output::BoundaryHeatRate, "x0"),
        -500.0,
        SINGLE_CELL_POWER_TOL_W,
    );
    close(report.balances[0].imbalance, 0.0, SINGLE_CELL_POWER_TOL_W);
    // Integral of x^2 over the unit reference tetrahedron is 1/60.
    let hydrostatic_modulus = 200.0e9 / (1.0 - 2.0 * 0.3);
    let energy = 1.5 * hydrostatic_modulus * EXPANSION.powi(2) / 60.0;
    close(
        scalar(&report, Output::ElasticEnergy),
        energy,
        SINGLE_CELL_ENERGY_TOL_J,
    );
    let s = -hydrostatic_modulus * EXPANSION / 4.0;
    close_tensor(
        &stresses(&report)[0],
        [[s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]],
        STRESS_TOL_PA,
    );
}

#[test]
fn pressure_and_body_force_add_to_thermal_expansion() {
    let mut model = example();
    let case = coupled_case(&mut model);
    case.mesh = structured_block([1.0, 0.1, 0.1], 4);
    case.mechanical.material.poisson_ratio = 0.0;
    case.mechanical.body_force_n_m3 = [2.0e6, 0.0, 0.0];
    case.mechanical.boundary_conditions.pop();
    case.mechanical
        .boundary_conditions
        .push(SolidBoundary::Pressure {
            region: "x1".to_owned(),
            applied: Pressure::Absolute {
                value_pa: 1_101_325.0,
            },
            ambient: Pressure::Absolute {
                value_pa: 101_325.0,
            },
        });
    let report = solve(&model).unwrap();
    assert_converged(&report);
    let (nodes, values) = nodal_vector(&report, Output::DisplacementVector, "m");
    // For nu=0, sigma_xx = -p + b(L-x), so the load-induced tip extension is zero.
    for (&[x, _, _], u) in nodes.iter().zip(values) {
        let mechanical = (1.0e6 * x - 1.0e6 * x.powi(2)) / 200.0e9;
        close(u[0], EXPANSION * x + mechanical, LOADED_DISPLACEMENT_TOL_M);
    }
    let balance = report
        .balances
        .iter()
        .find(|balance| balance.name == "solid_force_x")
        .unwrap();
    close(balance.terms["applied"], 1.0e4, LOADED_APPLIED_TOL_N);
    close(balance.terms["reaction"], -1.0e4, LOADED_REACTION_TOL_N);
}

#[test]
fn zero_expansion_or_reference_temperature_recovers_legacy_mechanics() {
    let mut elastic =
        Model::from_json(include_bytes!("../../../examples/elastic_solid.json")).unwrap();
    let Physics::ElasticSolid(case) = &mut elastic.physics else {
        panic!("expected elastic solid");
    };
    case.body_force_n_m3 = [500.0, -1_000.0, 1_500.0];
    let mechanical = SolidMechanics {
        material: case.material.clone(),
        boundary_conditions: case.boundary_conditions.clone(),
        body_force_n_m3: case.body_force_n_m3,
    };
    let mut coupled = example();
    let combined = coupled_case(&mut coupled);
    combined.mesh = case.mesh.clone();
    combined.geometry = case.geometry.clone();
    combined.mechanical = mechanical;
    let elastic_report = solve(&elastic).unwrap();

    for zero_alpha in [false, true] {
        let combined = coupled_case(&mut coupled);
        combined.expansion.linear_expansion_coefficient_per_k =
            if zero_alpha { 0.0 } else { 12.0e-6 };
        combined.thermal.boundary_conditions = vec![temperature("x0", 300.0)];
        if zero_alpha {
            combined
                .thermal
                .boundary_conditions
                .push(temperature("x1", 350.0));
        }
        let report = solve(&coupled).unwrap();
        assert_converged(&report);
        for &output in &elastic.outputs {
            assert_eq!(
                serde_json::to_value(&report.quantities[&output]).unwrap(),
                serde_json::to_value(&elastic_report.quantities[&output]).unwrap()
            );
        }
        assert_eq!(
            serde_json::to_value(&report.balances[1..]).unwrap(),
            serde_json::to_value(&elastic_report.balances).unwrap()
        );
    }
}

#[test]
fn iteration_limits_preserve_only_the_executed_stage_traces() {
    let mut thermal_limit = example();
    let case = coupled_case(&mut thermal_limit);
    // One iteration cannot reach the 1e-10 relative threshold: the flux and convection conditions
    // leave the full temperature system to be solved.
    case.thermal_solver.max_iterations = 1;
    case.thermal.boundary_conditions = vec![
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
    let mut mechanical_limit = example();
    // One CG iteration cannot reach the 1e-10 relative threshold of the mechanical stage.
    mechanical_limit.linear_solver.max_iterations = 1;
    for (model, stages, unit) in [(thermal_limit, 1, "W"), (mechanical_limit, 2, "N")] {
        let report = solve(&model).unwrap();
        assert_eq!(report.stop_reason, StopReason::IterationLimit);
        assert!(report.quantities.is_empty());
        assert!(report.balances.is_empty());
        assert_eq!(report.convergence.len(), stages);
        for stage in &report.convergence[..stages - 1] {
            assert_eq!(stage.stop_reason, StopReason::Converged);
        }
        let stopped = report.convergence.last().unwrap();
        assert_eq!(stopped.stop_reason, StopReason::IterationLimit);
        assert_eq!(stopped.residual_unit, unit);
        assert_eq!(stopped.iterations, 1);
        assert!(stopped.final_true_residual_l2.unwrap() > stopped.residual_threshold.unwrap());
    }
}
