//! Multimaterial heat conduction: the series-resistance reference, interface heat rates and
//! regional energy balances, attribution of exterior power, refinement of a curved profile,
//! and rejected volume assignments.

// Reference values in tests are independent of the library decision and result paths.
#![allow(clippy::disallowed_methods)]

#[path = "common/block.rs"]
mod block;
mod common;

use caekit::physics::heat_conduction::{
    HeatConduction, MultimaterialHeatConduction, SolidThermal, ThermalBoundary, ThermalVolumeRegion,
};
use caekit::{
    Balance, InterfaceValue, Model, Output, Physics, Quantity, RunReport, SampleLocation,
    StopReason, solve,
};

use block::{TETRA_SAMPLES, interpolate, structured_block};
use caekit_numerics::verify::{
    BandCheck, Behaviour, ORDER_SHIFT_BUDGET, TOLERANCE_DIVISOR, algebraic_check,
    algebraic_error_budget, band_check, classify,
};
use common::{close, invalid_at, region};

// Allowances of the checks on the 6-tetrahedron two-layer fixture (layers of 0.01 m, 0.01 m^2
// cross section). Linear tetrahedra represent its piecewise-linear temperature exactly, so the
// solution differs from the analytical one only by the linear solve, which stops at the default
// relative residual 1e-10 of docs/model.md, and by binary64 rounding. "Measured" is the largest
// |actual - expected| over the sites that use the constant (print it in `common::close` and run
// `cargo test -p caekit --test multimaterial_heat_conduction -- --nocapture --test-threads=1`).

/// Nodal temperature in K: about 1e-9 of the 10 K to 11 K temperature differences of these cases.
/// Measured 8.1e-10 K for the parallel heat paths and at most 5.7e-14 K elsewhere.
const AFFINE_TEMPERATURE_TOL_K: f64 = 1.0e-8;
/// Cell heat flux in W/m^2: 1e-9 of the 1000 W/m^2 of the layered case. Measured 4.6e-13 W/m^2.
const AFFINE_FLUX_TOL_W_M2: f64 = 1.0e-6;
/// Heat rates and energy imbalances in W: 1e-9 of the 10 W that crosses the interface and 3.6e-10
/// of the 27.5 W of the parallel-path case. Measured at most 6.7e-11 W (the parallel-path
/// interface rate and region imbalance).
const AFFINE_POWER_TOL_W: f64 = 1.0e-8;
/// Heat rates and imbalances of the cycle of four slabs in W: 1e-8 of the 10 W that each of the
/// three interfaces carries. Measured 7.5e-11 W.
const CYCLE_POWER_TOL_W: f64 = 1.0e-7;
/// Heat rates and imbalances of the weak-convection case in W: 1e-9 of the 1e-3 W that it moves.
/// Measured 2.4e-18 W.
const WEAK_POWER_TOL_W: f64 = 1.0e-12;
/// Heat rates and imbalances of the sourced layers in W: 2e-7 of the 1 W that the sources
/// generate in total (docs/physics/multimaterial-heat-conduction.md). Measured 8.8e-13 W on 4, 8
/// and 16 divisions.
const SOURCED_POWER_TOL_W: f64 = 2.0e-7;
/// Smallest ratio of the temperature errors of successive uniform levels of 4, 8 and 16
/// divisions. Second order gives 4 and first order 2; 2.7 is an observed order of 1.43 (2^1.43),
/// between them. Measured 4.015 and 4.005.
const UNIFORM_TEMPERATURE_RATIO_MIN: f64 = 2.7;
/// Smallest ratio of the flux errors of successive uniform levels, per layer. First order gives
/// 2; 1.65 is an observed order of 0.72. Measured 1.963 and 1.987 on the left, 2.025 and 2.009 on
/// the right.
const UNIFORM_FLUX_RATIO_MIN: f64 = 1.65;
/// Largest temperature error of the 16-division level in K. Measured 0.00528861 K, 3.8 times
/// below.
const FINEST_TEMPERATURE_ERROR_MAX_K: f64 = 0.02;
/// Largest flux errors of the 16-division level per layer in W/m^2. Measured 71.97 and 18.08,
/// 1.4 and 1.9 times below.
const FINEST_FLUX_ERROR_MAX_W_M2: [f64; 2] = [100.0, 35.0];
/// Sum of the 64 weights of the tetrahedron rule, which should be 1. Each weight is a product of
/// five rounded factors, so 1e-14 (45 eps) lies between the typical accumulated rounding, about
/// sqrt(64) eps = 1.8e-15, and the worst case 63 eps = 1.4e-14. Measured 2.2e-16.
const RULE_WEIGHT_SUM_TOL: f64 = 1.0e-14;
/// Relative allowance for the integrated volume of the two layers. The finest level accumulates
/// 6.3e6 rule points per layer, for which a random-walk estimate of the rounding is
/// sqrt(6.3e6) u = 2.8e-13 relative (u = 1.1e-16); the worst case m u is not attained. Measured
/// 4.8e-14 relative.
const VOLUME_RELATIVE_TOL: f64 = 1.0e-12;

fn example() -> Model {
    Model::from_json(include_bytes!(
        "../../../examples/multimaterial_heat_conduction.json"
    ))
    .unwrap()
}

