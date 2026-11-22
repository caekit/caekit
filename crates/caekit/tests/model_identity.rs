//! Frozen identities of the saved reference models.
//!
//! `model_sha256` is the SHA-256 digest of the compact serialization of the typed model with
//! defaults applied. Schema version 1 promises that an existing document keeps its meaning
//! and its digest. A change to the typed model that serializes an existing document
//! differently, such as a new field that is written with its default, a renamed key or a
//! reordered field, fails here.
//!
//! Update a digest only when the reference model itself was edited on purpose. In every
//! other case the change breaks version 1 and needs a new schema version.

#[path = "common/stopped.rs"]
mod stopped;

use caekit::{Error, Model, Output, Physics, RunReport, solve};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const FROZEN: &[(&str, &[u8], &str)] = &[
    (
        "axial_bar",
        include_bytes!("../../../examples/axial_bar.json"),
        "ef1ad1431b563c0b252bd8f872cd40941a8feebcabcfe6692ecb529e85614e17",
    ),
    (
        "cooling_channel",
        include_bytes!("../../../examples/cooling_channel.json"),
        "7e3c9e22d7e6440124ef2cbfeec4907fdbc834435d7baf4cf22f9b22ac0388c8",
    ),
    (
        "elastic_solid",
        include_bytes!("../../../examples/elastic_solid.json"),
        "faec4995d67cbf9758d4410a0e4bdf5ff2e963fb1393f7b59d67709e786cc6f5",
    ),
    (
        "heat_conduction",
        include_bytes!("../../../examples/heat_conduction.json"),
        "92a87d6342966ab22015eb3877847fb825e7ee0b63dc2184441ba35bfc5cdb8c",
    ),
    (
        "laminar_channel",
        include_bytes!("../../../examples/laminar_channel.json"),
        "6d727104492ae5145e37696bde65b6d6c76899e90a5c1e5fafd85cc47f0669fe",
    ),
    (
        "multimaterial_heat_conduction",
        include_bytes!("../../../examples/multimaterial_heat_conduction.json"),
        "f0599ab94a8ff13831daf0929f03d585811464569351324d9f591304823b3b1d",
    ),
    (
        "thermoelastic_solid",
        include_bytes!("../../../examples/thermoelastic_solid.json"),
        "a58274be1f1e0f1b3aaf7e2ddd12b46c4a2d03b7e793320dec93257ba0145804",
    ),
];

#[test]
fn reference_models_keep_their_version_1_digest() {
    for (name, document, digest) in FROZEN {
        let model = Model::from_json(document).unwrap();
        assert_eq!(model.schema_version, 1, "{name}");
        let bytes = serde_json::to_vec(&model).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), *digest, "{name}");
        let decoded = Model::from_json(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes, "{name}");
        let report = solve(&model).unwrap();
        assert_eq!(
            report.model_sha256, *digest,
            "examples/{name}.json: the serialized typed model of schema version 1 changed"
        );
    }
}

fn contract_fields(value: &mut Value, key: &str) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                contract_fields(child, key);
            }
        }
        Value::Array(array) => {
            // Structured entries carry field names, whereas numerical arrays
            // are verified by the solver references and report size test.
            if array.first().is_some_and(Value::is_object) {
                for child in array {
                    contract_fields(child, "");
                }
            } else {
                array.clear();
            }
        }
        _ if matches!(
            key,
            "kind"
                | "location"
                | "unit"
                | "residual_unit"
                | "algorithm"
                | "stage"
                | "method"
                | "stop_reason"
                | "from_region"
                | "to_region"
        ) => {}
        _ => *value = Value::Null,
    }
}

fn assert_report_contract(report: &RunReport, expected: &Value, name: &str, outcome: &str) {
    let mut fields = serde_json::to_value(report).unwrap();
    contract_fields(&mut fields, "");
    for (key, value) in fields.as_object().unwrap() {
        assert_eq!(value, &expected["fields"][key], "{name} {outcome}: {key}");
    }
    assert_eq!(fields, expected["fields"], "{name} {outcome}: missing keys");
    let output_order: Vec<_> = report.quantities.keys().collect();
    assert_eq!(
        json!(output_order),
        expected["output_order"],
        "{name} {outcome}: output order"
    );
}

