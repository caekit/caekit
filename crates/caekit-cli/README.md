# caekit-cli

The `caekit` command-line tool of [CAEKit](https://github.com/caekit/caekit), a Rust project
for open-source multiphysics engineering simulation. It validates and solves versioned JSON
models and writes the result as JSON.

## Install

- Prebuilt binaries for Linux, macOS and Windows, and a WASI module:
  [GitHub Releases](https://github.com/caekit/caekit/releases)
- `cargo binstall caekit-cli` downloads the prebuilt binary.
- `cargo install --locked caekit-cli` builds from source with Rust 1.85 or newer.

## Use

```sh
caekit check model.json    # validate; print the schema version and mesh metrics
caekit run model.json      # validate and solve; print the run report
caekit --version
```

Checks and reports are written as JSON to standard output; diagnostics go to standard error.

| Exit code | Meaning |
| --- | --- |
| 0 | The model is valid (`check`) or the run converged (`run`) |
| 1 | Invalid arguments, an invalid model, a calculation outside the finite binary64 range, or an I/O error |
| 2 | The run stopped without convergence. The report is still written; it keeps the convergence traces and withholds the physical fields and balances |

The [reference models](https://github.com/caekit/caekit/tree/master/examples) are ready to
run, and each has a [physics page](https://github.com/caekit/caekit/tree/master/docs/physics)
with its analytical reference.

## Documentation

- [Model and result format](https://github.com/caekit/caekit/blob/master/docs/model.md)
- [Changelog](https://github.com/caekit/caekit/blob/master/CHANGELOG.md)

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
