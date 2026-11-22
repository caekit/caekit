//! Axial bar: the analytical tension case, rigid translation, nonzero displacement
//! conditions, model identity versus execution outcome, and rejected inputs.

mod common;

use caekit::physics::axial_bar::BarBoundary;
use caekit::{Error, Model, Output, Physics, Quantity, StopReason, solve};
use common::{close, invalid_at, region};

const INPUT: &[u8] = include_bytes!("../../../examples/axial_bar.json");

// The bar is solved with linear elements, which reproduce its uniform strain exactly, so every
// allowance below covers only the linear solve and binary64 rounding. The solver stops at the
// default relative residual 1e-10 of docs/model.md; most allowances are that factor times the
// scale of the quantity. "Measured" is the largest |actual - expected| over the sites that use
// the constant (print it in `common::close` and run `cargo test -p caekit --test axial_bar --
// --nocapture`).

/// End displacement of the 1 kN case in m: 2e-9 of the 5e-5 m extension. Measured 6.8e-21 m.
const TIP_DISPLACEMENT_TOL_M: f64 = 1.0e-13;
/// Reaction force and force imbalance of the 1 kN case in N: 1e-10 of 1000 N. Measured 4.5e-13 N.
const REACTION_TOL_N: f64 = 1.0e-7;
/// Axial stress of the 1 kN case in Pa: 1e-10 of 1e7 Pa. Measured 5.6e-9 Pa.
const STRESS_TOL_PA: f64 = 1.0e-3;
/// Axial stress under a rigid translation in Pa: 1e-10 of the 0.01 Pa that the 1e-6 N tip force
/// gives. Measured 6.9e-18 Pa.
const RIGID_STRESS_TOL_PA: f64 = 1.0e-12;
/// Tip displacement under a 1 mm rigid translation in m: 3e-19 is 1.4 ulp of 1e-3 (ulp is
/// 2^-62 = 2.2e-19), the rounding of the sum of the translation and the 5e-14 m extension in
/// the expected value and in the solve. Measured 0.
const RIGID_TIP_DISPLACEMENT_TOL_M: f64 = 3.0e-19;
/// Force imbalance under a rigid translation in N: 1e-10 of the 1e-6 N tip force. Measured
/// 4.2e-22 N.
const RIGID_IMBALANCE_TOL_N: f64 = 1.0e-16;
/// Reactions of the two-displacement case in N: 1e-9 of 1000 N, the ratio being the only
/// derivation. Measured 5.7e-13 N, which is 5 ulp of 1000 N (2^-43 = 1.1e-13): the reaction
/// subtracts the prescribed 1e-5 m from 6e-5 m.
const PRESCRIBED_REACTION_TOL_N: f64 = 1.0e-6;

#[test]
fn solves_analytic_tension_case() {
    let report = solve(&Model::from_json(INPUT).unwrap()).unwrap();

    assert_eq!(report.stop_reason, StopReason::Converged);
    close(
        region(&report, Output::EndDisplacement, "tip"),
        5.0e-5,
        TIP_DISPLACEMENT_TOL_M,
    );
    close(
        region(&report, Output::ReactionForce, "clamp"),
        -1000.0,
        REACTION_TOL_N,
    );
    close(report.balances[0].imbalance, 0.0, REACTION_TOL_N);

    let Quantity::Field { values, .. } = &report.quantities[&Output::AxialStress] else {
        panic!("expected a stress field");
    };
    for &stress in values {
        close(stress, 1.0e7, STRESS_TOL_PA);
    }
    let stage = &report.convergence[0];
    assert!(stage.final_true_residual_l2.unwrap() <= stage.residual_threshold.unwrap());
}

#[test]
fn rigid_translation_preserves_axial_stress_and_force_balance() {
    let mut model = Model::from_json(INPUT).unwrap();
    let Physics::AxialBar(case) = &mut model.physics else {
        unreachable!()
    };
    case.boundary_conditions = vec![
        BarBoundary::Displacement {
            region: "clamp".into(),
            value_m: 0.001,
        },
        BarBoundary::Force {
            region: "tip".into(),
            value_n: 1e-6,
        },
    ];
    let report = solve(&model).unwrap();
    assert_eq!(report.stop_reason, StopReason::Converged);
    let Quantity::Field { values, .. } = &report.quantities[&Output::AxialStress] else {
        panic!("expected a stress field");
    };
    for &stress in values {
        close(stress, 0.01, RIGID_STRESS_TOL_PA);
    }
    close(
        region(&report, Output::EndDisplacement, "tip"),
        0.001 + 5e-14,
        RIGID_TIP_DISPLACEMENT_TOL_M,
    );
    close(report.balances[0].imbalance, 0.0, RIGID_IMBALANCE_TOL_N);
}

