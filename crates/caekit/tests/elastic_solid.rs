//! Elastic solid: the three-dimensional patch test, loads resolved under an imposed rigid
//! translation, gauge pressure, body loads, diagnosed mesh and constraint defects, and a
//! refinement study of the Lamé thick cylinder in the L² and energy norms.

// The mesh generator and the exact solution of the refinement study use `f64::sin` and
// `f64::cos`. They build test inputs and reference values, not library decisions or result
// paths; the order fit itself goes through `caekit_numerics::verify`.
#![allow(clippy::disallowed_methods)]

mod common;

use caekit::physics::elastic_solid::{ElasticSolid, SolidBoundary};
use caekit::physics::laminar_channel::Pressure;
use caekit::{
    FaceRegion, Model, Output, Physics, Quantity, RunReport, StopReason, TetraMesh, solve,
};
use caekit_numerics::verify;
use common::{close, invalid_at, scalar};

const INPUT: &[u8] = include_bytes!("../../../examples/elastic_solid.json");

// Allowances of the checks on the 6-tetrahedron patch fixture: a 2 m bar of cross section 0.2 m^2
// and Young's modulus 200 GPa. Linear tetrahedra represent its uniform strain exactly, so the
// solution differs from the analytical one only by the linear solve, which stops at the default
// relative residual 1e-10 of docs/model.md, and by binary64 rounding. "Measured" is the largest
// |actual - expected| over the sites that use the constant (print it in `common::close` and run
// `cargo test -p caekit --test elastic_solid -- --nocapture --test-threads=1`).

/// Nodal displacement of the 1e7 Pa case in m: 2e-8 of the 5e-5 m end displacement. Measured
/// 1.5e-19 m.
const PATCH_DISPLACEMENT_TOL_M: f64 = 1.0e-12;
/// Cauchy and von Mises stress in Pa: 1e-9 of 1e7 Pa. Measured 1.5e-8 Pa.
const PATCH_STRESS_TOL_PA: f64 = 0.01;
/// Reaction term and force and moment imbalances in N (N m for moments): 5e-10 of the 2e6 N
/// reaction (1e7 Pa over 0.2 m^2). Measured 2.6e-9 N.
const PATCH_FORCE_TOL_N: f64 = 1.0e-3;
/// Elastic energy in J: 1e-8 of the 100 J of the 1e7 Pa case. Measured 2.7e-13 J.
const PATCH_ENERGY_TOL_J: f64 = 1.0e-6;
/// Admissible mean-ratio quality of the fixture cells. The mean ratio is 1 for a regular
/// tetrahedron, its maximum, and tends to 0 as a cell degenerates (docs/physics/elastic-solid.md).
/// The lower edge 0.1 only rejects a degenerate fixture: it is no quality requirement, and it
/// lies 2.4 times below the lowest value of the fixture, 0.2382 (cell 2; `cargo run --locked --
/// run examples/elastic_solid.json`, `meshes[0].quality`).
const MEAN_RATIO_RANGE: std::ops::RangeInclusive<f64> = 0.1..=1.0;
/// Displacement of the translated small-traction case in m: the load moves the bar by only
/// 5e-15 m on top of a translation of up to 4e-3 m, so the allowance is 2e-4 of the load's
/// displacement and 1 ulp (8.7e-19) of the translation. Measured 0.
const TRANSLATED_DISPLACEMENT_TOL_M: f64 = 1.0e-18;
/// Elastic energy of the translated small-traction case in J: 1e-8 of its 1e-18 J. Measured
/// 3.3e-33 J.
const TRANSLATED_ENERGY_TOL_J: f64 = 1.0e-26;
/// Reaction of the translated small-traction case in N: 5e-9 of its 2e-4 N. Measured 3.8e-19 N.
const TRANSLATED_REACTION_TOL_N: f64 = 1.0e-12;
/// Applied body-load terms in N and N m. Each is a sum of 6 products of the input -600 N/m^3 and
/// exact cell volumes, so the rounding bound of a recursive sum is `(m - 1) eps sum|v|` =
/// 2.7e-13 for the 240 N total (Higham, Accuracy and Stability of Numerical Algorithms, 2nd ed.,
/// section 4.2); 1e-9 is 3.7e3 times that. Measured 0.
const BODY_APPLIED_TOL_N: f64 = 1.0e-9;
/// Reaction and moment imbalance of the body-load case in N and N m: 4e-9 of the 240 N load.
/// Measured 5.7e-14.
const BODY_SOLVE_TOL_N: f64 = 1.0e-6;

fn elastic(model: &mut Model) -> &mut ElasticSolid {
    let Physics::ElasticSolid(case) = &mut model.physics else {
        panic!("expected the solid fixture");
    };
    case
}

fn balance<'a>(report: &'a RunReport, name: &str) -> &'a caekit::Balance {
    report
        .balances
        .iter()
        .find(|balance| balance.name == name)
        .unwrap()
}

