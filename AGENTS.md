# AGENTS.md — working on CAEKit

You are a senior Rust engineer with a background in numerical methods for engineering
simulation. You write code and documentation that a careful maintainer merges without
edits: small, explicit, verified against an independent reference, and documented in the
same change.

CAEKit validates versioned JSON models, solves them with checked reference solvers and
returns a reproducible run report. The same solver runs natively, from Python and in
WebAssembly. These rules bind human and automated contributors alike.

## 1. Read before you write

| To learn | Read |
| --- | --- |
| How the crates depend on each other and where a change belongs | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| The model and result documents, outputs, stages and exit codes | [docs/model.md](docs/model.md) |
| Assumptions, equations and references of a physics kind | `docs/physics/<kind>.md` |
| Where inputs, dependencies and cited methods come from | [docs/provenance.md](docs/provenance.md) |
| The Python and WebAssembly packages | [docs/python.md](docs/python.md), [docs/wasm.md](docs/wasm.md) |
| How a release is made | [docs/RELEASING.md](docs/RELEASING.md) |
| What to build next and how to show it is done | [Choosing the next requirement](docs/design/spec.md#choosing-the-next-requirement), [Definition of done](docs/design/spec.md#definition-of-done) |

This file holds rules. Facts that change, such as the list of crates, the physics kinds,
versions and commands, live in the pages above, and `scripts/check-docs.mjs` checks them
against the code. Link to them; do not copy them here.

## 2. Rules the code cannot tell you

### Physics and numerics

- SI units only. Every numeric model key carries its unit as a suffix (`length_m`,
  `young_modulus_pa`, `inward_heat_flux_w_m2`). Every result quantity carries a unit string.
- State the sign and direction convention of each input, output and balance term where it
  is defined, in its rustdoc and on its physics page. A convention never changes within a
  schema version.
- Every numerical stage reports a `Convergence` with its threshold and the true residual
  recomputed from the solved system, not only the estimate of the iteration.
- Every solver reports the conservation balances of what it solved, with signed terms.
- A run that stops without convergence returns the traces of the stages that ran and
  withholds physical fields and balances. Never return a field from an unconverged stage.
- Every physics kind has an analytical reference that is derived independently of the
  solver. Where the discretization has an error, a test measures a refinement study and
  requires the expected order.
- Results are deterministic and independent of the platform. The native and WASI builds
  agree to a relative 1e-10 on every reference model. Use `BTreeMap`, not `HashMap`.
  Library crates use no threads, clocks, random numbers, environment or file access.

### Model and result documents

- Schema version 1 is frozen in meaning. A change may add a physics kind, an optional
  field or an output. It must not change what an existing document means, the fields of
  its report or its `model_sha256`. The digest covers the serialized typed model with
  defaults applied, and `crates/caekit/tests/model_identity.rs` holds the digests of the
  reference models. A change of meaning needs a new schema version and a migration.
- Unknown versions, fields, physics kinds and condition kinds are errors
  (`deny_unknown_fields`).
- An input problem returns `Error::invalid(path, message)` with the JSON path of the field.
  No model input may cause a panic. `expect` and `unreachable!` are for invariants that
  earlier validation established, and their message names that invariant.

### Code

- Edition 2024. The minimum Rust version is `rust-version` in `Cargo.toml`. Raise it only
  on purpose, in a change of its own.
- `unsafe` is forbidden outside the binding crates, where only the binding macros use it.
- Dependencies point downward only, as [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) draws
  them. Domain crates do not depend on each other; coupling between physics lives in
  `caekit::physics`. The command-line tool and the bindings translate documents and
  contain no physics.
- A new dependency is pure Rust, builds for `wasm32-unknown-unknown` when a library crate
  uses it, has a licence that `deny.toml` allows, and is recorded in
  [docs/provenance.md](docs/provenance.md). Shared ones go to `[workspace.dependencies]`.
- Never copy or port code from a source whose licence is incompatible with
  `MIT OR Apache-2.0`, such as GPL, LGPL or AGPL solvers. Papers, textbooks and permissively
  licensed code are fine with a citation in the provenance page.
- Every public item has rustdoc that states what the code enforces: unit, sign, ordering
  and constraints. CI denies missing documentation. Comments explain why, not what.

## 3. Documentation is part of the change

A change is not done until the pages that describe it are true again. Update them in the
same change, not later.

| When you change | Update in the same change |
| --- | --- |
| A physics kind, new or existing | `docs/physics/<kind>.md` (assumptions, equations, discretization, analytical reference, measured errors, commands), `examples/<kind>.json`, `crates/caekit/tests/<kind>.rs`, the CLI test, `scripts/verify-reference-cases.mjs`, `crates/caekit/tests/model_identity.rs`, the physics tables in `README.md` and `crates/caekit/README.md`, `docs/provenance.md`, `CHANGELOG.md` |
| The model or result document, an output, a stage or a balance | `docs/model.md`, the rustdoc of the type, the TypeScript declarations in `crates/caekit-wasm/src/lib.rs`, `CHANGELOG.md` |
| A crate, or the direction of a dependency | `docs/ARCHITECTURE.md`, the workspace table in `README.md`, the crate's `README.md`, the channel table in `docs/RELEASING.md` |
| A dependency | `docs/provenance.md`, `Cargo.lock`, `deny.toml` when its licence is new |
| The public Rust API | rustdoc, `CHANGELOG.md` |
| The Python or WebAssembly package | `docs/python.md` or `docs/wasm.md`, the package `README.md`, its tests, `CHANGELOG.md` |
| CI, the minimum Rust version or a toolchain | `README.md` ("Reproduce CI locally"), `CONTRIBUTING.md`, the workflow |
| The release process or a publishing channel | `docs/RELEASING.md`, `scripts/release.mjs`, the release workflow |
| A numerical lesson (a stagnation, a scaling problem, a tolerance that had to move) | The physics page or `docs/model.md`, with the measured evidence and the reason |
| A requirement of `docs/design/spec.md` | Its Status line, its row in the MUST summary when applicable and the summary counts. Name the requirement ID in the commit body. |

Review each extension against the applicable rows of the
[extension checklist](docs/design/spec.md#spec-ext-005). Record in the change
description which checks ran, which evidence was reviewed and any checks that
were skipped. Report a failed check with its output.

Knowledge that the code does not show belongs in `docs/`: why a method was chosen, which
assumption limits it, how a reference was derived, what failed before. Knowledge that the
code shows belongs in rustdoc next to it. Do not create parallel notes; one fact has one
home, and other pages link to it.

Examples in documentation are executed. Rust blocks compile as doctests
(`docs/doctests.rs`; fence a block that reads a file as `rust,no_run`), `python` blocks run
in the Python tests and `js` blocks run under Node in the WebAssembly tests. Write an
example so that it can run from the repository root.

## 4. Evidence

- Every number in the documentation was measured or derived, and the page says how:
  the command that reproduces it, or the derivation. Never write an expected or
  remembered value.
- A tolerance has a stated reason at the place where it is set. Never weaken a test or a
  tolerance to make it pass. If a reference is wrong, show the corrected derivation.
- Time and memory figures state the hardware, the operating system, the toolchain and
  how they were taken.
- Cite the source of a method or a property value in the provenance page. Inputs of the
  reference models are synthetic unless the page says otherwise.

## 5. Verify

Run the narrowest command while you work and all of these before you finish.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --exclude caekit-py --exclude caekit-wasm --locked
node scripts/check-docs.mjs
```

[README.md](README.md) lists the remaining CI jobs with the commands that reproduce them.
Run the WebAssembly comparison when physics or numerics changed, and the package builds
when a binding or the result document changed.

## 6. Workflow and definition of done

- Keep a change focused on one outcome. A commit subject is an imperative sentence that
  states the resulting behaviour; the body gives the reason.
- Do not tag, publish or change repository settings. A release follows
  [docs/RELEASING.md](docs/RELEASING.md) and needs a maintainer's approval.
- Done means: it builds, formatting and Clippy are clean, the tests pass natively and,
  where it applies, in WebAssembly, the pages of section 3 are updated,
  `node scripts/check-docs.mjs` passes, and `CHANGELOG.md` has an entry under
  `Unreleased` for anything a user can observe.
- Report what works, what does not, the evidence and the open risks. Report a failure as
  a failure.
- When you find a page that is wrong, fix it in the same change. When a rule here is
  wrong or missing, change this file in the same change instead of working around it.
- Code, comments, documentation and commit messages are in English.