fn case(model: &mut Model) -> &mut MultimaterialHeatConduction {
    let Physics::MultimaterialHeatConduction(case) = &mut model.physics else {
        panic!("expected multimaterial heat conduction");
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

fn fluxes(report: &RunReport) -> &[[f64; 3]] {
    let Quantity::VectorField {
        unit,
        location: SampleLocation::Cell,
        values,
        ..
    } = &report.quantities[&Output::SolidHeatFlux]
    else {
        panic!("expected cell heat flux vectors");
    };
    assert_eq!(unit, "W/m^2");
    values
}

fn interfaces(report: &RunReport) -> &[InterfaceValue] {
    let Quantity::ByInterface { unit, values } = &report.quantities[&Output::InterfaceHeatRate]
    else {
        panic!("expected directed interface heat rates");
    };
    assert_eq!(unit, "W");
    values
}

fn energy<'a>(report: &'a RunReport, name: &str) -> &'a Balance {
    let balance = report.balances.iter().find(|balance| balance.name == name);
    let balance = balance.unwrap_or_else(|| panic!("missing balance {name}"));
    assert_eq!(balance.unit, "W");
    balance
}

fn check_region_energy(
    report: &RunReport,
    name: &str,
    generated: f64,
    exterior: f64,
    interface: f64,
    tolerance: f64,
) {
    let balance = energy(report, &format!("solid_energy:{name}"));
    close(balance.terms["volumetric_generation"], generated, tolerance);
    close(balance.terms["exterior_heat_input"], exterior, tolerance);
    close(balance.terms["interface_heat_input"], interface, tolerance);
    close(balance.imbalance, 0.0, tolerance);
}

fn check_energy_totals(report: &RunReport, tolerance: f64) {
    let global = energy(report, "solid_energy");
    let volumes = report
        .balances
        .iter()
        .filter(|balance| balance.name.starts_with("solid_energy:"));
    let sum = |term: &str| volumes.clone().map(|balance| balance.terms[term]).sum();
    close(
        sum("volumetric_generation"),
        global.terms["volumetric_generation"],
        tolerance,
    );
    close(
        sum("exterior_heat_input"),
        global.terms["prescribed_temperature_reaction"]
            + global.terms["prescribed_heat_flux"]
            + global.terms["convection"],
        tolerance,
    );
    close(sum("interface_heat_input"), 0.0, tolerance);
    close(
        volumes.map(|balance| balance.imbalance).sum(),
        global.imbalance,
        tolerance,
    );
    close(global.imbalance, 0.0, tolerance);
}

#[test]
fn layered_conduction_matches_the_series_resistance_solution() {
    let mut model = example();
    let report = solve(&model).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    let stage = &report.convergence[0];
    assert_eq!(stage.residual_unit, "W");
    assert!(stage.final_true_residual_l2.unwrap() <= stage.residual_threshold.unwrap());
    let mesh = &case(&mut model).mesh;
    let (nodes, temperature) = temperatures(&report);
    assert_eq!(nodes, mesh.vertices_m);
    assert_eq!(temperature.len(), nodes.len());
    let mut interface_nodes = 0;
    for (&[x, _, _], &value) in nodes.iter().zip(temperature) {
        let expected = if x <= 0.01 {
            311.0 - 100.0 * x
        } else {
            310.0 - 1_000.0 * (x - 0.01)
        };
        close(value, expected, AFFINE_TEMPERATURE_TOL_K);
        if x == 0.01 {
            interface_nodes += 1;
            close(value, 310.0, AFFINE_TEMPERATURE_TOL_K);
        }
    }
    assert_eq!(interface_nodes, 4);
    let flux = fluxes(&report);
    assert_eq!(flux.len(), mesh.cells.len());
    for vector in flux {
        for (&value, expected) in vector.iter().zip([1_000.0, 0.0, 0.0]) {
            close(value, expected, AFFINE_FLUX_TOL_W_M2);
        }
    }
    let [contact] = interfaces(&report) else {
        panic!("expected one interface between the two layers");
    };
    assert_eq!(contact.from_region, "left");
    assert_eq!(contact.to_region, "right");
    close(contact.value, 10.0, AFFINE_POWER_TOL_W);
    let Quantity::ByRegion { unit, values } = &report.quantities[&Output::BoundaryHeatRate] else {
        panic!("expected exterior heat rates");
    };
    assert_eq!(unit, "W");
    for (name, expected) in [
        ("x0", 10.0),
        ("x1", -10.0),
        ("y0", 0.0),
        ("y1", 0.0),
        ("z0", 0.0),
        ("z1", 0.0),
    ] {
        close(values[name], expected, AFFINE_POWER_TOL_W);
    }
    assert_eq!(report.balances.len(), 3);
    check_region_energy(&report, "left", 0.0, 10.0, -10.0, AFFINE_POWER_TOL_W);
    check_region_energy(&report, "right", 0.0, -10.0, 10.0, AFFINE_POWER_TOL_W);
    check_energy_totals(&report, AFFINE_POWER_TOL_W);
    let projection = &report.convergence[1];
    assert_eq!(projection.stage, "solid_interface_equilibration");
    assert_eq!(projection.residual_unit, "W");
    assert!(projection.final_true_residual_l2.unwrap() <= projection.residual_threshold.unwrap());
}