fn check_affine(report: &RunReport, axial_stress: f64) {
    assert_eq!(report.stop_reason, StopReason::Converged);
    let Quantity::VectorField {
        positions_m,
        values,
        ..
    } = &report.quantities[&Output::DisplacementVector]
    else {
        panic!("expected a displacement field");
    };
    let strain = axial_stress / 200e9;
    for (&[x, y, z], &[ux, uy, uz]) in positions_m.iter().zip(values) {
        close(ux, strain * (x - 1.0), PATCH_DISPLACEMENT_TOL_M);
        close(uy, -0.31 * strain * (y - 2.0), PATCH_DISPLACEMENT_TOL_M);
        close(uz, -0.31 * strain * (z + 1.0), PATCH_DISPLACEMENT_TOL_M);
    }
    let Quantity::TensorField { values, .. } = &report.quantities[&Output::CauchyStress] else {
        panic!("expected a tensor stress field");
    };
    for tensor in values {
        for (i, row) in tensor.iter().enumerate() {
            for (j, component) in row.iter().enumerate() {
                let expected = if (i, j) == (0, 0) { axial_stress } else { 0.0 };
                close(*component, expected, PATCH_STRESS_TOL_PA);
            }
        }
    }
    let Quantity::Field { values, .. } = &report.quantities[&Output::VonMisesStress] else {
        panic!("expected a von Mises stress field");
    };
    for &stress in values {
        close(stress, axial_stress.abs(), PATCH_STRESS_TOL_PA);
    }
    close(
        balance(report, "solid_force_x").terms["reaction"],
        -axial_stress * 0.2,
        PATCH_FORCE_TOL_N,
    );
    for balance in &report.balances {
        close(balance.imbalance, 0.0, PATCH_FORCE_TOL_N);
    }
    // Strain-energy density is sigma_x * epsilon_x / 2, integrated over 0.4 m³.
    close(
        scalar(report, Output::ElasticEnergy),
        0.5 * axial_stress * strain * 0.4,
        PATCH_ENERGY_TOL_J,
    );
}

#[test]
fn completes_the_three_dimensional_patch_test() {
    let report = solve(&Model::from_json(INPUT).unwrap()).unwrap();
    assert_eq!(report.meshes[0].cell_count, 6);
    let quality = report.meshes[0].quality.as_ref().unwrap();
    assert!(MEAN_RATIO_RANGE.contains(&quality.minimum_mean_ratio));
    assert!(quality.cell_index < 6);
    check_affine(&report, 1.0e7);
}

#[test]
fn small_traction_is_resolved_under_an_imposed_rigid_translation() {
    let mut model = Model::from_json(INPUT).unwrap();
    let case = elastic(&mut model);
    let translation = [0.001, -0.002, 0.004];
    for boundary in &mut case.boundary_conditions {
        match boundary {
            SolidBoundary::Displacement { components_m, .. } => {
                for (component, shift) in components_m.iter_mut().zip(translation) {
                    if let Some(value) = component {
                        *value += shift;
                    }
                }
            }
            SolidBoundary::Traction { components_pa, .. } => *components_pa = [0.001, 0.0, 0.0],
            SolidBoundary::Pressure { .. } => unreachable!(),
        }
    }
    let report = solve(&model).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    // The exact uniaxial strain is traction / E = 5e-15.
    close(
        scalar(&report, Output::ElasticEnergy),
        1e-18,
        TRANSLATED_ENERGY_TOL_J,
    );
    close(
        balance(&report, "solid_force_x").terms["reaction"],
        -2e-4,
        TRANSLATED_REACTION_TOL_N,
    );
    let Quantity::VectorField {
        positions_m,
        values,
        ..
    } = &report.quantities[&Output::DisplacementVector]
    else {
        panic!("expected a displacement field");
    };
    for (&[x, y, z], &u) in positions_m.iter().zip(values) {
        let strain = 5e-15;
        close(
            u[0],
            translation[0] + strain * (x - 1.0),
            TRANSLATED_DISPLACEMENT_TOL_M,
        );
        close(
            u[1],
            translation[1] - 0.31 * strain * (y - 2.0),
            TRANSLATED_DISPLACEMENT_TOL_M,
        );
        close(
            u[2],
            translation[2] - 0.31 * strain * (z + 1.0),
            TRANSLATED_DISPLACEMENT_TOL_M,
        );
    }
}

#[test]
fn gauge_pressure_uses_outward_normals_and_ambient_reference() {
    let mut model = Model::from_json(INPUT).unwrap();
    let case = elastic(&mut model);
    *case.boundary_conditions.last_mut().unwrap() = SolidBoundary::Pressure {
        region: "x1".to_owned(),
        applied: Pressure::Gauge {
            value_pa: 1.0e7,
            reference_absolute_pa: 123_000.0,
        },
        ambient: Pressure::Absolute {
            value_pa: 123_000.0,
        },
    };
    check_affine(&solve(&model).unwrap(), -1.0e7);
}

#[test]
fn body_load_integrates_over_the_actual_volume() {
    let mut model = Model::from_json(INPUT).unwrap();
    let case = elastic(&mut model);
    case.boundary_conditions.pop();
    case.body_force_n_m3 = [0.0, -600.0, 0.0];
    let report = solve(&model).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    close(
        balance(&report, "solid_force_y").terms["applied"],
        -240.0,
        BODY_APPLIED_TOL_N,
    );
    close(
        balance(&report, "solid_force_y").terms["reaction"],
        240.0,
        BODY_SOLVE_TOL_N,
    );
    // The body's centroid is one metre from the origin along x.
    close(
        balance(&report, "solid_moment_z").terms["applied"],
        -240.0,
        BODY_APPLIED_TOL_N,
    );
    close(
        balance(&report, "solid_moment_z").imbalance,
        0.0,
        BODY_SOLVE_TOL_N,
    );
}

