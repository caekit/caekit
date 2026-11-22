# Architecture

CAEKit turns a versioned model document into a run report. This page describes how the
workspace is divided, which way dependencies point and where a change belongs.
[AGENTS.md](../AGENTS.md) holds the rules that apply to every change.

## From model to report

1. A surface hands the model to `caekit::Model::from_json` as UTF-8 JSON. The surfaces are
   the Rust API, the `caekit` command-line tool, the Python package and the WebAssembly
   package.
2. `from_json` reads `schema_version` before anything else, decodes the typed model and
   validates geometry, meshes, conditions and the requested outputs. Unknown versions,
   fields and kinds are errors that name the path of the field. Validation returns a
   `ModelCheck` with the metrics of every mesh.
3. `caekit::solve` dispatches on the `Physics` variant to a domain solver. The solver
   returns a `Solution`: its method name, one `Convergence` per numerical stage, the
   conservation balances and the quantities.
4. `solve` takes the run's stop reason from the first stage that did not converge and
   withholds all quantities and balances for that outcome. For converged runs it rejects
   quantities that are not finite and keeps the requested outputs. It hashes the typed
   model and builds the `RunReport` with the `Execution` record of the reference CPU path.
5. The surface writes the report as JSON. [Model and result documents](model.md)

Because every surface exchanges the same two JSON documents, a new physics kind, field
or output reaches the command-line tool and both bindings without a change to them.

## Crates

```text
 caekit-cli     caekit-py     caekit-wasm       surfaces: translate documents, no physics
       \            |            /
                 caekit                         model, dispatch, coupled cases, run report
       /            |            \
caekit-fluids  caekit-thermal  caekit-mechanics domain solvers
       \            |            /
     caekit-mesh         caekit-numerics        meshes and regions · sparse systems
          |
     caekit-types                               shared values, balances and errors
```

| Crate | Depends on | Responsibility | Published to |
| --- | --- | --- | --- |
| `caekit-types` | | `Error` and the input checks, `Output`, `Quantity`, `Balance`, `Pressure`, `MaterialSource`, `Solution` | crates.io |
| `caekit-numerics` | | `SparseAssembly`, `ConstrainedSystem`, `LinearSettings`, `Convergence`, `StopReason`; Jacobi-preconditioned conjugate gradient in binary64; `verify` (observed order, grid convergence index, algebraic budget) | crates.io |
| `caekit-mesh` | `caekit-types` | `LineMesh`, `AxisFrame`, `TetraMesh`, `FaceRegion`, `GeometryRevision`, mesh metrics and quality | crates.io |
| `caekit-fluids` | mesh, numerics, types | Laminar channel momentum, mass flow and pressure recovery | crates.io |
| `caekit-thermal` | mesh, numerics, types | Solid heat conduction for one or several materials; wall conduction and coolant energy transport across nonmatching meshes | crates.io |
| `caekit-mechanics` | mesh, numerics, types | Axial bar, three-dimensional elasticity, thermal expansion from nodal temperatures | crates.io |
| `caekit` | all of the above | `Model`, `Physics`, `solve`, `RunReport`, and the coupled cases in `caekit::physics` | crates.io |
| `caekit-cli` | `caekit` | The `caekit` executable: arguments, file I/O, exit codes | crates.io, GitHub Releases |
| `caekit-py` | `caekit` | The Python package `caekit` (PyO3, abi3) | PyPI |
| `caekit-wasm` | `caekit` | The npm package `caekit-wasm` (wasm-bindgen) and its TypeScript declarations | npm |

The workspace also holds `docs/` as the unpublished package `caekit-docs`. Its only
content is `doctests.rs`, which compiles the Rust examples of the Markdown pages.

## Rules of the layering

- **Dependencies point downward only.** A domain crate never depends on another domain
  crate. A case that couples two kinds of physics lives in `caekit::physics`:
  `cooling_channel` passes the mass flow of the fluid solve to the thermal solve, and
  `thermoelastic_solid` passes nodal temperatures of the thermal solve to mechanics.
- **`caekit-numerics` knows no physics.** It receives a matrix, a right-hand side and
  stopping settings, and it returns the solution with its convergence record. The caller
  names the stage and the unit of the residual.
- **Library crates are pure computation.** They do no I/O and use no threads, clock,
  environment or random numbers, so they build for `wasm32-unknown-unknown` and give the
  same result on every platform. Ordered collections (`BTreeMap`) keep assembly and
  serialization deterministic. File and process handling belongs to the surfaces.
- **Surfaces translate.** The command-line tool and the bindings convert between their
  host and the two JSON documents and map errors. They contain no physics and no numerics.
- **Validation comes before assembly.** A solver may rely on what `validate` established,
  and says so where it does. No model input may cause a panic.
- **Errors name their origin.** `Error::InvalidModel` carries the JSON path of the field,
  and `Error::NonFiniteResult` names the quantity that left the finite binary64 range.

## Where a change belongs

**A new physics kind.**

1. Put the solver in the domain crate that owns the physics. If it couples two domains,
   put the coupling in `crates/caekit/src/physics/`.
2. Add new quantities to `Output` in `caekit-types`. Give the case an `OUTPUTS` constant.
3. Add the variant to `Physics` in `crates/caekit/src/model.rs` (`geometry`, `supports`,
   `validate`) and the dispatch in `caekit::solve`.
4. Add `examples/<kind>.json`, `crates/caekit/tests/<kind>.rs` with the analytical
   reference, a refinement study and rejected inputs, the case in
   `crates/caekit-cli/tests/reference_cases.rs`, the reference in
   `scripts/verify-reference-cases.mjs` and the digest in
   `crates/caekit/tests/model_identity.rs`.
5. Write `docs/physics/<kind>.md` and update the pages that
   [AGENTS.md](../AGENTS.md) lists for a physics change.

**A new field or output of an existing case** must leave earlier documents unchanged in
meaning and digest. Give it a default that reproduces the earlier behaviour, document it
in [model.md](model.md), and extend the TypeScript declarations in
`crates/caekit-wasm/src/lib.rs` when the shape of the result document changes.

**A new numerical method** goes to `caekit-numerics` when it is independent of physics.
It reports a `Convergence` with a true residual, like the existing solve.

**A new surface or crate** depends on `caekit` and stays out of the solvers. Add it to the
table above, to the workspace table in the README and to the channel table in
[RELEASING.md](RELEASING.md).

## Verification layers

| Layer | Where | What it proves |
| --- | --- | --- |
| Physics tests | `crates/caekit/tests/<kind>.rs` | Analytical references, refinement orders, balances, rejected inputs |
| Numerics tests | `crates/caekit-numerics/tests` | Verified residuals; stop reasons that never claim convergence wrongly |
| Model identity | `crates/caekit/tests/model_identity.rs` | The reference models keep their version 1 digest |
| Command-line tests | `crates/caekit-cli/tests` | Output and exit codes of the real executable |
| Native against WASI | `scripts/verify-reference-cases.mjs` | Both builds meet the references and agree to a relative 1e-10 |
| Python package | `crates/caekit-py/tests` | The installed wheel: references, input forms, errors, documentation examples |
| WebAssembly package | `crates/caekit-wasm/tests` | The built npm package: references, declarations, entry points, documentation examples |
| Documentation | `scripts/check-docs.mjs`, `docs/doctests.rs` | Pages match the code; Rust examples compile |
| Packaging | `node scripts/release.mjs check`, `publish-crates --dry-run` | Every crate builds from its packaged sources with its licence files |

[README.md](../README.md#reproduce-ci-locally) gives the command for each CI job.