#[test]
fn two_regions_can_share_a_material_and_recover_the_uniform_solution() {
    let mut model = example();
    model
        .outputs
        .retain(|output| *output != Output::InterfaceHeatRate);
    let multi = case(&mut model);
    multi.thermal.materials.pop();
    multi.thermal.volume_regions[1].material_id = multi.thermal.materials[0].id.clone();
    for region in &mut multi.thermal.volume_regions {
        region.volumetric_heat_source_w_m3 = 200_000.0;
    }
    let uniform = HeatConduction {
        geometry: multi.geometry.clone(),
        mesh: multi.mesh.clone(),
        thermal: SolidThermal {
            boundary_conditions: multi.thermal.boundary_conditions.clone(),
            material: multi.thermal.materials[0].clone(),
            volumetric_heat_source_w_m3: 200_000.0,
        },
    };
    let report = solve(&model).unwrap();
    model.physics = Physics::HeatConduction(Box::new(uniform));
    let reference = solve(&model).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    assert_eq!(reference.stop_reason, StopReason::Converged);
    for output in [Output::SolidTemperature, Output::SolidHeatFlux] {
        assert_eq!(
            serde_json::to_value(&report.quantities[&output]).unwrap(),
            serde_json::to_value(&reference.quantities[&output]).unwrap()
        );
    }
    for name in ["x0", "x1", "y0", "y1", "z0", "z1"] {
        close(
            region(&report, Output::BoundaryHeatRate, name),
            region(&reference, Output::BoundaryHeatRate, name),
            AFFINE_POWER_TOL_W,
        );
    }
    assert_eq!(
        serde_json::to_value(&report.balances[0]).unwrap(),
        serde_json::to_value(&reference.balances[0]).unwrap()
    );
    check_energy_totals(&report, AFFINE_POWER_TOL_W);
}

#[test]
fn a_cycle_of_region_pairs_keeps_the_flux_from_the_element_gradients() {
    let mut model = example();
    model.outputs = vec![Output::InterfaceHeatRate];
    let multi = case(&mut model);
    multi.mesh = structured_block([0.02, 0.1, 0.1], 4);
    multi.thermal.boundary_conditions = vec![temperature("x0", 306.5), temperature("x1", 300.0)];
    // Four slabs ordered a, b, c, a along x. The same 10 W crosses each of the three
    // interfaces. The two parts of a touch opposite ends, so its exterior net is zero.
    let mut regions: Vec<_> = [("c", "high-k"), ("a", "high-k"), ("b", "low-k")]
        .into_iter()
        .map(|(name, material_id)| ThermalVolumeRegion {
            name: name.into(),
            material_id: material_id.into(),
            cells: Vec::new(),
            volumetric_heat_source_w_m3: 0.0,
        })
        .collect();
    for (index, cell) in multi.mesh.cells.iter().enumerate() {
        let x = interpolate(cell.map(|node| multi.mesh.vertices_m[node][0]), [0.25; 4]);
        let slab = (x / 0.005) as usize;
        let region = [1, 2, 0, 1][slab];
        regions[region].cells.push(index);
    }
    multi.thermal.volume_regions = regions;
    let report = solve(&model).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    let expected = [("a", "b", 10.0), ("a", "c", -10.0), ("b", "c", 10.0)];
    let rates = interfaces(&report);
    assert_eq!(rates.len(), expected.len());
    for (rate, (from, to, power)) in rates.iter().zip(expected) {
        assert_eq!(rate.from_region, from);
        assert_eq!(rate.to_region, to);
        close(rate.value, power, CYCLE_POWER_TOL_W);
    }
    for name in ["a", "b", "c"] {
        check_region_energy(&report, name, 0.0, 0.0, 0.0, CYCLE_POWER_TOL_W);
    }
    check_energy_totals(&report, CYCLE_POWER_TOL_W);
}

#[test]
fn flux_and_weak_convection_are_attributed_to_the_owning_volume() {
    let mut model = example();
    case(&mut model).thermal.boundary_conditions = vec![
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
        let right_drop = 0.1 * (0.02 - x.max(0.01));
        let left_drop = 0.1 * (0.01 - x).max(0.0) / 10.0;
        close(
            value,
            310.0 + right_drop + left_drop,
            AFFINE_TEMPERATURE_TOL_K,
        );
    }
    close(interfaces(&report)[0].value, 0.001, WEAK_POWER_TOL_W);
    check_region_energy(&report, "left", 0.0, 0.001, -0.001, WEAK_POWER_TOL_W);
    check_region_energy(&report, "right", 0.0, -0.001, 0.001, WEAK_POWER_TOL_W);
    check_energy_totals(&report, WEAK_POWER_TOL_W);
}

#[test]
fn shared_fixed_vertices_do_not_create_transfer_between_parallel_heat_paths() {
    let mut model = example();
    let multi = case(&mut model);
    multi.mesh = structured_block([0.02, 0.1, 0.1], 2);
    for volume in &mut multi.thermal.volume_regions {
        volume.cells.clear();
    }
    for (i, &cell) in multi.mesh.cells.iter().enumerate() {
        let y = interpolate(cell.map(|node| multi.mesh.vertices_m[node][1]), [0.25; 4]);
        multi.thermal.volume_regions[usize::from(y > 0.05)]
            .cells
            .push(i);
    }
    let cold = multi.mesh.boundary_regions.remove(1);
    for (name, upper) in [("out_high_k", false), ("out_low_k", true)] {
        multi.mesh.boundary_regions.push(caekit::FaceRegion {
            name: name.to_owned(),
            faces: cold
                .faces
                .iter()
                .copied()
                .filter(|face| {
                    let y = face
                        .iter()
                        .map(|&node| multi.mesh.vertices_m[node][1] / 3.0)
                        .sum::<f64>();
                    (y > 0.05) == upper
                })
                .collect(),
        });
    }
    multi.thermal.boundary_conditions = vec![
        temperature("x0", 310.0),
        ThermalBoundary::HeatFlux {
            region: "out_high_k".into(),
            inward_heat_flux_w_m2: -5_000.0,
        },
        ThermalBoundary::HeatFlux {
            region: "out_low_k".into(),
            inward_heat_flux_w_m2: -500.0,
        },
    ];
    let report = solve(&model).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    let (nodes, values) = temperatures(&report);
    for (&[x, _, _], &value) in nodes.iter().zip(values) {
        close(value, 310.0 - 500.0 * x, AFFINE_TEMPERATURE_TOL_K);
    }
    close(
        region(&report, Output::BoundaryHeatRate, "x0"),
        27.5,
        AFFINE_POWER_TOL_W,
    );
    close(interfaces(&report)[0].value, 0.0, AFFINE_POWER_TOL_W);
    for name in ["left", "right"] {
        check_region_energy(&report, name, 0.0, 0.0, 0.0, AFFINE_POWER_TOL_W);
    }
    check_energy_totals(&report, AFFINE_POWER_TOL_W);
}