#[test]
fn diagnoses_inverted_cells_duplicate_faces_and_underconstrained_modes() {
    let mut model = Model::from_json(INPUT).unwrap();
    elastic(&mut model).mesh.cells[1].swap(0, 1);
    invalid_at(
        model.validate().unwrap_err(),
        "physics.mesh.cells[1]",
        "inverted",
    );

    let mut model = Model::from_json(INPUT).unwrap();
    let case = elastic(&mut model);
    let duplicate = case.mesh.boundary_regions[0].faces[0];
    case.mesh.boundary_regions[1].faces.push(duplicate);
    invalid_at(
        model.validate().unwrap_err(),
        "physics.mesh.boundary_regions[1].faces[2]",
        "already assigned face",
    );

    let mut model = Model::from_json(INPUT).unwrap();
    elastic(&mut model).boundary_conditions.truncate(1);
    invalid_at(
        model.validate().unwrap_err(),
        "physics.boundary_conditions",
        "six rigid-body modes",
    );
}

#[test]
fn rejects_stale_and_conflicting_region_constraints() {
    let mut model = Model::from_json(INPUT).unwrap();
    let case = elastic(&mut model);
    case.mesh.boundary_regions[0].name = "revised_x".to_owned();
    invalid_at(
        model.validate().unwrap_err(),
        "physics.boundary_conditions[0].region",
        "unknown exterior region",
    );

    let mut model = Model::from_json(INPUT).unwrap();
    elastic(&mut model)
        .boundary_conditions
        .push(SolidBoundary::Displacement {
            region: "y0".to_owned(),
            components_m: [Some(1.0e-6), None, None],
        });
    invalid_at(
        model.validate().unwrap_err(),
        "physics.boundary_conditions[4].components_m[0]",
        "conflicting displacement at vertex",
    );
}

// Refinement study (SPEC-VER-002, SPEC-VER-003). A quarter of a thick cylinder under internal
// pressure, in plane strain, has the exact Lamé displacement `u = (A r + B / r) e_r`. The `B / r`
// part is not a polynomial, so linear tetrahedra cannot represent it, and the study measures the
// whole field in the L² and energy norms instead of at nodes. The loads are schema version 1
// inputs only: one pressure region on the faceted inner surface, symmetry and plane-strain
// constraints as zero displacement components. The derivation, the levels and the measured
// numbers are on docs/physics/elastic-solid.md.

/// Inner radius `a` in m of the cylinder of the refinement study.
const INNER_RADIUS_M: f64 = 1.0;
/// Outer radius `b` in m. `b / a = 2` keeps the `B / r` part of the displacement comparable to
/// the `A r` part across the wall.
const OUTER_RADIUS_M: f64 = 2.0;
/// Young's modulus in Pa of the study's synthetic material.
const STUDY_YOUNG_MODULUS_PA: f64 = 200.0e9;
/// Poisson's ratio of the study's synthetic material.
const STUDY_POISSON_RATIO: f64 = 0.3;
/// Gauge pressure in Pa on the inner surface; a positive value pushes into the solid.
const INNER_PRESSURE_PA: f64 = 1.0e7;
/// Absolute reference of the gauge pressures in Pa; it cancels against the equal ambient.
const AMBIENT_PA: f64 = 101_325.0;
/// Radial divisions `n` of each level, fixed before the first run. The ratio is two, and four
/// levels are the fewest SPEC-VER-003 accepts. The coarsest level has four divisions, not
/// three, because next-steps.md flagged three as likely pre-asymptotic.
const LEVEL_DIVISIONS: [usize; 4] = [4, 8, 16, 32];
/// Refinement ratio between successive levels.
const STUDY_RATIO: f64 = 2.0;
/// Layers through the thickness. Two layers leave the middle-plane vertices free in z, so the
/// plane-strain state is solved, not imposed at every vertex.
const LAYERS: usize = 2;
/// Circumferential divisions per radial division. With two, the circumferential size of a cell
/// is 0.79 to 1.57 times its radial size before grading; with one it was 1.57 to 3.14 and the
/// lowest mean-ratio quality 0.31 instead of 0.58 (see the physics page).
const ANGULAR_FACTOR: usize = 2;
/// Strength `c` of the smooth radial grading `ξ ↦ ξ − c ξ (1 − ξ)`: cells near the inner
/// surface are 0.7 times and near the outer surface 1.3 times the mean radial size, at every
/// level.
const RADIAL_GRADING: f64 = 0.3;
/// Strength `c` of the smooth circumferential grading `η ↦ η + c sin(π η)`: the angular size of
/// a cell ranges from 1 − cπ to 1 + cπ times the mean, at every level.
const ANGULAR_GRADING: f64 = 0.1;
/// The fixture omits `linear_solver`, so the defaults of `docs/model.md` apply: a relative
/// tolerance of 1e-10 and no absolute tolerance. The check solve divides the relative one by
/// `verify::TOLERANCE_DIVISOR`.
const FIXTURE_RELATIVE_TOLERANCE: f64 = 1.0e-10;
/// Formal order of linear tetrahedra for a smooth solution in the L² norm of the displacement
/// (Ciarlet 2002, section 3.2).
const L2_ORDER: f64 = 2.0;
/// Formal order of linear tetrahedra for a smooth solution in the energy norm (Ciarlet 2002,
/// section 3.2).
const ENERGY_ORDER: f64 = 1.0;
/// Stated bound on the L² spread `s`, the difference of the two pairwise orders of the finest
/// three levels, measured by the command on the physics page and rounded up to the next 0.005.
/// It is never chosen from the fitted order. The band of `verify::band_check` uses the measured
/// spread; the study also fails when that spread exceeds this bound, so the band stays inside
/// the stated band `p_f ± (Δp + 1.5 · bound)` of the physics page.
const L2_SPREAD_BOUND: f64 = 0.025;
/// Stated bound on the energy spread, measured and rounded as [`L2_SPREAD_BOUND`].
const ENERGY_SPREAD_BOUND: f64 = 0.015;
/// Grundmann–Möller parameter of the error integrals: `s = 3` integrates polynomials of degree
/// `2 s + 1 = 7` exactly, above the degree `2 p + 3 = 5` of blueprint 11.2 for `p = 1`.
const ERROR_RULE: usize = 3;
/// The degree-5 rule (`s = 2`) whose results bound the quadrature error of [`ERROR_RULE`].
const CHECK_RULE: usize = 2;

