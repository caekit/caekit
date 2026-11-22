# caekit

Multiphysics engineering simulation in Rust. A model is a versioned JSON document.
`caekit` validates it, solves it with checked reference solvers on the double-precision CPU
path and returns a run report that carries convergence evidence, conservation balances and
the requested quantities with their units. The same solver is available as the
[`caekit` command-line tool](https://crates.io/crates/caekit-cli), from
[Python](https://pypi.org/project/caekit/) and as
[WebAssembly](https://www.npmjs.com/package/caekit-wasm).

## Use

```sh
cargo add caekit
```

```rust
use caekit::{Model, Output, Quantity, StopReason, solve};

fn main() -> Result<(), caekit::Error> {
    // A 1 m bar of 100 mm² and 200 GPa, clamped at one end, under 1 kN at the other.
    let model = Model::from_json(
        br#"{
          "schema_version": 1,
          "name": "Axial bar under a 1 kN tensile force",
          "units": "si",
          "physics": {
            "kind": "axial_bar",
            "geometry": {
              "identity": { "id": "uniform-bar", "revision": "1" },
              "frame": { "origin_m": [0, 0, 0], "direction": [1, 0, 0] },
              "length_m": 1,
              "cross_section_area_m2": 0.0001
            },
            "mesh": { "vertices_m": [0, 0.5, 1], "start_region": "clamp", "end_region": "tip" },
            "material": {
              "id": "reference-elastic-solid",
              "young_modulus_pa": 200e9,
              "source": {
                "citation": "Synthetic constant modulus",
                "usage_terms": "Covered by the CAEKit project license"
              }
            },
            "boundary_conditions": [
              { "kind": "displacement", "region": "clamp", "value_m": 0 },
              { "kind": "force", "region": "tip", "value_n": 1000 }
            ]
          },
          "outputs": ["end_displacement", "axial_stress", "reaction_force"]
        }"#,
    )?;

    let report = solve(&model)?;
    assert_eq!(report.stop_reason, StopReason::Converged);

    // F L / (E A) = 1000 N * 1 m / (200e9 Pa * 1e-4 m²) = 5e-5 m
    let Quantity::ByRegion { unit, values } = &report.quantities[&Output::EndDisplacement] else {
        unreachable!("end_displacement is reported by region");
    };
    assert_eq!(unit, "m");
    // Linear elements reproduce the uniform strain exactly, so only the solve and rounding remain;
    // 1e-13 m is 2e-9 of the extension, the TIP_DISPLACEMENT_TOL_M of crates/caekit/tests/axial_bar.rs.
    assert!((values["tip"] - 5.0e-5).abs() < 1.0e-13);
    Ok(())
}
```

`Model::from_json` rejects unknown schema versions, fields, physics kinds and condition
kinds, and names the path of the offending field. `solve` returns a report even when a
numerical stage stops without convergence: read `stop_reason`. Such a report keeps the
convergence traces and withholds the physical fields and balances.

## Physics

| `physics.kind` | Solves |
| --- | --- |
| [`axial_bar`](https://github.com/caekit/caekit/blob/master/docs/physics/axial-bar.md) | Static small-strain axial deformation of a uniform bar |
| [`laminar_channel`](https://github.com/caekit/caekit/blob/master/docs/physics/laminar-channel.md) | Steady, fully developed laminar flow between parallel plates |
| [`cooling_channel`](https://github.com/caekit/caekit/blob/master/docs/physics/cooling-channel.md) | Laminar coolant heated through a conductive wall |
| [`elastic_solid`](https://github.com/caekit/caekit/blob/master/docs/physics/elastic-solid.md) | Three-dimensional isotropic linear elasticity on tetrahedra |
| [`heat_conduction`](https://github.com/caekit/caekit/blob/master/docs/physics/heat-conduction.md) | Steady isotropic heat conduction in a tetrahedral solid |
| [`multimaterial_heat_conduction`](https://github.com/caekit/caekit/blob/master/docs/physics/multimaterial-heat-conduction.md) | Steady conduction through volume regions in ideal contact |
| [`thermoelastic_solid`](https://github.com/caekit/caekit/blob/master/docs/physics/thermoelastic-solid.md) | Heat conduction followed by thermal-expansion elasticity |

Each page states the assumptions, the equations, the discretization and the analytical
reference that the tests compare against. A converged run measures iterative error only.
Engineering error also depends on the physical assumptions, the geometry, the mesh and the
material properties, so verify a result independently before you rely on it.

## Documentation

- [Model and result format](https://github.com/caekit/caekit/blob/master/docs/model.md)
- [Architecture](https://github.com/caekit/caekit/blob/master/docs/ARCHITECTURE.md)
- [Source and data provenance](https://github.com/caekit/caekit/blob/master/docs/provenance.md)
- [Changelog](https://github.com/caekit/caekit/blob/master/CHANGELOG.md)

The library has no I/O, threads or clock of its own and builds for
`wasm32-unknown-unknown` and `wasm32-wasip1`. The minimum supported Rust version is 1.85.

## The CAEKit crates

| Crate | Responsibility |
| --- | --- |
| [`caekit`](https://crates.io/crates/caekit) | Versioned model, physics dispatch, `solve` and run reports |
| [`caekit-cli`](https://crates.io/crates/caekit-cli) | The `caekit` command-line tool |
| [`caekit-mechanics`](https://crates.io/crates/caekit-mechanics) | Axial bar, three-dimensional elasticity and thermal expansion |
| [`caekit-fluids`](https://crates.io/crates/caekit-fluids) | Laminar channel momentum, mass flow and pressure recovery |
| [`caekit-thermal`](https://crates.io/crates/caekit-thermal) | Solid heat conduction, wall conduction and coolant energy transport |
| [`caekit-mesh`](https://crates.io/crates/caekit-mesh) | Line and tetrahedral meshes, geometry revisions, regions and mesh quality |
| [`caekit-numerics`](https://crates.io/crates/caekit-numerics) | Sparse systems, iterative solves, convergence histories and solution verification |
| [`caekit-types`](https://crates.io/crates/caekit-types) | Shared physical inputs, result quantities, balances and errors |

[Architecture](https://github.com/caekit/caekit/blob/master/docs/ARCHITECTURE.md) describes how they depend on each other.

## License

MIT or Apache-2.0, at your option.