fn temperature(region: &str, value: f64) -> ThermalBoundary {
    ThermalBoundary::Temperature {
        region: region.to_owned(),
        temperature_k: value,
    }
}

struct RefinementErrors {
    temperature_k: f64,
    flux_w_m2: [f64; 2],
}

// The refinement test records timing for the separate measurement run.
#[allow(clippy::disallowed_types)]
fn sourced_layers(divisions: usize) -> RefinementErrors {
    const HALF_LENGTH: f64 = 0.01;
    const SIDE: f64 = 0.02;
    const AREA: f64 = SIDE * SIDE;
    const HOT: f64 = 311.0;
    const COLD: f64 = 300.0;
    const K: [f64; 2] = [10.0, 1.0];
    const Q: [f64; 2] = [200_000.0, 50_000.0];
    let mut model = example();
    let multi = case(&mut model);
    multi.mesh = structured_block([SIDE; 3], divisions);
    for (i, volume) in multi.thermal.volume_regions.iter_mut().enumerate() {
        volume.cells.clear();
        volume.volumetric_heat_source_w_m3 = Q[i];
    }
    let side = |cell: [usize; 4]| {
        usize::from(
            interpolate(cell.map(|node| multi.mesh.vertices_m[node][0]), [0.25; 4]) > HALF_LENGTH,
        )
    };
    for (i, &cell) in multi.mesh.cells.iter().enumerate() {
        multi.thermal.volume_regions[side(cell)].cells.push(i);
    }
    let started = std::time::Instant::now();
    let report = solve(&model).unwrap();
    let seconds = started.elapsed().as_secs_f64();
    assert_eq!(report.stop_reason, StopReason::Converged);

    // Solve q' = Q and T' = -q/k separately in each layer, with continuous
    // temperature and flux at the interface and the two prescribed end temperatures.
    let source_drop = K.map(|k| HALF_LENGTH.powi(2) / (2.0 * k));
    let q_interface = (HOT - COLD + Q[0] * source_drop[0] - Q[1] * source_drop[1])
        / (HALF_LENGTH / K[0] + HALF_LENGTH / K[1]);
    let t_interface = HOT - q_interface * HALF_LENGTH / K[0] + Q[0] * source_drop[0];
    let expected_transfer = q_interface * AREA;
    let left_exterior = (q_interface - Q[0] * HALF_LENGTH) * AREA;
    let right_exterior = -(q_interface + Q[1] * HALF_LENGTH) * AREA;
    close(
        interfaces(&report)[0].value,
        expected_transfer,
        SOURCED_POWER_TOL_W,
    );
    close(
        region(&report, Output::BoundaryHeatRate, "x0"),
        left_exterior,
        SOURCED_POWER_TOL_W,
    );
    close(
        region(&report, Output::BoundaryHeatRate, "x1"),
        right_exterior,
        SOURCED_POWER_TOL_W,
    );
    check_region_energy(
        &report,
        "left",
        Q[0] * HALF_LENGTH * AREA,
        left_exterior,
        -expected_transfer,
        SOURCED_POWER_TOL_W,
    );
    check_region_energy(
        &report,
        "right",
        Q[1] * HALF_LENGTH * AREA,
        right_exterior,
        expected_transfer,
        SOURCED_POWER_TOL_W,
    );
    check_energy_totals(&report, SOURCED_POWER_TOL_W);

    let (nodes, nodal_t) = temperatures(&report);
    let cells = &case(&mut model).mesh.cells;
    let mut temperature_error = 0.0;
    let mut flux_error = [0.0; 2];
    let mut samples = [0; 2];
    for (&cell, &[qx, qy, qz]) in cells.iter().zip(fluxes(&report)) {
        let xs = cell.map(|node| nodes[node][0]);
        let ts = cell.map(|node| nodal_t[node]);
        let layer = usize::from(interpolate(xs, [0.25; 4]) > HALF_LENGTH);
        for weights in TETRA_SAMPLES {
            let x = interpolate(xs, weights) - HALF_LENGTH;
            let t = interpolate(ts, weights);
            let expected_t = t_interface - (q_interface * x + 0.5 * Q[layer] * x * x) / K[layer];
            let expected_q = q_interface + Q[layer] * x;
            temperature_error += (t - expected_t).powi(2);
            flux_error[layer] += (qx - expected_q).powi(2) + qy.powi(2) + qz.powi(2);
            samples[layer] += 1;
        }
    }
    let errors = RefinementErrors {
        temperature_k: (temperature_error / samples.iter().sum::<usize>() as f64).sqrt(),
        flux_w_m2: std::array::from_fn(|i| (flux_error[i] / samples[i] as f64).sqrt()),
    };
    eprintln!(
        "{divisions} divisions, {} cells: {:.8} K, {:.6} and {:.6} W/m^2 RMS, \
         interface {:.9} W, {} + {} iterations, {seconds:.6} s",
        cells.len(),
        errors.temperature_k,
        errors.flux_w_m2[0],
        errors.flux_w_m2[1],
        interfaces(&report)[0].value,
        report.convergence[0].iterations,
        report.convergence[1].iterations,
    );
    errors
}