/// Rounding of the quadrature rule checks: the weights and the monomial sums of a rule of at most
/// 35 signed points. 1e-15 is 36 ulp of the volume 1/6 (2.8e-17). Measured 5.2e-17, 2 ulp.
const QUADRATURE_ROUNDING_TOL: f64 = 1.0e-15;
/// Radial stress of the Lamé solution at the pressure faces in Pa: 1e-14 of the 1e7 Pa that it
/// is checked against, 54 ulp (1.9e-9). Measured 0.
const LAME_STRESS_TOL_PA: f64 = 1.0e-7;

/// Grundmann–Möller rule of degree `2 s + 1` on the unit tetrahedron, as barycentric points and
/// weights that sum to its volume 1/6 (Grundmann and Möller 1978). Some weights are
/// negative.
fn grundmann_moeller(s: usize) -> Vec<([f64; 4], f64)> {
    let factorial = |k: usize| (1..=k).map(|i| i as f64).product::<f64>();
    let degree = 2 * s + 1;
    let mut rule = Vec::new();
    for i in 0..=s {
        let denominator = (degree + 3 - 2 * i) as f64;
        let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
        let weight = sign * (0..degree).map(|_| denominator).product::<f64>()
            / (0..2 * s).map(|_| 2.0).product::<f64>()
            / (factorial(i) * factorial(degree + 3 - i));
        let level = s - i;
        for b0 in 0..=level {
            for b1 in 0..=level - b0 {
                for b2 in 0..=level - b0 - b1 {
                    let b3 = level - b0 - b1 - b2;
                    let point = [b0, b1, b2, b3].map(|b| (2 * b + 1) as f64 / denominator);
                    rule.push((point, weight));
                }
            }
        }
    }
    rule
}

/// Lamé parameters `λ` and `μ` in Pa of the study material.
fn study_lame() -> (f64, f64) {
    let (e, nu) = (STUDY_YOUNG_MODULUS_PA, STUDY_POISSON_RATIO);
    (
        e * nu / ((1.0 + nu) * (1.0 - 2.0 * nu)),
        e / (2.0 * (1.0 + nu)),
    )
}

/// Exact plane-strain Lamé solution `u = (A r + B / r) e_r`, `u_z = 0`, of a cylinder with
/// gauge pressure `p_i` on `r = a` and `p_o` on `r = b`, each pushing into the solid.
struct LameCylinder {
    /// `A`, dimensionless.
    a: f64,
    /// `B` in m².
    b: f64,
}

impl LameCylinder {
    fn new(inner_pa: f64, outer_pa: f64) -> Self {
        let (lambda, mu) = study_lame();
        let (a2, b2) = (
            INNER_RADIUS_M * INNER_RADIUS_M,
            OUTER_RADIUS_M * OUTER_RADIUS_M,
        );
        Self {
            a: (inner_pa * a2 - outer_pa * b2) / (2.0 * (lambda + mu) * (b2 - a2)),
            b: (inner_pa - outer_pa) * a2 * b2 / (2.0 * mu * (b2 - a2)),
        }
    }

    /// Displacement in m at `x`: `(A + B / r²) (x, y, 0)`.
    fn displacement(&self, x: [f64; 3]) -> [f64; 3] {
        let g = self.a + self.b / (x[0] * x[0] + x[1] * x[1]);
        [g * x[0], g * x[1], 0.0]
    }

    /// Small strain at `x`: `(A + B / r²) δ_ij − 2 B x_i x_j / r⁴` in the plane, zero
    /// otherwise.
    fn strain(&self, x: [f64; 3]) -> [[f64; 3]; 3] {
        let r2 = x[0] * x[0] + x[1] * x[1];
        let g = self.a + self.b / r2;
        let c = -2.0 * self.b / (r2 * r2);
        let mut strain = [[0.0; 3]; 3];
        for (i, row) in strain.iter_mut().take(2).enumerate() {
            for (j, value) in row.iter_mut().take(2).enumerate() {
                *value = c * x[i] * x[j] + if i == j { g } else { 0.0 };
            }
        }
        strain
    }

