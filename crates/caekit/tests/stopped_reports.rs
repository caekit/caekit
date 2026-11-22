//! A failed stage must preserve its trace and withhold fields and balances.

#[path = "common/stopped.rs"]
mod stopped;

use caekit::{Model, StopReason, solve};

#[test]
fn an_iteration_limit_on_the_first_stage_withholds_the_run_results() {
    let fixtures: &[(&[u8], &str)] = &[
        (
            include_bytes!("../../../examples/laminar_channel.json"),
            "unit_gradient_axial_momentum",
        ),
        (
            include_bytes!("../../../examples/elastic_solid.json"),
            "solid_elastic_equilibrium",
        ),
        (
            include_bytes!("../../../examples/heat_conduction.json"),
            "solid_heat_conduction",
        ),
        (
            include_bytes!("../../../examples/multimaterial_heat_conduction.json"),
            "solid_heat_conduction",
        ),
    ];
    for &(input, stage_name) in fixtures {
        let mut model = Model::from_json(input).unwrap();
        stopped::limit_iterations(&mut model);
        let report = solve(&model).unwrap();
        assert_eq!(
            report.stop_reason,
            StopReason::IterationLimit,
            "{}",
            model.name
        );
        assert!(report.quantities.is_empty());
        assert!(report.balances.is_empty());
        let [trace] = report.convergence.as_slice() else {
            panic!("expected exactly the first stage for {}", model.name);
        };
        assert_eq!(trace.stage, stage_name);
        assert_eq!(trace.stop_reason, report.stop_reason);
        // The limit that `limit_iterations` sets.
        assert_eq!(trace.iterations, 1);
        assert!(trace.final_true_residual_l2.unwrap() > trace.residual_threshold.unwrap());
    }
}