#[test]
fn uniform_layers_keep_nodal_powers_and_pairwise_ratios() {
    // The first refinement check of this kind, kept unchanged: uniform meshes, four interior
    // samples per cell, pairwise error ratios and the analytical powers at every level.
    let errors = [4, 8, 16].map(sourced_layers);
    for coarse_fine in errors.windows(2) {
        let [coarse, fine] = coarse_fine else {
            unreachable!();
        };
        assert!(coarse.temperature_k / fine.temperature_k > UNIFORM_TEMPERATURE_RATIO_MIN);
        for i in 0..2 {
            assert!(coarse.flux_w_m2[i] / fine.flux_w_m2[i] > UNIFORM_FLUX_RATIO_MIN);
        }
    }
    assert!(errors[2].temperature_k < FINEST_TEMPERATURE_ERROR_MAX_K);
    assert!(errors[2].flux_w_m2[0] < FINEST_FLUX_ERROR_MAX_W_M2[0]);
    assert!(errors[2].flux_w_m2[1] < FINEST_FLUX_ERROR_MAX_W_M2[1]);
}

// SPEC-VER-003 study of the sourced two-layer case on perturbed meshes.

/// Layer thickness in m; the interface plane is x = LAYER_M.
const LAYER_M: f64 = 0.01;
/// Edge of the square cross section in m.
const CROSS_SECTION_M: f64 = 0.02;
const CONDUCTIVITY: [f64; 2] = [10.0, 1.0];
const END_TEMPERATURES_K: [f64; 2] = [311.0, 300.0];
const SOURCES: [f64; 2] = [200_000.0, 50_000.0];
/// Divisions per axis of the self-similar levels. Each level halves the mesh spacing, so r = 2.
const STUDY_DIVISIONS: [usize; 4] = [4, 8, 16, 32];
const RATIO: f64 = 2.0;
/// Largest vertex displacement as a fraction of the spacing `h`. The test asserts a positive
/// smallest cell volume per level and prints it.
///
/// Reason for 0.1 and not 0.2: each level has its own random mesh, so the constant of the error
/// differs from level to level by a few parts in a thousand, and the pairwise orders drift with
/// it instead of approaching the formal order. At 0.2 the study failed the band rule on the
/// shrink rate (temperature 0.6204, left flux 1.6), which no band widening can repair; at 0.1
/// and 0.05 it meets the rule. 0.1 keeps the mesh distinctly non-uniform (smallest cell above
/// half of the unperturbed volume). The three measured runs are on the physics page.
const PERTURBATION: f64 = 0.1;
/// Formal orders of the measured norms for linear tetrahedra: the L2 norm of the temperature
/// converges with order 2 and the L2 norm of the flux, which is the gradient, with order 1
/// (Ciarlet 2002, chapter 3: the solution is smooth inside each layer and the mesh conforms to
/// the interface).
const FORMAL_ORDERS: [f64; 3] = [2.0, 1.0, 1.0];
const NORMS: [&str; 3] = ["T L2 [K]", "q left L2 [W/m^2]", "q right L2 [W/m^2]"];

#[derive(Debug)]
struct Level {
    divisions: usize,
    cells: usize,
    spacing_m: f64,
    /// Smallest cell volume divided by the volume `h^3/6` of a cell of the unperturbed mesh.
    smallest_volume_ratio: f64,
    /// Root mean square error of the temperature in K and of each layer's flux in W/m^2.
    errors: [f64; 3],
    iterations: usize,
}

/// A hash of `seed` mapped to [-1, 1], from the SplitMix64 finalizer, so a perturbed mesh is the
/// same on every platform and in every run.
fn unit_hash(seed: u64) -> f64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1_u64 << 53) as f64 * 2.0 - 1.0
}

/// Displaces the vertices of the block of `structured_block`, keeping the exterior planes and the
/// interface plane x = `LAYER_M` where they are: a vertex on such a plane moves only within it, so
/// every named boundary region and the interface stay planar.
fn perturb(mesh: &mut caekit::TetraMesh, divisions: usize, amplitude_m: f64) {
    let side = divisions + 1;
    for (index, vertex) in mesh.vertices_m.iter_mut().enumerate() {
        let lattice = [index / (side * side), index / side % side, index % side];
        for axis in 0..3 {
            let fixed = lattice[axis] == 0
                || lattice[axis] == divisions
                || (axis == 0 && lattice[axis] == divisions / 2);
            if !fixed {
                vertex[axis] += amplitude_m * unit_hash((3 * index + axis) as u64);
            }
        }
    }
}

/// Tetrahedron volume in m^3.
fn tetrahedron_volume(corners: [[f64; 3]; 4]) -> f64 {
    let edge = |i: usize| std::array::from_fn::<f64, 3, _>(|a| corners[i][a] - corners[0][a]);
    let (u, v, w) = (edge(1), edge(2), edge(3));
    let cross = [
        v[1] * w[2] - v[2] * w[1],
        v[2] * w[0] - v[0] * w[2],
        v[0] * w[1] - v[1] * w[0],
    ];
    (u[0] * cross[0] + u[1] * cross[1] + u[2] * cross[2]).abs() / 6.0
}

