//! Each balance of a reference model whose test functions sum to one stays within
//! `weight · √n_free · ‖r‖₂` of zero, where `‖r‖₂` is the true residual of the stage that solved
//! it and `n_free` the number of its free equations.
//!
//! The derivation, the balances it covers and those it does not are on the model page,
//! `docs/model.md#what-a-closed-balance-proves`. A closed balance is consistency evidence for
//! the algebraic solve, not evidence that the field is accurate; the analytical references in
//! the per-kind tests carry that.

use std::collections::BTreeSet;

use caekit::{Model, Output, Quantity, RunReport, StopReason, solve};
use serde_json::Value;

/// Weight of a balance whose test function is the constant one in every free equation (the
/// energy balances, the axial force balance and the solid force balances): the bound is then
/// `sqrt(n_free) * ||r||` itself, as the derivation of the module documentation gives.
const UNIT_WEIGHT: f64 = 1.0;
/// Absolute size of the netted terms for `assert_bounded` when no reported term of the balance
/// sums values with cancellation, as in the momentum and channel energy balances.
const NOTHING_NETTED: f64 = 0.0;

/// Solves a reference model and returns its document next to the converged report.
fn reference(input: &[u8]) -> (Value, RunReport) {
    let document: Value = serde_json::from_slice(input).unwrap();
    let report = solve(&Model::from_json(input).unwrap()).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    (document, report)
}

/// The true residual `‖b − A x‖₂` that the report gives for `stage`.
fn residual(report: &RunReport, stage: &str) -> f64 {
    let trace = report
        .convergence
        .iter()
        .find(|trace| trace.stage == stage)
        .unwrap_or_else(|| panic!("the report has no {stage} stage"));
    assert_eq!(trace.stop_reason, StopReason::Converged, "{stage}");
    trace.final_true_residual_l2.unwrap()
}

/// `weight · √n_free · ‖r‖₂`, the bound of a balance on `n_free` equations with residual `r`.
fn residual_bound(weight: f64, n_free: usize, r: f64) -> f64 {
    weight * (n_free as f64).sqrt() * r
}

fn quantity(report: &RunReport, output: Output) -> &Quantity {
    report
        .quantities
        .get(&output)
        .unwrap_or_else(|| panic!("the reference model requests {output:?}"))
}

fn scalar(report: &RunReport, output: Output) -> f64 {
    match quantity(report, output) {
        Quantity::Scalar { value, .. } => *value,
        quantity => panic!("expected a scalar, got {quantity:?}"),
    }
}

/// Sum of the absolute values of a `by_region` output.
fn absolute_by_region(report: &RunReport, output: Output) -> f64 {
    match quantity(report, output) {
        Quantity::ByRegion { values, .. } => values.values().map(|value| value.abs()).sum(),
        quantity => panic!("expected region values, got {quantity:?}"),
    }
}

/// Absolute nodal support reactions in N: per component, and summed over all components.
fn absolute_reactions(report: &RunReport) -> ([f64; 3], f64) {
    let Quantity::VectorField { values, .. } = quantity(report, Output::NodalReaction) else {
        panic!("expected a vector field of nodal reactions");
    };
    let mut component = [0.0; 3];
    for reaction in values {
        for (sum, value) in component.iter_mut().zip(reaction) {
            *sum += value.abs();
        }
    }
    (component, component.iter().sum())
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap()
}

fn count(values: &Value) -> usize {
    values.as_array().unwrap().len()
}