#[test]
fn loaded_start_has_negative_projection_and_geometry_is_preserved() {
    let mut model = Model::from_json(INPUT).unwrap();
    let Physics::AxialBar(case) = &mut model.physics else {
        panic!("expected the axial bar fixture");
    };
    case.geometry.frame.origin_m = [3.0, 0.0, -2.0];
    case.geometry.frame.direction = [0.0, 1.0, 0.0];
    case.boundary_conditions = vec![
        BarBoundary::Force {
            region: "clamp".to_owned(),
            value_n: -1000.0,
        },
        BarBoundary::Displacement {
            region: "tip".to_owned(),
            value_m: 0.0,
        },
    ];

    let report = solve(&model).unwrap();
    close(
        region(&report, Output::EndDisplacement, "clamp"),
        -5.0e-5,
        TIP_DISPLACEMENT_TOL_M,
    );
    close(
        region(&report, Output::ReactionForce, "tip"),
        1000.0,
        REACTION_TOL_N,
    );
    let Quantity::Field { positions_m, .. } = &report.quantities[&Output::AxialDisplacement] else {
        panic!("expected a displacement field");
    };
    assert_eq!(positions_m[0], [3.0, 0.0, -2.0]);
    assert_eq!(positions_m[5], [3.0, 1.0, -2.0]);
}

#[test]
fn supports_nonzero_displacement_conditions_at_both_ends() {
    let mut model = Model::from_json(INPUT).unwrap();
    let Physics::AxialBar(case) = &mut model.physics else {
        panic!("expected the axial bar fixture");
    };
    case.boundary_conditions = vec![
        BarBoundary::Displacement {
            region: "clamp".to_owned(),
            value_m: 1.0e-5,
        },
        BarBoundary::Displacement {
            region: "tip".to_owned(),
            value_m: 6.0e-5,
        },
    ];
    let report = solve(&model).unwrap();
    close(
        region(&report, Output::ReactionForce, "tip"),
        1000.0,
        PRESCRIBED_REACTION_TOL_N,
    );
    close(
        region(&report, Output::ReactionForce, "clamp"),
        -1000.0,
        PRESCRIBED_REACTION_TOL_N,
    );
}

#[test]
fn separates_model_identity_and_execution_outcome() {
    let mut model = Model::from_json(INPUT).unwrap();
    let before = solve(&model).unwrap();
    let normalized = serde_json::to_vec_pretty(&model).unwrap();
    let after = solve(&Model::from_json(&normalized).unwrap()).unwrap();
    assert_eq!(before.model_sha256, after.model_sha256);

    // One CG iteration cannot reach the relative residual 1e-10, so the stage ends at its
    // iteration limit; the history then holds the initial residual and that one iteration.
    model.linear_solver.max_iterations = 1;
    let stopped = solve(&model).unwrap();
    assert_ne!(before.model_sha256, stopped.model_sha256);
    assert_eq!(stopped.stop_reason, StopReason::IterationLimit);
    assert!(stopped.quantities.is_empty());
    assert!(stopped.balances.is_empty());
    assert_eq!(stopped.convergence.len(), 1);
    assert_eq!(stopped.convergence[0].stop_reason, stopped.stop_reason);
    assert_eq!(stopped.convergence[0].residual_l2_history.len(), 2);
}

#[test]
fn rejects_future_schemas_unknown_fields_and_invalid_conditions() {
    let mut json: serde_json::Value = serde_json::from_slice(INPUT).unwrap();
    json["schema_version"] = 2.into();
    assert!(matches!(
        Model::from_json(&serde_json::to_vec(&json).unwrap()),
        Err(Error::UnsupportedModelVersion(2))
    ));

    json["schema_version"] = 1.into();
    json["physics"]["boundary_conditions"][0]["forcce"] = 1.into();
    let error = Model::from_json(&serde_json::to_vec(&json).unwrap()).unwrap_err();
    assert!(
        matches!(error, Error::Json(ref cause) if cause.to_string().contains("unknown field `forcce`"))
    );

    let mut model = Model::from_json(INPUT).unwrap();
    let Physics::AxialBar(case) = &mut model.physics else {
        panic!("expected the axial bar fixture");
    };
    case.boundary_conditions.pop();
    invalid_at(
        solve(&model).unwrap_err(),
        "physics.boundary_conditions",
        "each endpoint requires",
    );

    let mut model = Model::from_json(INPUT).unwrap();
    let Physics::AxialBar(case) = &mut model.physics else {
        panic!("expected the axial bar fixture");
    };
    case.mesh.vertices_m[1] = 0.0;
    invalid_at(
        solve(&model).unwrap_err(),
        "physics.mesh.vertices_m[1]",
        "must",
    );
}