/// Barycentric points and weights of a rule on the tetrahedron, exact to degree 5. The weights sum
/// to 1 and multiply the cell volume. It is the Duffy map of the cube with 4-point Gauss-Legendre
/// in each direction: the map's Jacobian adds degree 2 in the first variable and 4 points are
/// exact to degree 7 = 5 + 2. The squared error of a P1 field against a quadratic is of degree 4
/// and the squared flux error against a linear field of degree 2, so every integral below is
/// exact up to rounding; the rule's degree exceeds the integrands' own.
fn tetrahedron_rule() -> Vec<([f64; 4], f64)> {
    // Gauss-Legendre on [0, 1] with 4 points: (node, weight).
    const NODES: [(f64, f64); 4] = [
        (0.069_431_844_202_973_71, 0.173_927_422_568_726_93),
        (0.330_009_478_207_571_87, 0.326_072_577_431_273_07),
        (0.669_990_521_792_428_1, 0.326_072_577_431_273_07),
        (0.930_568_155_797_026_3, 0.173_927_422_568_726_93),
    ];
    let mut rule = Vec::new();
    for (u, wu) in NODES {
        for (v, wv) in NODES {
            for (w, ww) in NODES {
                let lambda = [u, v * (1.0 - u), w * (1.0 - u) * (1.0 - v)];
                rule.push((
                    [
                        1.0 - lambda[0] - lambda[1] - lambda[2],
                        lambda[0],
                        lambda[1],
                        lambda[2],
                    ],
                    6.0 * wu * wv * ww * (1.0 - u).powi(2) * (1.0 - v),
                ));
            }
        }
    }
    rule
}

/// Interface values of the analytical two-layer solution for sources `q` in W/m^3: the flux `q_I`
/// in W/m^2 along +x and the temperature `T_I` in K at x = `LAYER_M`.
fn interface_state(q: [f64; 2]) -> (f64, f64) {
    let source_drop = CONDUCTIVITY.map(|k| LAYER_M.powi(2) / (2.0 * k));
    let q_interface = (END_TEMPERATURES_K[0] - END_TEMPERATURES_K[1] + q[0] * source_drop[0]
        - q[1] * source_drop[1])
        / (LAYER_M / CONDUCTIVITY[0] + LAYER_M / CONDUCTIVITY[1]);
    let t_interface =
        END_TEMPERATURES_K[0] - q_interface * LAYER_M / CONDUCTIVITY[0] + q[0] * source_drop[0];
    (q_interface, t_interface)
}

/// The model of one level: the perturbed block, the two layers, the sources `q` in W/m^3 and the
/// relative linear-solver tolerance divided by `tolerance_divisor`.
fn layered_model(divisions: usize, q: [f64; 2], tolerance_divisor: f64) -> (Model, f64) {
    let mut model = example();
    model.linear_solver.relative_residual_tolerance /= tolerance_divisor;
    let multi = case(&mut model);
    multi.mesh = structured_block([CROSS_SECTION_M; 3], divisions);
    let spacing_m = CROSS_SECTION_M / divisions as f64;
    perturb(&mut multi.mesh, divisions, PERTURBATION * spacing_m);
    for (i, volume) in multi.thermal.volume_regions.iter_mut().enumerate() {
        volume.cells.clear();
        volume.volumetric_heat_source_w_m3 = q[i];
    }
    for (i, &cell) in multi.mesh.cells.iter().enumerate() {
        let x = interpolate(cell.map(|node| multi.mesh.vertices_m[node][0]), [0.25; 4]);
        multi.thermal.volume_regions[usize::from(x > LAYER_M)]
            .cells
            .push(i);
    }
    (model, spacing_m)
}

/// Solves one level and measures the three error norms against the analytical solution for
/// sources `q`.
fn measure_level(divisions: usize, q: [f64; 2], tolerance_divisor: f64) -> Level {
    let (mut model, spacing_m) = layered_model(divisions, q, tolerance_divisor);
    let report = solve(&model).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    let (nodes, nodal_t) = temperatures(&report);
    let (q_interface, t_interface) = interface_state(q);
    let rule = tetrahedron_rule();
    let weight_sum: f64 = rule.iter().map(|(_, w)| w).sum();
    assert!(
        (weight_sum - 1.0).abs() < RULE_WEIGHT_SUM_TOL,
        "{weight_sum}"
    );
    let cells = &case(&mut model).mesh.cells;
    let mut temperature_error = 0.0;
    let mut flux_error = [0.0; 2];
    let mut volume = [0.0; 2];
    let mut smallest_volume = f64::INFINITY;
    for (&cell, &[qx, qy, qz]) in cells.iter().zip(fluxes(&report)) {
        let corners = cell.map(|node| nodes[node]);
        let cell_volume = tetrahedron_volume(corners);
        smallest_volume = smallest_volume.min(cell_volume);
        let xs = corners.map(|corner| corner[0]);
        let ts = cell.map(|node| nodal_t[node]);
        let layer = usize::from(interpolate(xs, [0.25; 4]) > LAYER_M);
        for &(weights, weight) in &rule {
            let z = interpolate(xs, weights) - LAYER_M;
            let expected_t =
                t_interface - (q_interface * z + 0.5 * q[layer] * z * z) / CONDUCTIVITY[layer];
            let expected_q = q_interface + q[layer] * z;
            let t = interpolate(ts, weights);
            temperature_error += weight * cell_volume * (t - expected_t).powi(2);
            flux_error[layer] +=
                weight * cell_volume * ((qx - expected_q).powi(2) + qy.powi(2) + qz.powi(2));
            volume[layer] += weight * cell_volume;
        }
    }
    let total_volume = volume[0] + volume[1];
    let level = Level {
        divisions,
        cells: cells.len(),
        spacing_m,
        smallest_volume_ratio: smallest_volume / (spacing_m.powi(3) / 6.0),
        errors: [
            (temperature_error / total_volume).sqrt(),
            (flux_error[0] / volume[0]).sqrt(),
            (flux_error[1] / volume[1]).sqrt(),
        ],
        iterations: report.convergence[0].iterations,
    };
    // The mesh fills two layers of the cross section: the integrated volume checks the rule and
    // the perturbation.
    close(
        total_volume,
        2.0 * LAYER_M * CROSS_SECTION_M.powi(2),
        VOLUME_RELATIVE_TOL * 2.0 * LAYER_M * CROSS_SECTION_M.powi(2),
    );
    assert!(level.smallest_volume_ratio > 0.0, "{level:?}");
    level
}