#[test]
fn version_one_reports_keep_the_reference_contract() {
    // Captured from the pre-change v1 CLI, with platform and numerical data
    // removed. Keep names, stage and quantity labels, and object structure.
    let snapshot: Value =
        serde_json::from_str(include_str!("common/report_contract_v1.json")).unwrap();
    assert_eq!(snapshot.as_object().unwrap().len(), FROZEN.len());
    for (name, document, _) in FROZEN {
        let mut model = Model::from_json(document).unwrap();
        let report = solve(&model).unwrap();
        assert_report_contract(&report, &snapshot[name]["converged"], name, "converged");
        stopped::limit_iterations(&mut model);
        let report = solve(&model).unwrap();
        assert_report_contract(
            &report,
            &snapshot[name]["iteration_limit"],
            name,
            "iteration_limit",
        );
    }
}

#[test]
fn the_digest_ignores_formatting_and_key_order() {
    for (name, document, digest) in FROZEN {
        let value: serde_json::Value = serde_json::from_slice(document).unwrap();
        // serde_json orders object keys alphabetically here, unlike the saved files.
        let reordered = serde_json::to_vec_pretty(&value).unwrap();
        assert_ne!(reordered.as_slice(), *document, "{name}");
        let report = solve(&Model::from_json(&reordered).unwrap()).unwrap();
        assert_eq!(report.model_sha256, *digest, "{name}");
    }
}

fn object_paths(value: &Value, path: &str, paths: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            paths.push(path.to_owned());
            for (key, child) in object {
                let key = key.replace('~', "~0").replace('/', "~1");
                object_paths(child, &format!("{path}/{key}"), paths);
            }
        }
        Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                object_paths(child, &format!("{path}/{index}"), paths);
            }
        }
        _ => {}
    }
}

#[test]
fn reference_models_reject_unknown_fields_discriminants_and_versions() {
    for (name, document, _) in FROZEN {
        // Serializing the typed model includes settings that the fixture leaves at defaults.
        let model = Model::from_json(document).unwrap();
        let original = serde_json::to_value(model).unwrap();
        let mut paths = Vec::new();
        object_paths(&original, "", &mut paths);
        for path in paths {
            let mut invalid = original.clone();
            invalid.pointer_mut(&path).unwrap()["unknown_model_key"] = json!(true);
            assert!(
                matches!(
                    Model::from_json(&serde_json::to_vec(&invalid).unwrap()),
                    Err(Error::Json(_))
                ),
                "{name} accepted an unknown field at {path}/unknown_model_key",
            );
            if original.pointer(&path).unwrap().get("kind").is_some() {
                let mut invalid = original.clone();
                invalid.pointer_mut(&path).unwrap()["kind"] = json!("unknown_model_kind");
                assert!(
                    matches!(
                        Model::from_json(&serde_json::to_vec(&invalid).unwrap()),
                        Err(Error::Json(_))
                    ),
                    "{name} accepted an unknown kind at {path}/kind",
                );
            }
        }

        let mut invalid = original.clone();
        invalid["outputs"][0] = json!("unknown_model_output");
        assert!(
            matches!(
                Model::from_json(&serde_json::to_vec(&invalid).unwrap()),
                Err(Error::Json(_))
            ),
            "{name} accepted an unknown output"
        );
        invalid["schema_version"] = json!(2);
        assert!(
            matches!(
                Model::from_json(&serde_json::to_vec(&invalid).unwrap()),
                Err(Error::UnsupportedModelVersion(2))
            ),
            "{name} did not check schema_version before decoding outputs"
        );
    }
}

#[test]
fn reference_models_use_si_and_unit_suffixed_numeric_keys() {
    const DIMENSIONLESS: &[&str] = &[
        "cells",
        "direction",
        "faces",
        "max_iterations",
        "poisson_ratio",
        "relative_residual_tolerance",
        "schema_version",
        "transverse_direction",
    ];
    const SI_SUFFIXES: &[&str] = &[
        "_j_kg_k", "_k", "_kg_m3", "_m", "_m2", "_m3_s", "_n", "_n_m3", "_pa", "_pa_s", "_per_k",
        "_w", "_w_m2", "_w_m2_k", "_w_m3", "_w_m_k",
    ];

    fn check(value: &Value, key: &str, path: &str) {
        match value {
            Value::Number(_) => {
                // This legacy setting uses the unit of the stage it controls.
                let residual_setting = key == "absolute_residual_tolerance"
                    && (path.ends_with("/linear_solver/absolute_residual_tolerance")
                        || path.ends_with("/thermal_solver/absolute_residual_tolerance"));
                assert!(
                    residual_setting
                        || DIMENSIONLESS.contains(&key)
                        || SI_SUFFIXES.iter().any(|suffix| key.ends_with(suffix)),
                    "numeric model field {path} has no SI unit suffix",
                );
            }
            Value::Object(object) => {
                for (key, child) in object {
                    check(child, key, &format!("{path}/{key}"));
                }
            }
            Value::Array(array) => {
                for (index, child) in array.iter().enumerate() {
                    check(child, key, &format!("{path}/{index}"));
                }
            }
            _ => {}
        }
    }

    for (name, document, _) in FROZEN {
        let model = Model::from_json(document).unwrap();
        let value = serde_json::to_value(model).unwrap();
        assert_eq!(value["units"], json!("si"), "{name}");
        check(&value, "", name);
        let mut invalid = value;
        invalid["units"] = json!("us_customary");
        assert!(
            Model::from_json(&serde_json::to_vec(&invalid).unwrap()).is_err(),
            "{name}"
        );
    }
}