/// Vertices of the faces of the named exterior region of a tetrahedral mesh.
fn region_vertices(mesh: &Value, name: &str) -> BTreeSet<u64> {
    let region = mesh["boundary_regions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|region| region["name"] == name)
        .unwrap_or_else(|| panic!("the mesh has no region {name}"));
    region["faces"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|face| face.as_array().unwrap().iter().map(|v| v.as_u64().unwrap()))
        .collect()
}

/// Free temperature unknowns: the vertices not on a prescribed-temperature region.
fn free_temperatures(mesh: &Value, conditions: &Value) -> usize {
    let prescribed: BTreeSet<_> = conditions
        .as_array()
        .unwrap()
        .iter()
        .filter(|condition| condition["kind"] == "temperature")
        .flat_map(|condition| region_vertices(mesh, condition["region"].as_str().unwrap()))
        .collect();
    count(&mesh["vertices_m"]) - prescribed.len()
}

/// Free displacement components: three per vertex, less each prescribed (vertex, component).
fn free_displacements(mesh: &Value, conditions: &Value) -> usize {
    let mut prescribed = BTreeSet::new();
    for condition in conditions.as_array().unwrap() {
        if condition["kind"] != "displacement" {
            continue;
        }
        let vertices = region_vertices(mesh, condition["region"].as_str().unwrap());
        for (component, value) in condition["components_m"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            if !value.is_null() {
                prescribed.extend(vertices.iter().map(|&vertex| (vertex, component)));
            }
        }
    }
    3 * count(&mesh["vertices_m"]) - prescribed.len()
}

/// Largest distance in m of a vertex from the first vertex, the reference point of the moments.
fn lever_m(mesh: &Value) -> f64 {
    let vertices = mesh["vertices_m"].as_array().unwrap();
    let origin: Vec<f64> = vertices[0].as_array().unwrap().iter().map(number).collect();
    vertices
        .iter()
        .map(|vertex| {
            let point = vertex.as_array().unwrap().iter().map(number);
            point
                .zip(&origin)
                .map(|(x, o)| (x - o) * (x - o))
                .sum::<f64>()
                .sqrt()
        })
        .fold(0.0, f64::max)
}

/// Checks `|imbalance| ≤ bound + rounding` for the named balance.
///
/// `entities` is the number of mesh entities (vertices or cells) whose values the balance sums,
/// and `netted` the absolute size of the per-entity or per-region values that a reported term
/// already sums with cancellation, such as the reactions at both ends of a block. See
/// [`rounding_allowance`].
fn assert_bounded(report: &RunReport, name: &str, bound: f64, entities: usize, netted: f64) {
    let balance = report
        .balances
        .iter()
        .find(|balance| balance.name == name)
        .unwrap_or_else(|| panic!("the report has no {name} balance"));
    let magnitude = balance.terms.values().map(|term| term.abs()).sum::<f64>() + netted;
    let rounding = rounding_allowance(entities, magnitude);
    println!(
        "{}: {name} imbalance {:.3e} {unit}, bound {bound:.3e} {unit}, rounding allowance {rounding:.3e} {unit}",
        report.model_name,
        balance.imbalance,
        unit = balance.unit,
    );
    assert!(
        balance.imbalance.abs() <= bound + rounding,
        "{name}: |imbalance| {:.6e} exceeds {bound:.6e} + {rounding:.6e} {}",
        balance.imbalance.abs(),
        balance.unit
    );
}

/// Binary64 allowance for the parts of the identity that hold only in exact arithmetic.
///
/// The identity `imbalance = Σ r_free` uses that the assembled rows sum to zero against a
/// constant or rigid test vector, and it ignores the rounding of the sums that form the terms.
/// Neither holds exactly in binary64, so a balance whose residual is at the rounding floor can
/// exceed the bound: the heat-conduction reference solves its single free equation to a true
/// residual of exactly zero. A recursive sum of `m` values is within `(m − 1) ε Σ|values|` of
/// the exact sum (Higham, *Accuracy and Stability of Numerical Algorithms*, 2nd ed., § 4.2). The
/// balance sums one value per mesh entity and then its terms, so this allows `(entities + 1) ε`
/// times the absolute size of those values. It is the allowance of the coupled thermoelastic
/// force check in `crates/caekit-mechanics/src/elastic_solid.rs`. It does not bound cancellation
/// inside an assembled nodal value; the measured margins are on the model page.
fn rounding_allowance(entities: usize, magnitude: f64) -> f64 {
    (entities as f64 + 1.0) * f64::EPSILON * magnitude
}

#[test]
fn axial_bar_force_balance_is_bounded_by_the_residual() {
    let (document, report) = reference(include_bytes!("../../../examples/axial_bar.json"));
    let physics = &document["physics"];
    let vertices = count(&physics["mesh"]["vertices_m"]);
    let fixed = physics["boundary_conditions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|condition| condition["kind"] == "displacement")
        .count();
    let r = residual(&report, "elastic_equilibrium");
    let netted = absolute_by_region(&report, Output::ReactionForce);
    let bound = residual_bound(UNIT_WEIGHT, vertices - fixed, r);
    assert_bounded(&report, "global_axial_force", bound, vertices, netted);
}

/// Factor from the stage residual in Pa to the momentum balance in N.
///
/// The finite-volume rows are per unit wall area, so the balance is the wall area `W L` times
/// the sum of the cell residuals. The volume-flow drive solves for a 1 Pa/m reference gradient
/// and scales the velocity by `pressure_loss / (1 Pa/m · L)`, which scales the residual by the
/// same factor.
fn momentum_weight(geometry: &Value, report: &RunReport) -> f64 {
    let length = number(&geometry["length_m"]);
    let width = number(&geometry["width_m"]);
    width * length * (scalar(report, Output::PressureLoss) / length)
}

#[test]
fn laminar_channel_momentum_balance_is_bounded_by_the_residual() {
    let (document, report) = reference(include_bytes!("../../../examples/laminar_channel.json"));
    let physics = &document["physics"];
    assert_eq!(physics["drive"]["kind"], "volume_flow");
    let cells = count(&physics["mesh"]["vertices_m"]) - 1;
    let r = residual(&report, "unit_gradient_axial_momentum");
    let bound = residual_bound(momentum_weight(&physics["geometry"], &report), cells, r);
    assert_bounded(
        &report,
        "channel_axial_momentum",
        bound,
        cells,
        NOTHING_NETTED,
    );
}

#[test]
fn cooling_channel_balances_are_bounded_by_the_residuals() {
    let (document, report) = reference(include_bytes!("../../../examples/cooling_channel.json"));
    let physics = &document["physics"];
    assert_eq!(physics["flow"]["drive"]["kind"], "volume_flow");
    let flow_cells = count(&physics["flow"]["mesh"]["vertices_m"]) - 1;
    let flow_r = residual(&report, "unit_gradient_axial_momentum");
    let weight = momentum_weight(&physics["flow"]["geometry"], &report);
    let bound = residual_bound(weight, flow_cells, flow_r);
    assert_bounded(
        &report,
        "channel_axial_momentum",
        bound,
        flow_cells,
        NOTHING_NETTED,
    );

    // The coupled residual stacks one energy equation per wall panel and per coolant cell; the
    // wall and coolant balances each sum one of the two blocks.
    let wall = count(&physics["wall"]["axial_mesh"]["vertices_m"]) - 1;
    let coolant = count(&physics["coolant"]["axial_mesh"]["vertices_m"]) - 1;
    let r = residual(&report, "coupled_wall_coolant_energy");
    for (name, n_free) in [
        ("wall_energy", wall),
        ("coolant_energy", coolant),
        ("system_energy", wall + coolant),
    ] {
        assert_bounded(
            &report,
            name,
            residual_bound(UNIT_WEIGHT, n_free, r),
            n_free,
            NOTHING_NETTED,
        );
    }
}

/// Checks the three force and three moment balances of a tetrahedral elasticity stage.
fn assert_solid_balances(report: &RunReport, mesh: &Value, n_free: usize) {
    let r = residual(report, "solid_elastic_equilibrium");
    let vertices = count(&mesh["vertices_m"]);
    let lever = lever_m(mesh);
    let (per_component, total) = absolute_reactions(report);
    for (axis, netted) in ["x", "y", "z"].into_iter().zip(per_component) {
        let force = residual_bound(UNIT_WEIGHT, n_free, r);
        assert_bounded(
            report,
            &format!("solid_force_{axis}"),
            force,
            vertices,
            netted,
        );
        // A moment about the first vertex tests with a rigid rotation, whose nodal values are at
        // most `lever` long.
        let moment = residual_bound(lever, n_free, r);
        let netted = lever * total;
        assert_bounded(
            report,
            &format!("solid_moment_{axis}"),
            moment,
            vertices,
            netted,
        );
    }
}

#[test]
fn elastic_solid_force_and_moment_balances_are_bounded_by_the_residual() {
    let (document, report) = reference(include_bytes!("../../../examples/elastic_solid.json"));
    let physics = &document["physics"];
    let n_free = free_displacements(&physics["mesh"], &physics["boundary_conditions"]);
    assert_solid_balances(&report, &physics["mesh"], n_free);
}

/// Checks the global `solid_energy` balance of a conduction stage.
fn assert_energy_balance(report: &RunReport, mesh: &Value, conditions: &Value) {
    let n_free = free_temperatures(mesh, conditions);
    let r = residual(report, "solid_heat_conduction");
    // The region heat rates carry the prescribed-temperature reactions before they are netted.
    let netted = absolute_by_region(report, Output::BoundaryHeatRate);
    let bound = residual_bound(UNIT_WEIGHT, n_free, r);
    assert_bounded(
        report,
        "solid_energy",
        bound,
        count(&mesh["vertices_m"]),
        netted,
    );
}

#[test]
fn heat_conduction_energy_balance_is_bounded_by_the_residual() {
    let (document, report) = reference(include_bytes!("../../../examples/heat_conduction.json"));
    let physics = &document["physics"];
    assert_energy_balance(
        &report,
        &physics["mesh"],
        &physics["thermal"]["boundary_conditions"],
    );
}

#[test]
fn multimaterial_global_energy_balance_is_bounded_by_the_residual() {
    let (document, report) = reference(include_bytes!(
        "../../../examples/multimaterial_heat_conduction.json"
    ));
    let physics = &document["physics"];
    assert_energy_balance(
        &report,
        &physics["mesh"],
        &physics["thermal"]["boundary_conditions"],
    );
}

#[test]
fn thermoelastic_balances_are_bounded_by_the_residuals() {
    let (document, report) =
        reference(include_bytes!("../../../examples/thermoelastic_solid.json"));
    let physics = &document["physics"];
    let mesh = &physics["mesh"];
    assert_energy_balance(&report, mesh, &physics["thermal"]["boundary_conditions"]);
    let n_free = free_displacements(mesh, &physics["mechanical"]["boundary_conditions"]);
    assert_solid_balances(&report, mesh, n_free);
}