/// The band rule of `docs/design/blueprint.md` section 11.2 for one norm of `levels`
/// (`caekit_numerics::verify::band_check`): the fit over the finest three levels against the
/// formal order, with the half-width from the measured spread, and the shrink rate of the
/// distances to the formal order.
fn study_band(levels: &[Level], norm: usize) -> BandCheck {
    let spacings: Vec<f64> = levels.iter().map(|level| level.spacing_m).collect();
    let errors: Vec<f64> = levels.iter().map(|level| level.errors[norm]).collect();
    band_check(&spacings, &errors, FORMAL_ORDERS[norm]).unwrap()
}

/// Solves every study level for sources `q`, one thread per level where the target has threads.
///
/// `wasm32-wasip1` cannot spawn threads, so there the levels are solved one after another. The
/// levels are independent and each solve is deterministic, so both paths give the same values.
#[cfg(target_os = "wasi")]
fn measure_all(q: [f64; 2], tolerance_divisor: f64) -> Vec<Level> {
    STUDY_DIVISIONS
        .iter()
        .map(|&divisions| measure_level(divisions, q, tolerance_divisor))
        .collect()
}

/// Solves every study level for sources `q`, one thread per level.
#[cfg(not(target_os = "wasi"))]
fn measure_all(q: [f64; 2], tolerance_divisor: f64) -> Vec<Level> {
    std::thread::scope(|scope| {
        let jobs: Vec<_> = STUDY_DIVISIONS
            .iter()
            .map(|&divisions| scope.spawn(move || measure_level(divisions, q, tolerance_divisor)))
            .collect();
        jobs.into_iter().map(|job| job.join().unwrap()).collect()
    })
}

/// The levels with the sources at the example's solver tolerance. Computed once and shared by the
/// study and the trap check, which takes its band from them.
fn sourced_levels() -> &'static [Level] {
    static LEVELS: std::sync::OnceLock<Vec<Level>> = std::sync::OnceLock::new();
    LEVELS.get_or_init(|| measure_all(SOURCES, 1.0))
}

