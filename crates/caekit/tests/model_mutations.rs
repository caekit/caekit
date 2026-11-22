//! Repeatable, bounded mutation checks for the model reader and the solver.

use caekit::{Model, Physics, solve};
use serde_json::{Value, json};

const CORPUS: &[&[u8]] = &[
    include_bytes!("../../../examples/axial_bar.json"),
    include_bytes!("../../../examples/cooling_channel.json"),
    include_bytes!("../../../examples/elastic_solid.json"),
    include_bytes!("../../../examples/heat_conduction.json"),
    include_bytes!("../../../examples/laminar_channel.json"),
    include_bytes!("../../../examples/multimaterial_heat_conduction.json"),
    include_bytes!("../../../examples/thermoelastic_solid.json"),
];
/// Mutated byte documents per fixture; with the 7 fixtures, 3,584 reader cases
/// (docs/design/must-progress.md). A count that bounds the run time, not a numerical value.
const READ_CASES_PER_MODEL: usize = 512;
/// Mutated typed models per fixture; with the 7 fixtures, 1,792 solver cases. As above.
const SOLVE_CASES_PER_MODEL: usize = 256;
/// Iteration cap of every stage of a mutated solve. A cost cap, not a numerical requirement: a
/// stage that has not converged by then returns an iteration-limit report, which this test
/// accepts like any other result that is not a panic.
const MUTATED_ITERATION_CAP: usize = 4;

struct Sequence(u64);

impl Sequence {
    fn next(&mut self) -> u64 {
        // Xorshift with a fixed nonzero seed makes CI failures replayable.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn index(&mut self, length: usize) -> usize {
        (self.next() % length as u64) as usize
    }
}

fn limit_iterations(model: &mut Model) {
    model.linear_solver.max_iterations = model
        .linear_solver
        .max_iterations
        .min(MUTATED_ITERATION_CAP);
    match &mut model.physics {
        Physics::CoolingChannel(case) => {
            case.coupling_solver.max_iterations = case
                .coupling_solver
                .max_iterations
                .min(MUTATED_ITERATION_CAP);
        }
        Physics::ThermoelasticSolid(case) => {
            case.thermal_solver.max_iterations = case
                .thermal_solver
                .max_iterations
                .min(MUTATED_ITERATION_CAP);
        }
        _ => {}
    }
}

#[test]
#[ignore = "bounded mutation suite run by the model-mutation job"]
fn document_reader_does_not_panic_on_mutated_bytes() {
    let mut sequence = Sequence(0xadd2_7d82_019e_c821);
    for (fixture, corpus) in CORPUS.iter().enumerate() {
        for case in 0..READ_CASES_PER_MODEL {
            let offset = sequence.index(corpus.len());
            let mut bytes = corpus.to_vec();
            match case % 5 {
                0 => bytes.truncate(offset),
                1 => {
                    bytes.remove(offset);
                }
                2 => bytes[offset] = sequence.next() as u8,
                3 => bytes.insert(offset, sequence.next() as u8),
                _ => {
                    bytes = (0..64).map(|_| sequence.next() as u8).collect();
                }
            }
            std::panic::catch_unwind(|| Model::from_json(&bytes))
                .unwrap_or_else(|_| {
                    panic!("model reader panicked on corpus {fixture}, case {case}")
                })
                .ok();
        }
    }
}

fn mutable_leaves<'a>(value: &'a mut Value, key: &str, leaves: &mut Vec<&'a mut Value>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                mutable_leaves(child, key, leaves);
            }
        }
        Value::Array(array) => {
            for child in array {
                mutable_leaves(child, key, leaves);
            }
        }
        Value::Number(_) => leaves.push(value),
        Value::String(_)
            if matches!(key, "region" | "id" | "revision" | "name" | "material_id") =>
        {
            leaves.push(value);
        }
        _ => {}
    }
}

#[test]
#[ignore = "bounded mutation suite run by the model-mutation job"]
fn solver_does_not_panic_on_mutated_typed_models() {
    const FLOATS: &[f64] = &[-f64::MAX, -1.0, -0.0, 0.0, f64::MIN_POSITIVE, 1.0, f64::MAX];
    const INTEGERS: &[u32] = &[0, 1, 2, 4, 20, u32::MAX];
    const STRINGS: &[&str] = &["", " ", "unknown-region", "x0"];
    let mut sequence = Sequence(0x9412_9e67_afbb_218d);
    for (fixture, corpus) in CORPUS.iter().enumerate() {
        let original = serde_json::to_value(Model::from_json(corpus).unwrap()).unwrap();
        for case in 0..SOLVE_CASES_PER_MODEL {
            let mut input = original.clone();
            let mut leaves = Vec::new();
            mutable_leaves(&mut input, "", &mut leaves);
            let index = sequence.index(leaves.len());
            let leaf = &mut leaves[index];
            **leaf = match leaf {
                Value::Number(number) if number.is_f64() => {
                    json!(FLOATS[sequence.index(FLOATS.len())])
                }
                Value::Number(_) => json!(INTEGERS[sequence.index(INTEGERS.len())]),
                _ => json!(STRINGS[sequence.index(STRINGS.len())]),
            };
            // Bypass from_json validation to exercise programmatically built models.
            let mut model: Model = serde_json::from_value(input).unwrap_or_else(|error| {
                panic!("untyped mutation for corpus {fixture}, case {case}: {error}")
            });
            limit_iterations(&mut model);
            std::panic::catch_unwind(|| solve(&model))
                .unwrap_or_else(|_| panic!("solve panicked on corpus {fixture}, case {case}"))
                .ok();
        }
    }
}
