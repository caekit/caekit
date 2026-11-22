//! The `caekit` executable on the saved reference models: `check` and `run` output, and the
//! exit codes for invalid input, calculation errors and runs that stop without convergence.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use caekit::{Model, RunReport, StopReason, solve};
use serde_json::{Value, json};

#[test]
fn axial_bar() {
    check_and_run("axial_bar.json");
}

#[test]
fn elastic_solid() {
    check_and_run("elastic_solid.json");
}

#[test]
fn laminar_channel() {
    check_and_run("laminar_channel.json");
}

#[test]
fn cooling_channel() {
    check_and_run("cooling_channel.json");
}

#[test]
fn heat_conduction() {
    check_and_run("heat_conduction.json");
}

#[test]
fn multimaterial_heat_conduction() {
    check_and_run("multimaterial_heat_conduction.json");
}

#[test]
fn thermoelastic_solid() {
    check_and_run("thermoelastic_solid.json");
}

#[test]
fn invalid_thermal_boundary_inputs_use_exit_one() {
    for (conditions, field, message) in [
        (
            json!([{"kind": "heat_flux", "region": "x0", "inward_heat_flux_w_m2": 1000}]),
            "physics.thermal.boundary_conditions",
            "determine absolute temperature",
        ),
        (
            json!([
                {"kind": "temperature", "region": "x0", "temperature_k": 300},
                {"kind": "temperature", "region": "y0", "temperature_k": 400}
            ]),
            "physics.thermal.boundary_conditions[1].temperature_k",
            "conflicting prescribed temperatures at vertex",
        ),
    ] {
        let mut model = fixture_json("heat_conduction.json");
        model["physics"]["thermal"]["boundary_conditions"] = conditions;
        let path = TempModel::new(&model);
        for command in ["check", "run"] {
            let output = run(command, &path.0);
            assert_exit(&output, 1, command);
            assert!(output.stdout.is_empty());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(error.contains(&format!("{field}:")), "{error}");
            assert!(error.contains(message), "{error}");
        }
    }
}

#[test]
fn invalid_volume_assignments_use_exit_one() {
    type Change = fn(&mut Value);
    let changes: [(Change, &str, &str); 7] = [
        (
            |thermal| thermal["volume_regions"][0]["cells"] = json!([0, 1, 2, 3, 4]),
            "volume_regions",
            "cell 5 has no volume region assignment",
        ),
        (
            |thermal| thermal["volume_regions"][1]["cells"] = json!([0, 6, 7, 8, 9, 10, 11]),
            "volume_regions[1].cells[0]",
            "volume region \"right\" assigns cell 0, already assigned to \"left\"",
        ),
        (
            |thermal| thermal["volume_regions"][0]["cells"][0] = json!(99),
            "volume_regions[0].cells[0]",
            "volume region \"left\" references invalid cell 99",
        ),
        (
            |thermal| thermal["volume_regions"][1]["name"] = json!("left"),
            "volume_regions[1].name",
            "duplicate volume region name \"left\"",
        ),
        (
            |thermal| thermal["volume_regions"][1]["material_id"] = json!("missing"),
            "volume_regions[1].material_id",
            "volume region \"right\" references unknown material \"missing\"",
        ),
        (
            |thermal| thermal["materials"][0]["thermal_conductivity_w_m_k"] = json!(-1),
            "materials[0].thermal_conductivity_w_m_k",
            "must be positive and finite",
        ),
        (
            |thermal| thermal["boundary_conditions"][0]["region"] = json!("left"),
            "boundary_conditions[0].region",
            "unknown exterior region \"left\"",
        ),
    ];
    for (change, field, message) in changes {
        let mut model = fixture_json("multimaterial_heat_conduction.json");
        change(&mut model["physics"]["thermal"]);
        assert_check_error(
            &TempModel::new(&model),
            &format!("physics.thermal.{field}"),
            message,
        );
    }

    let mut model = fixture_json("multimaterial_heat_conduction.json");
    model["physics"]["mesh"]["boundary_regions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "interior", "faces": [[4, 5, 7]]}));
    assert_check_error(
        &TempModel::new(&model),
        "physics.mesh.boundary_regions[6].faces[0]",
        "region references an unknown, interior, or already assigned face",
    );
}

#[test]
fn unreadable_conductivity_identifies_the_material_field() {
    let model = std::fs::read_to_string(fixture("multimaterial_heat_conduction.json")).unwrap();
    for (token, field, message) in [
        (
            "1e400",
            "physics.thermal.materials[0].thermal_conductivity_w_m_k",
            "number out of range",
        ),
        (
            "NaN",
            "physics.thermal.materials[0].thermal_conductivity_w_m_k",
            "expected value",
        ),
        (
            "null",
            "invalid model JSON: thermal_conductivity_w_m_k",
            "invalid type",
        ),
    ] {
        let invalid = model.replacen(
            "\"thermal_conductivity_w_m_k\": 10",
            &format!("\"thermal_conductivity_w_m_k\": {token}"),
            1,
        );
        assert_check_error(&TempModel::from_bytes(invalid.as_bytes()), field, message);
    }
}

fn assert_check_error(path: &TempModel, field: &str, message: &str) {
    let output = run("check", &path.0);
    assert_exit(&output, 1, "check");
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains(&format!("{field}:")), "{error}");
    assert!(error.contains(message), "{error}");
}