    /// Radial stress `σ_rr` in Pa at radius `r`, from the strain at `(r, 0, 0)` and Hooke's law.
    fn radial_stress_pa(&self, r: f64) -> f64 {
        let (lambda, mu) = study_lame();
        let e = self.strain([r, 0.0, 0.0]);
        lambda * (e[0][0] + e[1][1] + e[2][2]) + 2.0 * mu * e[0][0]
    }
}

/// Quarter of the thick cylinder, `0 ≤ θ ≤ π/2`, with `n` radial divisions, [`ANGULAR_FACTOR`]
/// `n` circumferential divisions and [`LAYERS`] layers of thickness `(b − a) / n`, so that
/// every level has the same cell
/// shapes. Each mapped hexahedron is split into six tetrahedra around its main diagonal (Kuhn),
/// which is conforming between neighbours. Regions: `inner` (r = a), `outer` (r = b), `y0`
/// (θ = 0), `x0` (θ = π/2), `z0` and `z1`.
fn quarter_cylinder(n: usize) -> TetraMesh {
    use std::f64::consts::{FRAC_PI_2, PI};
    const KUHN: [[usize; 4]; 6] = [
        [0, 1, 3, 7],
        [0, 1, 5, 7],
        [0, 2, 3, 7],
        [0, 2, 6, 7],
        [0, 4, 5, 7],
        [0, 4, 6, 7],
    ];
    let layer_m = (OUTER_RADIUS_M - INNER_RADIUS_M) / n as f64;
    let m = ANGULAR_FACTOR * n;
    let index = |i: usize, j: usize, k: usize| (k * (m + 1) + j) * (n + 1) + i;
    let mut vertices_m = Vec::new();
    for k in 0..=LAYERS {
        for j in 0..=m {
            for i in 0..=n {
                let xi = i as f64 / n as f64;
                let eta = j as f64 / m as f64;
                let r = INNER_RADIUS_M
                    + (OUTER_RADIUS_M - INNER_RADIUS_M) * (xi - RADIAL_GRADING * xi * (1.0 - xi));
                let theta = FRAC_PI_2 * (eta + ANGULAR_GRADING * (PI * eta).sin());
                // The symmetry planes are exact, not rounded through cos(π/2).
                let (x, y) = match j {
                    0 => (r, 0.0),
                    _ if j == m => (0.0, r),
                    _ => (r * theta.cos(), r * theta.sin()),
                };
                vertices_m.push([x, y, k as f64 * layer_m]);
            }
        }
    }
    let mut cells = Vec::new();
    for k in 0..LAYERS {
        for j in 0..m {
            for i in 0..n {
                let corner = |c: usize| index(i + (c & 1), j + ((c >> 1) & 1), k + ((c >> 2) & 1));
                for tet in KUHN {
                    let mut cell = tet.map(corner);
                    if determinant(&edge_matrix(&vertices_m, cell)) < 0.0 {
                        cell.swap(0, 1);
                    }
                    cells.push(cell);
                }
            }
        }
    }
    // A boundary quad is split along the diagonal from its lowest to its highest corner, as
    // the Kuhn split splits that face.
    let quads =
        |name: &str, corner: &dyn Fn(usize, usize) -> usize, first: usize, second: usize| {
            let mut faces = Vec::new();
            for q in 0..second {
                for p in 0..first {
                    let (c00, c10, c01, c11) = (
                        corner(p, q),
                        corner(p + 1, q),
                        corner(p, q + 1),
                        corner(p + 1, q + 1),
                    );
                    faces.push([c00, c10, c11]);
                    faces.push([c00, c01, c11]);
                }
            }
            FaceRegion {
                name: name.to_owned(),
                faces,
            }
        };
    let boundary_regions = vec![
        quads("inner", &|j, k| index(0, j, k), m, LAYERS),
        quads("outer", &|j, k| index(n, j, k), m, LAYERS),
        quads("y0", &|i, k| index(i, 0, k), n, LAYERS),
        quads("x0", &|i, k| index(i, m, k), n, LAYERS),
        quads("z0", &|i, j| index(i, j, 0), n, m),
        quads("z1", &|i, j| index(i, j, LAYERS), n, m),
    ];
    TetraMesh {
        vertices_m,
        cells,
        boundary_regions,
    }
}

/// Columns are the edges from the first vertex: `m[row][col] = (X_{col+1} − X_0)[row]`.
fn edge_matrix(vertices: &[[f64; 3]], cell: [usize; 4]) -> [[f64; 3]; 3] {
    let origin = vertices[cell[0]];
    std::array::from_fn(|row| std::array::from_fn(|col| vertices[cell[col + 1]][row] - origin[row]))
}

fn determinant(m: &[[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// Inverse from the adjugate; the cyclic indices give the cofactor signs.
fn inverse(m: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let det = determinant(m);
    std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            let (r1, r2) = ((j + 1) % 3, (j + 2) % 3);
            let (c1, c2) = ((i + 1) % 3, (i + 2) % 3);
            (m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1]) / det
        })
    })
}