#[test]
fn reference_reports_are_bounded_by_requested_entities_and_iteration_limits() {
    for (name, document, _) in FROZEN {
        let mut model = Model::from_json(document).unwrap();
        let input = serde_json::to_value(&model).unwrap();
        let physics = &input["physics"];
        let report = solve(&model).unwrap();
        assert_eq!(report.quantities.len(), model.outputs.len(), "{name}");
        for &output in &model.outputs {
            let quantity = serde_json::to_value(&report.quantities[&output]).unwrap();
            let count = |value: &Value| value.as_array().unwrap().len();
            let vertices = |mesh: &Value| count(&mesh["vertices_m"]);
            let mesh = &physics["mesh"];
            let samples = match output {
                Output::AxialDisplacement
                | Output::DisplacementVector
                | Output::NodalReaction
                | Output::SolidTemperature => vertices(mesh),
                Output::AxialStress => vertices(mesh) - 1,
                Output::AxialVelocity if matches!(model.physics, Physics::CoolingChannel(_)) => {
                    vertices(&physics["flow"]["mesh"]) - 1
                }
                Output::AxialVelocity => vertices(mesh) - 1,
                Output::FluidBulkTemperature => vertices(&physics["coolant"]["axial_mesh"]) - 1,
                Output::InterfaceHeatFlux => vertices(&physics["wall"]["axial_mesh"]) - 1,
                Output::WallTemperature => {
                    (vertices(&physics["wall"]["axial_mesh"]) - 1)
                        * vertices(&physics["wall"]["thickness_mesh"])
                }
                Output::CauchyStress | Output::VonMisesStress | Output::SolidHeatFlux => {
                    count(&mesh["cells"])
                }
                Output::InterfaceHeatRate => {
                    let regions = count(&physics["thermal"]["volume_regions"]);
                    assert!(
                        count(&quantity["values"]) <= regions * (regions - 1) / 2,
                        "{name}"
                    );
                    continue;
                }
                Output::BoundaryHeatRate => {
                    assert!(
                        quantity["values"].as_object().unwrap().len()
                            <= count(&mesh["boundary_regions"]),
                        "{name}"
                    );
                    continue;
                }
                Output::EndDisplacement
                | Output::ReactionForce
                | Output::AbsolutePressure
                | Output::WallDrag => {
                    // One value per end of the bar, port of the channel or wall of the channel:
                    // two at most.
                    assert!(quantity["values"].as_object().unwrap().len() <= 2, "{name}");
                    continue;
                }
                Output::VolumeFlowRate
                | Output::MassFlowRate
                | Output::PressureLoss
                | Output::ChannelReynolds
                | Output::OutletTemperature
                | Output::HeatTransferRate
                | Output::ElasticEnergy => {
                    assert!(quantity["value"].is_number(), "{name}");
                    continue;
                }
            };
            assert_eq!(
                count(&quantity["positions_m"]),
                samples,
                "{name}: {output:?}"
            );
            assert_eq!(count(&quantity["values"]), samples, "{name}: {output:?}");
        }
        for stage in &report.convergence {
            let limit = match (&model.physics, stage.stage.as_str()) {
                (Physics::CoolingChannel(case), "coupled_wall_coolant_energy") => {
                    case.coupling_solver.max_iterations
                }
                (Physics::ThermoelasticSolid(case), "solid_heat_conduction") => {
                    case.thermal_solver.max_iterations
                }
                _ => model.linear_solver.max_iterations,
            };
            assert!(stage.iterations <= limit, "{name}: {}", stage.stage);
            assert!(
                stage.residual_l2_history.len() <= stage.iterations + 1,
                "{name}: {}",
                stage.stage
            );
        }

        // Every fixture requests multiple outputs so this exercises omission as well.
        assert!(model.outputs.len() > 1, "{name}");
        model.outputs.truncate(1);
        let restricted = solve(&model).unwrap();
        assert_eq!(
            restricted.quantities.keys().copied().collect::<Vec<_>>(),
            model.outputs,
            "{name}"
        );
    }
}
