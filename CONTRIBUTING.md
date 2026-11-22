# Contributing to CAEKit

Thank you for helping. [AGENTS.md](AGENTS.md) holds the rules of the project and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) explains where a change belongs. Both bind
human and automated contributors alike. This page covers the practical parts.

## Set up

- Rust 1.85 or newer through `rustup`, with `rustfmt` and `clippy`.
- Node.js 24 for the documentation check, the WASI comparison and the npm package.
- Python 3.9 or newer with `maturin` and `pytest` when you work on the Python package.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --exclude caekit-py --exclude caekit-wasm --locked
node scripts/check-docs.mjs
```

[README.md](README.md#reproduce-ci-locally) lists every CI job with the command that
reproduces it. Use the narrowest command while you iterate, for example
`cargo test --locked -p caekit --test heat_conduction`.
The separate model mutation job checks that invalid documents and programmatic models
return without panicking. [Reproduce the mutation suite](docs/model.md#input-mutation-checks).

## What a change contains

- **A reference.** New or changed physics is compared with an analytical solution that
  is derived independently of the solver, and with a refinement study where the
  discretization has an error. Never weaken a tolerance to make a test pass.
- **Its documentation.** Section 3 of [AGENTS.md](AGENTS.md) lists the pages to update for
  each kind of change. `node scripts/check-docs.mjs` fails when a physics kind, an output
  or a crate is undocumented, when a link is broken or when an example is not compiled.
  Examples are executed: Rust blocks as doctests, `python` and `js` blocks by the package
  tests.
- **Evidence.** Every number in the documentation was measured or derived, and the page
  says how.
- **A changelog entry** under `Unreleased` in [CHANGELOG.md](CHANGELOG.md) for anything a
  user can observe.

Earlier model documents keep their meaning, their report fields and their
`model_sha256`. A change that cannot keep them needs a new schema version.

## Sources and licences

CAEKit is licensed under MIT or Apache-2.0. Do not copy or port code from a source whose
licence is incompatible with that, such as GPL, LGPL or AGPL solvers. Papers, textbooks
and permissively licensed code are fine; cite them in
[docs/provenance.md](docs/provenance.md). A new dependency is pure Rust and has a licence
that `deny.toml` allows.

Unless you state otherwise, a contribution that you submit for inclusion is licensed
under MIT or Apache-2.0, without additional terms.

## Pull requests

- Keep a pull request focused on one outcome and fill in its checklist.
- Report what works, what does not, the evidence and the open risks.
- Releases are made by maintainers. [docs/RELEASING.md](docs/RELEASING.md)

Report a security problem privately, as [SECURITY.md](SECURITY.md) describes.