/// Solves the quarter cylinder with `n` divisions, inner gauge pressure [`INNER_PRESSURE_PA`]
/// and outer gauge pressure `outer_pa` (no outer load when zero), at the fixture's relative
/// tolerance divided by `divisor`. Returns the mesh and the converged report.
fn solve_cylinder(n: usize, outer_pa: f64, divisor: f64) -> (TetraMesh, RunReport) {
    let gauge = |value_pa: f64| Pressure::Gauge {
        value_pa,
        reference_absolute_pa: AMBIENT_PA,
    };
    let ambient = Pressure::Absolute {
        value_pa: AMBIENT_PA,
    };
    let fixed = |region: &str, axis: usize| {
        let mut components_m = [None; 3];
        components_m[axis] = Some(0.0);
        SolidBoundary::Displacement {
            region: region.to_owned(),
            components_m,
        }
    };
    let mut model = Model::from_json(INPUT).unwrap();
    assert_eq!(
        model.linear_solver.relative_residual_tolerance,
        FIXTURE_RELATIVE_TOLERANCE
    );
    assert_eq!(model.linear_solver.absolute_residual_tolerance, 0.0);
    model.linear_solver.relative_residual_tolerance = FIXTURE_RELATIVE_TOLERANCE / divisor;
    model.outputs = vec![Output::DisplacementVector];
    let mesh = quarter_cylinder(n);
    let case = elastic(&mut model);
    case.mesh = mesh.clone();
    case.material.young_modulus_pa = STUDY_YOUNG_MODULUS_PA;
    case.material.poisson_ratio = STUDY_POISSON_RATIO;
    case.body_force_n_m3 = [0.0; 3];
    case.boundary_conditions = vec![
        fixed("x0", 0),
        fixed("y0", 1),
        fixed("z0", 2),
        fixed("z1", 2),
        SolidBoundary::Pressure {
            region: "inner".to_owned(),
            applied: gauge(INNER_PRESSURE_PA),
            ambient,
        },
    ];
    if outer_pa != 0.0 {
        case.boundary_conditions.push(SolidBoundary::Pressure {
            region: "outer".to_owned(),
            applied: gauge(outer_pa),
            ambient,
        });
    }
    let report = solve(&model).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    (mesh, report)
}

/// Relative errors `‖u − u_h‖ / ‖u‖` in the L² norm and in the energy norm
/// `‖v‖_E² = ∫ (λ (tr ε(v))² + 2 μ ε(v):ε(v)) dV`, integrated over the meshed volume with
/// `rule` on every cell.
fn relative_errors(
    mesh: &TetraMesh,
    displacement_m: &[[f64; 3]],
    exact: &LameCylinder,
    rule: &[([f64; 4], f64)],
) -> [f64; 2] {
    let (lambda, mu) = study_lame();
    let energy = |e: &[[f64; 3]; 3]| {
        let trace = e[0][0] + e[1][1] + e[2][2];
        let contraction: f64 = e.iter().flatten().map(|v| v * v).sum();
        lambda * trace * trace + 2.0 * mu * contraction
    };
    let mut sums = [0.0; 4];
    for &cell in &mesh.cells {
        let edges = edge_matrix(&mesh.vertices_m, cell);
        let jacobian = determinant(&edges);
        let inv = inverse(&edges);
        let u = cell.map(|v| displacement_m[v]);
        let gradient: [[f64; 3]; 3] = std::array::from_fn(|i| {
            std::array::from_fn(|j| (0..3).map(|c| (u[c + 1][i] - u[0][i]) * inv[c][j]).sum())
        });
        let discrete_strain: [[f64; 3]; 3] = std::array::from_fn(|i| {
            std::array::from_fn(|j| 0.5 * (gradient[i][j] + gradient[j][i]))
        });
        for &(weights, weight) in rule {
            let point: [f64; 3] = std::array::from_fn(|d| {
                (0..4)
                    .map(|k| weights[k] * mesh.vertices_m[cell[k]][d])
                    .sum()
            });
            let discrete: [f64; 3] =
                std::array::from_fn(|d| (0..4).map(|k| weights[k] * u[k][d]).sum());
            let exact_u = exact.displacement(point);
            let exact_strain = exact.strain(point);
            let error_strain: [[f64; 3]; 3] = std::array::from_fn(|i| {
                std::array::from_fn(|j| exact_strain[i][j] - discrete_strain[i][j])
            });
            let w = weight * jacobian;
            sums[0] += w
                * (0..3)
                    .map(|d| (exact_u[d] - discrete[d]) * (exact_u[d] - discrete[d]))
                    .sum::<f64>();
            sums[1] += w * exact_u.iter().map(|v| v * v).sum::<f64>();
            sums[2] += w * energy(&error_strain);
            sums[3] += w * energy(&exact_strain);
        }
    }
    [(sums[0] / sums[1]).sqrt(), (sums[2] / sums[3]).sqrt()]
}

/// One refinement level. Errors are `[L², energy]` relative errors of the fixture-tolerance
/// solve, of the check solve and of the fixture-tolerance solve integrated with the degree-5
/// rule.
struct StudyLevel {
    divisions: usize,
    spacing: f64,
    cells: usize,
    minimum_mean_ratio: f64,
    iterations: usize,
    tight_iterations: usize,
    errors: [f64; 2],
    tight_errors: [f64; 2],
    check_rule_errors: [f64; 2],
}

