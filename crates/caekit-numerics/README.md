# caekit-numerics

Sparse linear systems, iterative solves and convergence reporting for CAEKit.

This crate is part of [CAEKit](https://github.com/caekit/caekit), a Rust project for
open-source multiphysics engineering simulation. To solve a model, depend on
[`caekit`](https://crates.io/crates/caekit): it reads the versioned JSON model, dispatches to
the solvers and builds the run report, and it re-exports the types most callers need. Depend
on `caekit-numerics` directly when you build on this layer alone.

It has no dependency on any physics crate:

- `SparseAssembly` accumulates a sparse matrix with deterministic row and column order and
  solves it with Jacobi-preconditioned conjugate gradient in binary64.
- `ConstrainedSystem` eliminates prescribed unknowns symmetrically before the solve and
  returns the full vector.
- `LinearSettings` holds the stopping tolerances and the iteration limit.
- `Convergence` records the stage, the stop reason, the residual history and the final
  residual recomputed as `||b − Ax||₂`. A stage that stops without convergence keeps this
  evidence.
- The `verify` module holds the solution-verification functions of refinement studies: the
  observed order between two levels and fitted by least squares, the order band of a
  refinement study (`band_check`: the fit over the finest three levels in
  `p_f ± (0.01 + 1.5 s)`, with its shrink-rate condition), Richardson extrapolation,
  the grid convergence index of a monotone sequence, the classification of a sequence as
  monotone, oscillatory or divergent, and the algebraic error budget of a study.

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
