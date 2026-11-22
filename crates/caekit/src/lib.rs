#![doc = include_str!("../README.md")]

mod model;
pub mod physics;
mod result;

pub use caekit_mesh::{
    AxisFrame, FaceRegion, GeometryRevision, LineMesh, MeshMetrics, MeshQuality, TetraEntities,
    TetraMesh,
};
pub use caekit_numerics::{Convergence, LinearSettings, StopReason};
pub use caekit_types::{
    Balance, Error, InterfaceValue, MaterialSource, Output, Quantity, SampleLocation,
};
pub use model::{MODEL_SCHEMA_VERSION, Model, ModelCheck, Physics, Units};
pub use result::{Execution, RESULT_SCHEMA_VERSION, RunReport};

use sha2::{Digest, Sha256};

/// Validate and solve a model on the reference double-precision CPU path.
pub fn solve(model: &Model) -> Result<RunReport, Error> {
    model.validate_request()?;
    let (meshes, mut solution) = match &model.physics {
        Physics::AxialBar(case) => (vec![case.validate()?], case.solve(model.linear_solver)?),
        Physics::ElasticSolid(case) => {
            let (mesh, solution) = case.solve_with_metrics(model.linear_solver, &model.outputs)?;
            (vec![mesh], solution)
        }
        Physics::LaminarChannel(case) => (
            vec![case.validate("physics")?],
            case.solve(model.linear_solver)?,
        ),
        Physics::CoolingChannel(case) => (
            case.validate()?,
            case.solve(model.linear_solver, &model.outputs)?,
        ),
        Physics::HeatConduction(case) => {
            let (mesh, solution) = case.solve(model.linear_solver, &model.outputs)?;
            (vec![mesh], solution)
        }
        Physics::MultimaterialHeatConduction(case) => {
            let (mesh, solution) = case.solve(model.linear_solver, &model.outputs)?;
            (vec![mesh], solution)
        }
        Physics::ThermoelasticSolid(case) => {
            let (mesh, solution) = case.solve(model.linear_solver, &model.outputs)?;
            (vec![mesh], solution)
        }
    };
    let hash = Sha256::digest(serde_json::to_vec(model)?);

    let stop_reason = solution
        .convergence
        .iter()
        .find(|stage| stage.stop_reason != StopReason::Converged)
        .map_or(StopReason::Converged, |stage| stage.stop_reason);
    if stop_reason == StopReason::Converged {
        for (key, quantity) in &solution.quantities {
            if !quantity.all_finite() {
                return Err(Error::NonFiniteResult {
                    quantity: format!("{key:?}"),
                });
            }
        }
        solution
            .quantities
            .retain(|key, _| model.outputs.contains(key));
    } else {
        solution.balances.clear();
        solution.quantities.clear();
    }
    Ok(RunReport {
        schema_version: RESULT_SCHEMA_VERSION,
        model_schema_version: model.schema_version,
        model_name: model.name.clone(),
        model_sha256: format!("{hash:x}"),
        geometry: model.physics.geometry().into_iter().cloned().collect(),
        meshes,
        execution: Execution::reference_cpu(),
        method: solution.method.to_owned(),
        stop_reason,
        convergence: solution.convergence,
        balances: solution.balances,
        quantities: solution.quantities,
    })
}