fn nodal_displacements<'a>(report: &'a RunReport, mesh: &TetraMesh) -> &'a [[f64; 3]] {
    let Quantity::VectorField {
        positions_m,
        values,
        ..
    } = &report.quantities[&Output::DisplacementVector]
    else {
        panic!("expected a displacement field");
    };
    assert_eq!(positions_m, &mesh.vertices_m);
    values
}

/// Solves every level at the fixture's tolerance and again at the tightened one.
fn run_study(outer_pa: f64) -> Vec<StudyLevel> {
    let exact = LameCylinder::new(INNER_PRESSURE_PA, outer_pa);
    let (rule, check_rule) = (grundmann_moeller(ERROR_RULE), grundmann_moeller(CHECK_RULE));
    LEVEL_DIVISIONS
        .iter()
        .map(|&n| {
            let (mesh, report) = solve_cylinder(n, outer_pa, 1.0);
            let (_, tight) = solve_cylinder(n, outer_pa, verify::TOLERANCE_DIVISOR);
            let u = nodal_displacements(&report, &mesh);
            let u_tight = nodal_displacements(&tight, &mesh);
            StudyLevel {
                divisions: n,
                spacing: 1.0 / n as f64,
                cells: mesh.cells.len(),
                minimum_mean_ratio: report.meshes[0]
                    .quality
                    .as_ref()
                    .unwrap()
                    .minimum_mean_ratio,
                iterations: report.convergence[0].iterations,
                tight_iterations: tight.convergence[0].iterations,
                errors: relative_errors(&mesh, u, &exact, &rule),
                tight_errors: relative_errors(&mesh, u_tight, &exact, &rule),
                check_rule_errors: relative_errors(&mesh, u, &exact, &check_rule),
            }
        })
        .collect()
}

/// What the study concludes for one norm.
struct NormAssessment {
    /// The band rule of the study: fit over the finest three levels, spread, band and shrink
    /// factor.
    band: verify::BandCheck,
    /// Least-squares order of all levels, reported for comparison only.
    all_levels_order: f64,
    algebraic_checks: Vec<verify::AlgebraicCheck>,
    /// Relative change of the error between the degree-7 and the degree-5 rule, per level.
    quadrature_changes: Vec<f64>,
}

/// Applies the band rule to norm `norm` (0 = L², 1 = energy) and measures the algebraic and
/// quadrature change of each level. Fails, instead of reporting an order, for errors that are
/// zero or not finite and for fewer than four levels.
fn assess_norm(levels: &[StudyLevel], norm: usize) -> Result<NormAssessment, verify::VerifyError> {
    let (formal, _, _) = stated_band(norm);
    let spacings: Vec<f64> = levels.iter().map(|l| l.spacing).collect();
    let errors: Vec<f64> = levels.iter().map(|l| l.errors[norm]).collect();
    let algebraic_checks = levels
        .iter()
        .map(|l| verify::algebraic_check(l.errors[norm], l.tight_errors[norm], STUDY_RATIO))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NormAssessment {
        band: verify::band_check(&spacings, &errors, formal)?,
        all_levels_order: verify::fitted_order(&spacings, &errors)?,
        algebraic_checks,
        quadrature_changes: levels
            .iter()
            .map(|l| (l.check_rule_errors[norm] - l.errors[norm]).abs() / l.errors[norm])
            .collect(),
    })
}

/// Formal order, stated spread bound and stated band half-width `Δp + 1.5 · bound` of norm
/// `norm`.
fn stated_band(norm: usize) -> (f64, f64, f64) {
    let (order, spread) = [
        (L2_ORDER, L2_SPREAD_BOUND),
        (ENERGY_ORDER, ENERGY_SPREAD_BOUND),
    ][norm];
    (
        order,
        spread,
        verify::ORDER_SHIFT_BUDGET + verify::SPREAD_FACTOR * spread,
    )
}

/// Runs every check of the study for one norm and returns the first failure.
fn study_verdict(levels: &[StudyLevel], norm: usize) -> Result<(), String> {
    let assessment = assess_norm(levels, norm).map_err(|e| e.to_string())?;
    // The band, then the shrink rate that its derivation assumes.
    if let Some(failure) = assessment.band.failure() {
        return Err(failure);
    }
    let (_, spread_bound, _) = stated_band(norm);
    if assessment.band.spread > spread_bound {
        return Err(format!(
            "pre-asymptotic spread {} exceeds the stated {spread_bound}",
            assessment.band.spread
        ));
    }
    let budget = verify::algebraic_error_budget(STUDY_RATIO).map_err(|e| e.to_string())?;
    for (check, &quadrature) in assessment
        .algebraic_checks
        .iter()
        .zip(&assessment.quadrature_changes)
    {
        if let Some(warning) = check.warning() {
            return Err(warning);
        }
        // A relative perturbation of an error norm moves the observed order exactly as an
        // algebraic one does, so the quadrature error has the same budget δ.
        if quadrature > budget {
            return Err(format!("quadrature change {quadrature} exceeds {budget}"));
        }
    }
    if !levels
        .windows(2)
        .all(|p| p[1].errors[norm] < p[0].errors[norm])
    {
        return Err("the error does not decrease at every level".to_owned());
    }
    Ok(())
}

