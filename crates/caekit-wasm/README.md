# caekit-wasm

WebAssembly build of [CAEKit](https://github.com/caekit/caekit), a Rust project for
open-source multiphysics engineering simulation, for Node.js and browsers. A model is a
versioned JSON document. `solve` validates it, runs the reference double-precision solver
and returns the run report, with convergence evidence and conservation balances, as a
plain object.

## Install

```sh
npm install caekit-wasm
```

## Solve a model

```js
import init, { solve } from "caekit-wasm";

await init(); // does nothing under Node.js; loads the module in a browser

const report = solve({
  schema_version: 1,
  name: "Axial bar under a 1 kN tensile force",
  units: "si",
  physics: {
    kind: "axial_bar",
    geometry: {
      identity: { id: "uniform-bar", revision: "1" },
      frame: { origin_m: [0, 0, 0], direction: [1, 0, 0] },
      length_m: 1,
      cross_section_area_m2: 0.0001,
    },
    mesh: { vertices_m: [0, 0.5, 1], start_region: "clamp", end_region: "tip" },
    material: {
      id: "reference-elastic-solid",
      young_modulus_pa: 200e9,
      source: {
        citation: "Synthetic constant modulus",
        usage_terms: "Covered by the CAEKit project license",
      },
    },
    boundary_conditions: [
      { kind: "displacement", region: "clamp", value_m: 0 },
      { kind: "force", region: "tip", value_n: 1000 },
    ],
  },
  outputs: ["end_displacement", "axial_stress", "reaction_force"],
});

console.assert(report.stop_reason === "converged");
// F L / (E A) = 1000 N * 1 m / (200e9 Pa * 1e-4 m^2) = 5e-5 m
const extension = report.quantities.end_displacement.values.tip;
// Linear elements reproduce the uniform strain exactly, so only the solve and rounding remain;
// 1e-13 m is 2e-9 of the extension, the TIP_DISPLACEMENT_TOL_M of crates/caekit/tests/axial_bar.rs.
console.assert(Math.abs(extension - 5e-5) < 1e-13);
console.log(`extension ${extension.toExponential(3)} m, solver ${report.execution.solver_version}`);
```

`solve` and `check` accept the model as JSON text, as a `Uint8Array` of UTF-8 JSON or as
the parsed object. Read `report.stop_reason` before you use a result: a run that stops
without convergence is returned, not thrown. What each outcome returns or throws is in the
[outcome table](https://github.com/caekit/caekit/blob/master/docs/model.md#run-outcomes).
TypeScript declarations of the report are included.

## Documentation

- [WebAssembly reference](https://github.com/caekit/caekit/blob/master/docs/wasm.md)
- [Model and result format](https://github.com/caekit/caekit/blob/master/docs/model.md)
- [Physics cases and their analytical references](https://github.com/caekit/caekit/tree/master/docs/physics)
- [Changelog](https://github.com/caekit/caekit/blob/master/CHANGELOG.md)

## License

MIT or Apache-2.0, at your option.
