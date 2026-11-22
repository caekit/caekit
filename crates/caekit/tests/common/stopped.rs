use caekit::physics::heat_conduction::ThermalBoundary;
use caekit::{Model, Physics};

pub fn limit_iterations(model: &mut Model) {
    if let Physics::CoolingChannel(case) = &mut model.physics {
        // One sweep cannot reach the 1e-10 relative threshold of the saved cooling channel.
        case.coupling_solver.max_iterations = 1;
        return;
    }
    // One iteration cannot reach the 1e-10 relative threshold of the saved models, so the first
    // stage ends at its limit.
    model.linear_solver.max_iterations = 1;
    if let Physics::HeatConduction(case) = &mut model.physics {
        // The saved Dirichlet case converges in one iteration. Flux and
        // convection leave the full temperature system to be solved.
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
    }
}