#[test]
fn unphysical_calculated_temperatures_use_exit_one() {
    let mut model = fixture_json("heat_conduction.json");
    model["physics"]["thermal"]["volumetric_heat_source_w_m3"] = json!(-1e9);
    let path = TempModel::new(&model);
    assert_exit(&run("check", &path.0), 0, "check");
    let output = run("run", &path.0);
    assert_exit(&output, 1, "run");
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("solid temperature at vertex")
            && error.contains("solution must be above absolute zero"),
        "{error}"
    );
}

#[test]
fn a_stage_iteration_limit_uses_exit_two_and_preserves_the_trace() {
    let settings = json!({
        // The default tolerances written out; the limit of one iteration is what stops the stage
        // before it reaches the 1e-10 relative threshold.
        "relative_residual_tolerance": 1e-10,
        "absolute_residual_tolerance": 0,
        "max_iterations": 1
    });
    let mut thermal_limit = fixture_json("thermoelastic_solid.json");
    thermal_limit["physics"]["thermal_solver"] = settings.clone();
    thermal_limit["physics"]["thermal"]["boundary_conditions"] = json!([
        {"kind": "heat_flux", "region": "x0", "inward_heat_flux_w_m2": 1000},
        {"kind": "convection", "region": "x1", "ambient_temperature_k": 300,
         "film_coefficient_w_m2_k": 100}
    ]);
    let mut conduction_limit = fixture_json("multimaterial_heat_conduction.json");
    conduction_limit["linear_solver"] = settings.clone();
    let mut equilibration_limit = conduction_limit.clone();
    let physics = &mut equilibration_limit["physics"];
    // Fix every vertex so the temperature equations converge immediately.
    // Two separately heated parts of the left layer make the contact solve nontrivial.
    let conditions: Vec<_> = physics["mesh"]["boundary_regions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|boundary| {
            json!({"kind": "temperature", "region": boundary["name"], "temperature_k": 300})
        })
        .collect();
    physics["thermal"]["boundary_conditions"] = json!(conditions);
    let regions = physics["thermal"]["volume_regions"].as_array_mut().unwrap();
    regions[0]["cells"] = json!([0, 1, 2]);
    regions[0]["volumetric_heat_source_w_m3"] = json!(50_000);
    regions.swap(0, 1);
    regions.push(json!({
        "name": "middle", "material_id": "high-k", "cells": [3, 4, 5],
        "volumetric_heat_source_w_m3": 150_000,
    }));

    let mut mechanical_limit = fixture_json("thermoelastic_solid.json");
    mechanical_limit["linear_solver"] = settings;
    for (model, units) in [
        (conduction_limit, &["W"][..]),
        (equilibration_limit, &["W", "W"][..]),
        (thermal_limit, &["W"][..]),
        (mechanical_limit, &["W", "N"][..]),
    ] {
        let path = TempModel::new(&model);
        assert_exit(&run("check", &path.0), 0, "check");
        let output = run("run", &path.0);
        assert_exit(&output, 2, "run");
        let report: RunReport = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report.stop_reason, StopReason::IterationLimit);
        assert!(report.quantities.is_empty());
        assert!(report.balances.is_empty());
        assert_eq!(report.convergence.len(), units.len());
        for (index, (stage, &unit)) in report.convergence.iter().zip(units).enumerate() {
            assert_eq!(stage.residual_unit, unit);
            let (reason, within_tolerance) = if index == units.len() - 1 {
                // The `max_iterations` of `settings`.
                assert_eq!(stage.iterations, 1);
                (StopReason::IterationLimit, false)
            } else {
                (StopReason::Converged, true)
            };
            assert_eq!(stage.stop_reason, reason);
            assert_eq!(
                stage.final_true_residual_l2.unwrap() <= stage.residual_threshold.unwrap(),
                within_tolerance
            );
        }
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
}

fn fixture_json(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(fixture(name)).unwrap()).unwrap()
}

fn check_and_run(name: &str) {
    let path = fixture(name);
    let model = Model::from_json(&std::fs::read(&path).unwrap())
        .unwrap_or_else(|error| panic!("{name}: {error}"));
    let report = solve(&model).unwrap_or_else(|error| panic!("{name}: {error}"));
    assert_eq!(report.stop_reason, StopReason::Converged, "{name}");
    for (command, expected) in [
        ("check", serde_json::to_value(model.validate().unwrap())),
        ("run", serde_json::to_value(report)),
    ] {
        let output = run(command, &path);
        assert_exit(&output, 0, &format!("{command} {name}"));
        let actual: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{command} {name}: invalid JSON: {error}"));
        assert_eq!(actual, expected.unwrap(), "{command} {name}");
    }
}

fn run(command: &str, path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_caekit"))
        .arg(command)
        .arg(path)
        .output()
        .unwrap()
}

fn assert_exit(output: &Output, expected: i32, command: &str) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "{command}: measured {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

struct TempModel(PathBuf);

impl TempModel {
    fn new(model: &Value) -> Self {
        Self::from_bytes(&serde_json::to_vec(model).unwrap())
    }

    fn from_bytes(model: &[u8]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "caekit-test-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = File::create_new(&path).unwrap();
        let fixture = Self(path);
        file.write_all(model).unwrap();
        fixture
    }
}

impl Drop for TempModel {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
