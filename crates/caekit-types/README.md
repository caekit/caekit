# caekit-types

Shared physical inputs, result quantities, balances and errors for CAEKit.

This crate is part of [CAEKit](https://github.com/caekit/caekit), a Rust project for
open-source multiphysics engineering simulation. To solve a model, depend on
[`caekit`](https://crates.io/crates/caekit): it reads the versioned JSON model, dispatches to
the solvers and builds the run report, and it re-exports the types most callers need. Depend
on `caekit-types` directly when you build on this layer alone.

It holds the values that cross crate boundaries and appear in the JSON documents:

- `Error`, with the path of the offending model field, and the `finite`, `positive` and
  `nonempty` checks that produce it.
- `Output`, the name of every quantity a model can request, and `Quantity`, its value with
  unit and sample positions.
- `Balance`, the signed terms of a conservation balance and their signed sum.
- `Pressure`, an absolute or gauge reading with its reference, and `MaterialSource`, the
  citation and usage terms of property values.

The API follows the workspace version. Before 1.0 a minor version may change it; the
[changelog](https://github.com/caekit/caekit/blob/master/CHANGELOG.md) lists every such change.

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
