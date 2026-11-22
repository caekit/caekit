# CAEKit

Open-source multiphysics engineering simulation in Rust. A model is a versioned JSON
document. CAEKit validates it, solves it with checked reference solvers and writes a
reproducible report with convergence evidence, conservation balances and the requested
quantities with their units. The same solver runs natively, from Python and in WebAssembly.

<!-- unreleased-note -->
> **Status: pre-release.** Version 0.1.0 is not published yet. Until it is, the packages
> below are not on their registries; build from a checkout as shown under
> [Reproduce CI locally](#reproduce-ci-locally).

## Install

| You use | Install | Reference |
| --- | --- | --- |
| Rust | `cargo add caekit` | [docs.rs/caekit](https://docs.rs/caekit) |
| Command line | `cargo binstall caekit-cli`, `cargo install --locked caekit-cli`, or a binary from [Releases](https://github.com/caekit/caekit/releases) | [caekit-cli](crates/caekit-cli/README.md) |
| Python 3.9 or newer | `pip install caekit` | [docs/python.md](docs/python.md) |
| Node.js and browsers | `npm install caekit-wasm` | [docs/wasm.md](docs/wasm.md) |

Building from source needs Rust 1.85 or newer.

## Run a reference case

From a checkout, the command-line tool validates and solves the saved models.

```sh
cargo run --locked -- check examples/axial_bar.json
cargo run --locked -- run examples/axial_bar.json
```

`check` writes the validation result and mesh metrics as JSON. `run` writes the run report.
Exit code 0 is a converged run, 2 a run that stopped without convergence and 1 an input,
calculation-range or I/O error. [Model and result format](docs/model.md)

The library, the Python package and the WebAssembly package solve the same document.

```rust,no_run
use caekit::{Model, Output, StopReason, solve};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = Model::from_json(&std::fs::read("examples/axial_bar.json")?)?;
    let report = solve(&model)?;
    assert_eq!(report.stop_reason, StopReason::Converged);
    println!("{:?}", report.quantities[&Output::EndDisplacement]);
    Ok(())
}
```

```python
import pathlib

import caekit

report = caekit.solve(pathlib.Path("examples/axial_bar.json"))
assert report["stop_reason"] == "converged"
print(report["quantities"]["end_displacement"]["values"]["tip"], "m")
```

```js
import { readFileSync } from "node:fs";
import init, { solve } from "caekit-wasm";

await init();
const report = solve(readFileSync("examples/axial_bar.json", "utf8"));
console.assert(report.stop_reason === "converged");
console.log(report.quantities.end_displacement.values.tip, "m");
```

## Physics

| `physics.kind` | Solves | Reference result of the saved model |
| --- | --- | --- |
| [`axial_bar`](docs/physics/axial-bar.md) | Static small-strain axial deformation of a uniform bar | 0.05 mm extension under 1 kN |
| [`laminar_channel`](docs/physics/laminar-channel.md) | Steady, fully developed laminar flow between parallel plates | 2.4 Pa analytical pressure loss (2.395 Pa solved) at a Reynolds number of 200 |
| [`cooling_channel`](docs/physics/cooling-channel.md) | Laminar coolant heated through a conductive wall on independent meshes | Outlet temperature and heat duty of the continuous thermal circuit |
| [`elastic_solid`](docs/physics/elastic-solid.md) | Three-dimensional isotropic linear elasticity on tetrahedra | 0.1 mm extension, −2 MN reaction and 100 J strain energy |
| [`heat_conduction`](docs/physics/heat-conduction.md) | Steady isotropic heat conduction in a tetrahedral solid | 350 K at the centre, 20,000 W/m² along +x, ±200 W at the ends |
| [`multimaterial_heat_conduction`](docs/physics/multimaterial-heat-conduction.md) | Steady conduction through volume regions in ideal contact | 310 K at the interface, 10 W from the hot to the cold layer |
| [`thermoelastic_solid`](docs/physics/thermoelastic-solid.md) | Heat conduction followed by thermal-expansion elasticity | −120 MPa axial stress and opposing 12 kN reactions |

Each page states the assumptions, the equations, the discretization and the analytical
reference that the tests compare against. Each value of the last column is derived on the
linked page from the inputs of the saved model, and running that model measures it:
`cargo run --locked -- run examples/<kind>.json`.

## Scope and limits

- The solvers are steady or static, use linear constitutive laws and run on the reference
  path: IEEE 754 binary64 on one CPU thread.
- The reference cases are verified against analytical solutions. That shows the equations
  are solved correctly; it does not validate a model of your geometry.
- A converged run measures iterative error only. Engineering error also depends on the
  physical assumptions, the geometry, the mesh and the material properties. Verify a
  result independently before you rely on it.
- Version 1 of the model document is stable in meaning. The Rust API follows the workspace
  version, and before 1.0 a minor version may change it. [Changelog](CHANGELOG.md)

## Documentation

| Page | Content |
| --- | --- |
| [docs/model.md](docs/model.md) | Model and result documents, outputs, numerical stages, exit codes |
| `docs/physics/` | One page per physics kind, linked in the table above |
| [docs/python.md](docs/python.md) | The Python package |
| [docs/wasm.md](docs/wasm.md) | The WebAssembly package for Node.js and browsers |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crates, layering and where a change belongs |
| [docs/provenance.md](docs/provenance.md) | Sources of dependencies, inputs and methods |
| [docs/RELEASING.md](docs/RELEASING.md) | Publishing channels and the release procedure |
| [AGENTS.md](AGENTS.md), [CONTRIBUTING.md](CONTRIBUTING.md) | Rules for human and automated contributors |
| [SECURITY.md](SECURITY.md) | How to report a vulnerability |

## Cargo workspace

The `caekit` package exposes the versioned model, physics dispatch, `solve` and run
reports. Its domain solvers depend on shared lower-level crates, and every surface uses
the same public Rust API. [Architecture](docs/ARCHITECTURE.md)

| Package | Responsibility |
| --- | --- |
| `caekit` | Versioned model, physics dispatch, coupled cases, `solve` and run reports |
| `caekit-cli` | The `caekit` executable, argument handling and JSON I/O |
| `caekit-py` | The Python package `caekit` |
| `caekit-wasm` | The npm package `caekit-wasm` |
| `caekit-mechanics` | Axial bar, three-dimensional elasticity and thermal expansion |
| `caekit-fluids` | Laminar momentum, mass flow and pressure recovery |
| `caekit-thermal` | Tetrahedral solid conduction, wall conduction and coolant energy transport |
| `caekit-mesh` | Meshes, geometry revisions, regions and mesh quality |
| `caekit-numerics` | Sparse systems, iterative solves, convergence histories and solution verification |
| `caekit-types` | Shared physical input and result values and diagnostics |

The workspace selects the API and command-line packages by default at the repository
root. Build another package by name. If your project uses a local Cargo path dependency on
this checkout, point it to `crates/caekit`.

```sh
cargo test --locked -p caekit-numerics
cargo install --locked --path crates/caekit-cli
```

## Reproduce CI locally

**Verify CAEKit** runs on pull requests and pushes to `master`. The **Required CI** status
check succeeds only when every job below succeeds. Run the commands from the repository
root. The native jobs need `rustup`; the WebAssembly and documentation jobs also need
Node.js 24; the Python job needs Python 3.9 or newer.

```sh
rustup toolchain install stable --profile minimal --component rustfmt,clippy
rustup toolchain install 1.85.0 --profile minimal
rustup target add --toolchain stable wasm32-unknown-unknown wasm32-wasip1
```

| Job | Command |
| --- | --- |
| Native tests on Linux, macOS and Windows | `cargo +stable test --workspace --exclude caekit-py --exclude caekit-wasm --locked` |
| Bounded model mutations | `cargo +stable test --locked -p caekit --test model_mutations -- --ignored` |
| Rust MSRV | `cargo +1.85.0 test --workspace --exclude caekit-py --exclude caekit-wasm --locked` and `cargo +1.85.0 check -p caekit-py -p caekit-wasm --locked` |
| Rustfmt | `cargo +stable fmt --all -- --check` |
| Clippy | `cargo +stable clippy --workspace --all-targets --locked -- -D warnings` |
| Documentation | `RUSTDOCFLAGS="-D warnings" cargo +stable doc --workspace --no-deps --locked` and `node scripts/check-docs.mjs` |
| Packaging | `node scripts/release.mjs check` and `node scripts/release.mjs publish-crates --dry-run` |
| Licences and advisories | `cargo deny check` with [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) |
| WebAssembly library | `cargo +stable build --lib -p caekit --target wasm32-unknown-unknown --release --locked` |
| WASI tests and reference cases | See [WebAssembly](#webassembly) |
| Python package | See [docs/python.md](docs/python.md#build-and-test-from-a-checkout) |
| npm package | See [docs/wasm.md](docs/wasm.md#build-and-test-from-a-checkout) |

The binding crates `caekit-py` and `caekit-wasm` need their own toolchains, so the
workspace test commands exclude them and their jobs build and test the real packages.
Workspace tests include the doctests, which compile the Rust examples of this page and of
`docs/`.

### WebAssembly

CI compiles the public library for `wasm32-unknown-unknown` and runs the physics and
numerical tests for `wasm32-wasip1` under Node.js 24 with WASI Preview 1. The WASI job
launches the compiled command-line tool on every saved model and compares it with the
native build.

```sh
export CARGO_TARGET_WASM32_WASIP1_RUNNER="node $PWD/scripts/run-wasi.mjs"
cargo +stable test --workspace --exclude caekit-cli --exclude caekit-py --exclude caekit-wasm --target wasm32-wasip1 --locked --tests
cargo +stable build --locked -p caekit-cli
cargo +stable build --locked -p caekit-cli --target wasm32-wasip1
node scripts/verify-reference-cases.mjs target/debug/caekit target/wasm32-wasip1/debug/caekit.wasm
```

The reference check reports CLI exit errors and verifies convergence, the analytical
benchmarks, model identity, requested outputs, units and sample positions. It requires
equal stage iteration counts and stop reasons and prints report digests with platform
fields normalized, along with exact residual-history agreement. Numeric
comparisons use a unit-specific absolute tolerance near zero and a relative tolerance of
`1e-10` at larger magnitudes. Failed comparisons identify the model, quantity, expected
value and measured value.

## Contributing

Read [AGENTS.md](AGENTS.md) and [CONTRIBUTING.md](CONTRIBUTING.md) first. A change updates
its documentation in the same pull request, and `node scripts/check-docs.mjs` checks that
the pages still match the code.

## License

Choose either the [MIT license](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE).