#[test]
fn a_curved_profile_converges_in_temperature_and_in_each_layer_flux() {
    assert!(STUDY_DIVISIONS.len() >= 4, "VER-003 needs four levels");
    for pair in STUDY_DIVISIONS.windows(2) {
        assert_eq!(
            pair[1],
            2 * pair[0],
            "levels must be self-similar with r = 2"
        );
    }
    let budget = algebraic_error_budget(RATIO).unwrap();
    eprintln!(
        "algebraic budget delta = {ORDER_SHIFT_BUDGET} ln({RATIO})/2 = {budget:.6e}; \
         perturbation {PERTURBATION} h"
    );
    let levels = sourced_levels();
    let tight = measure_all(SOURCES, TOLERANCE_DIVISOR);
    for (level, tight) in levels.iter().zip(&tight) {
        eprintln!(
            "{:>2} divisions, {:>6} cells, h = {:.6e} m, smallest cell {:.4} h^3/6, \
             {} iterations; errors {:.8e} K, {:.8e} and {:.8e} W/m^2",
            level.divisions,
            level.cells,
            level.spacing_m,
            level.smallest_volume_ratio,
            level.iterations,
            level.errors[0],
            level.errors[1],
            level.errors[2],
        );
        for norm in 0..3 {
            let check = algebraic_check(level.errors[norm], tight.errors[norm], RATIO).unwrap();
            eprintln!(
                "    algebraic change of {}: {:.3e} (budget {:.3e})",
                NORMS[norm], check.relative_change, check.budget
            );
            assert!(check.within_budget(), "{:?}", check.warning());
        }
    }
    // Every norm is printed before any verdict, so a failing study shows all of its measurements.
    let mut failures = Vec::new();
    for norm in 0..3 {
        let check = study_band(levels, norm);
        let errors: Vec<f64> = levels.iter().map(|level| level.errors[norm]).collect();
        // Errors from the finest level to the coarsest, as `classify` takes them.
        let fine_to_coarse: Vec<f64> = errors.iter().rev().copied().collect();
        assert_eq!(classify(&fine_to_coarse), Ok(Behaviour::Monotone));
        eprintln!(
            "{}: pairwise orders {:.6?}, fitted (finest three) {:.6}, spread {:.6}, \
             half-width {:.6}, band [{:.6}, {:.6}], finest distance {:.6}, contraction {:.4}",
            NORMS[norm],
            check.pairwise_orders,
            check.fitted_order,
            check.spread,
            check.half_width,
            check.band[0],
            check.band[1],
            check.finest_distance,
            check.contraction
        );
        if let Some(failure) = check.failure() {
            failures.push(format!("{}: {failure}", NORMS[norm]));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn the_band_rejects_the_study_without_the_source_term() {
    // Trap check. With no source each layer is linear in x and the interface is a plane of
    // vertices, so P1 reproduces the solution on any mesh: the errors are the solver's rounding
    // and tolerance, not a discretization error. The study rejects them by its two parts: the
    // band rule, and the algebraic check against the solve with the tighter tolerance. The band
    // rule alone does not always reject noise, because the noise has a large spread and the
    // band follows the spread, so the test also requires the fitted order to leave the band that
    // the sourced study meets.
    let levels = measure_all([0.0; 2], 1.0);
    let tight = measure_all([0.0; 2], TOLERANCE_DIVISOR);
    let sourced = sourced_levels();
    for norm in 0..3 {
        let trap = study_band(&levels, norm);
        let reference = study_band(sourced, norm);
        let changes: Vec<f64> = levels
            .iter()
            .zip(&tight)
            .map(|(level, tight)| {
                let check = algebraic_check(level.errors[norm], tight.errors[norm], RATIO).unwrap();
                assert_eq!(check.within_budget(), check.warning().is_none());
                check.relative_change
            })
            .collect();
        let budget = algebraic_error_budget(RATIO).unwrap();
        let changes_shown: Vec<String> = changes.iter().map(|c| format!("{c:.3e}")).collect();
        let shown: Vec<String> = levels
            .iter()
            .map(|level| format!("{:.3e}", level.errors[norm]))
            .collect();
        eprintln!(
            "trap {}: errors {shown:?}, pairwise orders {:.6?}, fitted {:.6}, spread {:.6}, \
             band [{:.6}, {:.6}], contraction {:.4}, rule: {:?}; algebraic changes {changes_shown:?} \
             (budget {budget:.3e}); band of the sourced study [{:.6}, {:.6}]",
            NORMS[norm],
            trap.pairwise_orders,
            trap.fitted_order,
            trap.spread,
            trap.band[0],
            trap.band[1],
            trap.contraction,
            trap.failure(),
            reference.band[0],
            reference.band[1],
        );
        assert!(
            trap.failure().is_some() || changes.iter().any(|&change| change > budget),
            "{}: neither the band rule nor the algebraic check rejects the study",
            NORMS[norm]
        );
        assert!(
            (trap.fitted_order - FORMAL_ORDERS[norm]).abs() > reference.half_width,
            "{}: fitted order {}",
            NORMS[norm],
            trap.fitted_order
        );
    }
}

#[test]
#[ignore = "run in a separate native process to measure peak RSS"]
fn measure_sourced_layers() {
    let divisions = std::env::var("CAEKIT_REFINEMENT_DIVISIONS")
        .expect("set CAEKIT_REFINEMENT_DIVISIONS to 4, 8, or 16")
        .parse()
        .unwrap();
    assert!([4, 8, 16].contains(&divisions));
    sourced_layers(divisions);
}

#[test]
fn invalid_volume_assignments_fail_validation_and_solving() {
    type Change = fn(&mut MultimaterialHeatConduction);
    let changes: [(Change, &str, &str); 8] = [
        (
            |case| {
                case.thermal.volume_regions[0].cells.pop();
            },
            "volume_regions",
            "cell 5 has no volume region assignment",
        ),
        (
            |case| case.thermal.volume_regions[1].cells.push(0),
            "volume_regions[1].cells[6]",
            "volume region \"right\" assigns cell 0, already assigned to \"left\"",
        ),
        (
            |case| case.thermal.volume_regions[0].cells.push(60),
            "volume_regions[0].cells[6]",
            "volume region \"left\" references invalid cell 60",
        ),
        (
            |case| case.thermal.volume_regions[1].name = "left".into(),
            "volume_regions[1].name",
            "duplicate volume region name \"left\"",
        ),
        (
            |case| case.thermal.volume_regions[1].name = " ".into(),
            "volume_regions[1].name",
            "must be nonempty",
        ),
        (
            |case| case.thermal.volume_regions[1].material_id = "missing".into(),
            "volume_regions[1].material_id",
            "volume region \"right\" references unknown material \"missing\"",
        ),
        (
            |case| case.thermal.materials[1].id = case.thermal.materials[0].id.clone(),
            "materials[1].id",
            "duplicate material ID \"high-k\"",
        ),
        (
            |case| case.thermal.materials[0].thermal_conductivity_w_m_k = 0.0,
            "materials[0].thermal_conductivity_w_m_k",
            "must be positive and finite",
        ),
    ];
    for (change, field, message) in changes {
        let mut model = example();
        change(case(&mut model));
        let data = serde_json::to_vec(&model).unwrap();
        for error in [
            model.validate().unwrap_err(),
            Model::from_json(&data).unwrap_err(),
            solve(&model).unwrap_err(),
        ] {
            invalid_at(error, &format!("physics.thermal.{field}"), message);
        }
    }
    for conductivity in [f64::INFINITY, f64::NAN] {
        let mut model = example();
        case(&mut model).thermal.materials[0].thermal_conductivity_w_m_k = conductivity;
        invalid_at(
            model.validate().unwrap_err(),
            "physics.thermal.materials[0].thermal_conductivity_w_m_k",
            "must be positive and finite",
        );
    }
}