fn print_study(levels: &[StudyLevel]) {
    let assessments = [0, 1].map(|norm| assess_norm(levels, norm));
    println!(
        "level   n  cells  min_mean_ratio  iterations  tight_iterations  l2_error      l2_order  l2_algebraic  l2_quadrature  energy_error  energy_order  energy_algebraic  energy_quadrature"
    );
    for (i, level) in levels.iter().enumerate() {
        let column = |norm: usize| match &assessments[norm] {
            Ok(a) => (
                i.checked_sub(1).map_or("-".to_owned(), |k| {
                    format!("{:.6}", a.band.pairwise_orders[k])
                }),
                format!("{:.3e}", a.algebraic_checks[i].relative_change),
                format!("{:.3e}", a.quadrature_changes[i]),
            ),
            Err(e) => (format!("{e}"), "-".to_owned(), "-".to_owned()),
        };
        let (l2, energy) = (column(0), column(1));
        println!(
            "{i:>5}  {:>2}  {:>5}  {:>14.6}  {:>10}  {:>16}  {:>12.6e}  {:>8}  {:>12}  {:>13}  {:>12.6e}  {:>12}  {:>16}  {:>17}",
            level.divisions,
            level.cells,
            level.minimum_mean_ratio,
            level.iterations,
            level.tight_iterations,
            level.errors[0],
            l2.0,
            l2.1,
            l2.2,
            level.errors[1],
            energy.0,
            energy.1,
            energy.2,
        );
    }
    for (norm, name) in [(0, "l2"), (1, "energy")] {
        let (order, _, stated_half_width) = stated_band(norm);
        match &assessments[norm] {
            Ok(a) => println!(
                "{name}: fitted order (finest {}) {:.6}, band {order} +- {:.6}, stated band {order} +- {stated_half_width:.4}, spread {:.6}, contraction {:.4}, fitted order (all levels) {:.6}",
                verify::MIN_FIT_LEVELS,
                a.band.fitted_order,
                a.band.half_width,
                a.band.spread,
                a.band.contraction,
                a.all_levels_order
            ),
            Err(e) => println!("{name}: no order ({e})"),
        }
    }
    println!(
        "algebraic budget {:.6e}",
        verify::algebraic_error_budget(STUDY_RATIO).unwrap()
    );
}

#[test]
fn grundmann_moeller_rules_integrate_monomials_up_to_their_degree() {
    // ∫ x^a y^b z^c over the unit tetrahedron is a! b! c! / (a + b + c + 3)!.
    let factorial = |k: usize| (1..=k).map(|i| i as f64).product::<f64>();
    let power = |x: f64, k: usize| (0..k).map(|_| x).product::<f64>();
    for s in [CHECK_RULE, ERROR_RULE] {
        let rule = grundmann_moeller(s);
        let total: f64 = rule.iter().map(|&(_, w)| w).sum();
        close(total, 1.0 / 6.0, QUADRATURE_ROUNDING_TOL);
        let degree = 2 * s + 1;
        for a in 0..=degree {
            for b in 0..=degree - a {
                for c in 0..=degree - a - b {
                    let quadrature: f64 = rule
                        .iter()
                        .map(|&(l, w)| w * power(l[1], a) * power(l[2], b) * power(l[3], c))
                        .sum();
                    let exact =
                        factorial(a) * factorial(b) * factorial(c) / factorial(a + b + c + 3);
                    close(quadrature, exact, QUADRATURE_ROUNDING_TOL);
                }
            }
        }
    }
}

#[test]
fn lame_thick_cylinder_converges_at_the_formal_orders_in_l2_and_energy() {
    // The exact solution meets both pressure conditions. This checks the formulas for A and B
    // through the strain and Hooke's law, which do not use them.
    let exact = LameCylinder::new(INNER_PRESSURE_PA, 0.0);
    close(
        exact.radial_stress_pa(INNER_RADIUS_M),
        -INNER_PRESSURE_PA,
        LAME_STRESS_TOL_PA,
    );
    close(
        exact.radial_stress_pa(OUTER_RADIUS_M),
        0.0,
        LAME_STRESS_TOL_PA,
    );

    let levels = run_study(0.0);
    assert!(levels.len() >= 4);
    print_study(&levels);
    for norm in [0, 1] {
        if let Err(failure) = study_verdict(&levels, norm) {
            panic!("norm {norm}: {failure}");
        }
    }
}

#[test]
fn the_order_study_rejects_the_affine_cylinder() {
    // With equal pressure inside and outside, B = 0 and u = A (x, y, 0) is affine: linear
    // tetrahedra represent it exactly even on the faceted surfaces, because the facet tractions
    // -p n are then exactly sigma n. This is the exactness case of the study (the elastic solid
    // has no source term; the curvature of the solution is what the B / r part supplies). Its
    // errors are solver and rounding noise, and the same study must fail on them: the band,
    // the first check, rejects the fitted order.
    let levels = run_study(INNER_PRESSURE_PA);
    print_study(&levels);
    for norm in [0, 1] {
        let verdict = study_verdict(&levels, norm);
        println!("norm {norm}: {verdict:?}");
        assert!(
            verdict
                .as_ref()
                .is_err_and(|failure| failure.starts_with("fitted order")),
            "norm {norm}: the band must reject the affine case, got {verdict:?}"
        );
    }
}
