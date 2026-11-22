# WebAssembly package

The `caekit-wasm` package on npm runs the CAEKit solver in Node.js and in browsers. It is a
thin layer over the Rust library: the model goes in as a JSON document and the report
comes back as a plain object. [Model and result format](model.md)

## Install

```sh
npm install caekit-wasm
```

The package holds one WebAssembly module and two sets of glue code. Under Node.js, both
`import` and `require` resolve to the Node build, which is ready when it is loaded.
Everywhere else the package resolves to the web build, an ES module that needs one
`await init()` before the first call.

## Solve a model

```js
import { readFileSync } from "node:fs";
import init, { check, solve, version } from "caekit-wasm";

await init(); // does nothing under Node.js; loads the module in a browser

const model = JSON.parse(readFileSync("examples/heat_conduction.json", "utf8"));
console.log("solver", version(), "cells", check(model).meshes[0].cell_count);

const report = solve(model);
console.assert(report.stop_reason === "converged");

const temperature = report.quantities.solid_temperature;
console.log(temperature.unit, Math.min(...temperature.values), Math.max(...temperature.values));

for (const [region, power] of Object.entries(report.quantities.boundary_heat_rate.values)) {
  console.log(region, power.toFixed(1), "W"); // positive power enters the solid
}
```

| Function | Returns |
| --- | --- |
| `check(model)` | The validation result: `model_schema_version` and the metrics of every mesh |
| `solve(model)` | The run report: identity, meshes, execution, convergence, balances and quantities |
| `version()` | The version of the solver compiled into the module |

`model` is JSON text, a `Uint8Array` of UTF-8 JSON, or the parsed object. Every form gives
the same report and the same `model_sha256`. The report records `"wasm_cpu"` as its
execution backend.

## Browsers and bundlers

`init()` fetches `caekit_wasm_bg.wasm` from the location of the glue module. A bundler
that handles `new URL(..., import.meta.url)` needs no configuration. To supply the module
yourself, pass it to `init`:

```html
<script type="module">
  import init, { solve } from "./node_modules/caekit-wasm/web/caekit_wasm.js";

  await init({ module_or_path: new URL("./node_modules/caekit-wasm/web/caekit_wasm_bg.wasm", import.meta.url) });
  const model = await (await fetch("./model.json")).json();
  const report = solve(model);
  document.body.textContent = `${report.model_name}: ${report.stop_reason}`;
</script>
```

A solve blocks the thread that calls it. Call it from a Web Worker when the page has to
stay responsive. The subpaths `caekit-wasm/web` and `caekit-wasm/node` select a build
explicitly, and `caekit-wasm/caekit_wasm_bg.wasm` is the module itself.

## Errors

`check` and `solve` throw an `Error` whose `name` is `"CaekitError"` and whose `kind` says
what failed.

```js
import { readFileSync } from "node:fs";
import init, { check } from "caekit-wasm";

await init();
const model = JSON.parse(readFileSync("examples/axial_bar.json", "utf8"));
model.physics.geometry.length_m = -1;

try {
  check(model);
  throw new Error("a negative length must be rejected");
} catch (error) {
  if (error.name !== "CaekitError") throw error;
  console.log(error.kind, error.message); // invalid_model physics.geometry.length_m: ...
}
```

The `kind` of each outcome, `invalid_model`, `calculation` or `bad_argument`, is stated once
for every surface in the [outcome table](model.md#run-outcomes).

## Runs that do not converge

Read `stop_reason` before you use a result: a run that stops without convergence is
returned, not thrown. The [outcome table](model.md#run-outcomes) says what such a report
holds, and [Automated clients](model.md#automated-clients) says when to accept a result.

```js
import { readFileSync } from "node:fs";
import init, { solve } from "caekit-wasm";

await init();
const model = JSON.parse(readFileSync("examples/elastic_solid.json", "utf8"));
model.linear_solver = { relative_residual_tolerance: 1e-10, absolute_residual_tolerance: 0, max_iterations: 1 };

const report = solve(model);
console.assert(report.stop_reason === "iteration_limit");
console.assert(Object.keys(report.quantities).length === 0);
const [stage] = report.convergence;
console.log(stage.stage, stage.final_true_residual_l2, ">", stage.residual_threshold, stage.residual_unit);
```

## TypeScript

The package ships declarations for `ModelInput`, `ModelCheck`, `RunReport`, `Quantity`,
`Convergence`, `Balance`, `StopReason` and `CaekitError`. `Quantity` is a union that
narrows on `kind`: `scalar`, `field`, `vector_field`, `tensor_field`, `by_region` and
`by_interface`. The declarations are written by hand in `crates/caekit-wasm/src/lib.rs` and
a test compares them with real reports.

## Build and test from a checkout

The package lives in `crates/caekit-wasm`. The build needs the `wasm32-unknown-unknown`
target, Node.js 24 and the wasm-bindgen CLI in the version of the `wasm-bindgen` crate in
`Cargo.lock`. The build script looks for the CLI in `crates/caekit-wasm/.tools/` and then
on `PATH`, and it stops with the download address when the version differs.

```sh
rustup target add --toolchain stable wasm32-unknown-unknown
node crates/caekit-wasm/build.mjs --check-size
node --test "crates/caekit-wasm/tests/*_test.mjs"
node crates/caekit-wasm/tests/package_smoke.mjs --pack crates/caekit-wasm/npm
```

`build.mjs` writes the package to `crates/caekit-wasm/npm/`. `--check-size` fails when the
module exceeds the budget set in the script; the module measured 571,250 bytes at 0.1.0.
The tests solve every saved model with the Node build, compare the analytical references,
check the declarations and execute the `js` examples of this page, of the repository
README and of the package README. The smoke test packs the package, installs the tarball
in an empty project and exercises every entry point.
