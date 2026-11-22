# Next steps: closing the open MUST requirements

Status: **carried out and committed, 2026-11-22; not yet reviewed.** Packages 1–4
were built, with decisions D1, D6 and D7 taken as option (a). What is done, how it was verified and what
remains open are in the [MUST handoff](must-progress.md); the statuses are in the
[specification](spec.md#summary-of-must-requirements). The rest of this page is the plan as proposed, kept
for reference; its counts and expected results are not updated.

Basis: the [summary of MUST requirements](spec.md#summary-of-must-requirements) lists 82 MUST
entries: 49 Implemented, 21 Partial and 12 Planned. Of the 33 open ones, 22 bind today and are
covered below. The other 11 are conditional and bind only once their construct exists, so they
arrive with the feature that needs them ([choosing the next requirement](spec.md#choosing-the-next-requirement)).

Ruling R-3 ([rulings](blueprint.md#171-rulings)) was accepted by the owner on 2026-11-21.
Rounding-level movement from S0.1 needs only updated reference values and a CHANGELOG entry
under Unreleased. Every `model_sha256` stays unchanged.

## Packages

| # | Package | Closes | Partial or deferred | Blueprint step | Depends on | Parallel with |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Determinism: portable `fmath` (scaled 2-norm, threshold), no platform math in decision and result paths, `clippy.toml` bans, equal iterations and stop reasons in the native and WASI comparison | [DET-003](spec.md#spec-det-003), [DET-004](spec.md#spec-det-004), [DET-005](spec.md#spec-det-005) | [DET-012](spec.md#spec-det-012) (WebAssembly dependency graph check) left for a follow-up | [S0.1](blueprint.md#151-steps) | D1, D2 | 2, 3, 4 |
| 2 | Missing tests: rejection at every object level, unit suffixes, frozen model bytes and report key sets, withheld results on failure, report size bound, numbering, Python report keys and interpreter lock, deterministic fuzzing | [DOC-001](spec.md#spec-doc-001), [DOC-002](spec.md#spec-doc-002), [DOC-004](spec.md#spec-doc-004), [DOC-005](spec.md#spec-doc-005), [CNV-004](spec.md#spec-cnv-004), [OUT-003](spec.md#spec-out-003), [OUT-004](spec.md#spec-out-004), [BND-002](spec.md#spec-bnd-002) | [DOC-006](spec.md#spec-doc-006) (`expect` sites in package 1 files), [SOL-005](spec.md#spec-sol-005) (zero-diagonal exit), [DIS-006](spec.md#spec-dis-006) (edge numbering) | — | package 1 for the deferred items; D3, D4, D5 | 1, 3, 4 |
| 3 | Verification: `caekit_numerics::verify` (least-squares order, GCI, algebraic budget, Taylor orders); manufactured-solution refinement study for `elastic_solid`; fit, band, tighter rerun and trap check for the five existing studies | [VER-002](spec.md#spec-ver-002), [VER-003](spec.md#spec-ver-003); also [VER-010](spec.md#spec-ver-010) for Rust | [VER-004](spec.md#spec-ver-004) stays Planned (D7) | [S0.5](blueprint.md#151-steps) | D1, D7; package 1 for `pub mod verify;` | 1, 2, 4 |
| 4 | Documentation and evidence: new named checks in `scripts/check-docs.mjs` (numbers, tolerances, requirements, extension rows, methods), outcome table, what a balance proves, guidance for automated clients, a methods table in provenance, spec links in `AGENTS.md` | [EVD-001](spec.md#spec-evd-001), [EVD-002](spec.md#spec-evd-002), [EVD-008](spec.md#spec-evd-008), [AIA-004](spec.md#spec-aia-004), [AIA-008](spec.md#spec-aia-008), [EXT-005](spec.md#spec-ext-005); with them the SHOULDs [CNV-006](spec.md#spec-cnv-006), [CON-006](spec.md#spec-con-006), [EXT-009](spec.md#spec-ext-009) | The generated catalogue values of EVD-001 wait for [VER-007](spec.md#spec-ver-007) (S0.4) | — | D6; tolerance-reason comments in other packages' files | 1, 2, 3 |

## Order

| Step | What | Waits for |
| --- | --- | --- |
| 1 | Decide D1–D9 | — |
| 2 | Packages 1–4 run in parallel, each in its own worktree. Package 2 runs as two workers with disjoint files. | step 1 |
| 3 | Merge package 1 | package 1 reviewed |
| 4 | Merge package 3, then add `pub mod verify;` to `crates/caekit-numerics/src/lib.rs` | step 3 |
| 5 | Merge package 2, then make its deferred changes in package 1 files: the SOL-005 zero-diagonal exit in `pcg`, the `expect` messages for DOC-006, and the path assertions in `crates/caekit/tests/elastic_solid.rs` | step 4 |
| 6 | Merge package 4. The tolerance-reason check passes only after the hand-off comments from steps 3–5 are in. | step 5 |
| 7 | Update `CHANGELOG.md`, the spec status lines, the summary table and counts, and the R-3 row of the blueprint. Run the [verify commands](../../AGENTS.md#5-verify) and the WebAssembly comparison. | step 6 |
| Follow-up | DET-012 dependency-graph check (D9); DIS-006 edge numbering with the first P2 space ([S2.1](blueprint.md#151-steps), D5) | step 7 |

## Shared files

| File | Packages | Rule |
| --- | --- | --- |
| `crates/caekit/tests/{axial_bar,cooling_channel,heat_conduction,laminar_channel,multimaterial_heat_conduction,thermoelastic_solid}.rs` | 1 (lint header), 2 (path assertions), 3 (order fit), 4 (tolerance reasons) | Each package changes its own lines; merge in order 1 → 3 → 2 → 4 |
| `crates/caekit/tests/elastic_solid.rs` | 1, 2, 3, 4 | Package 3 owns it. The changes from packages 2 and 4 are applied after package 3 merges. |
| `crates/caekit-numerics/src/lib.rs` | 1, 2 (deferred), 3 (`pub mod`) | Package 1 owns it; the others follow at merge |
| `crates/caekit-numerics/tests/sparse_system.rs` | 1, 2, 4 | Package 1 first, then 2, then 4 |
| `docs/model.md` | 1 (numerical lesson), 2 (report size, fuzz budgets), 4 (outcomes, balances, automated clients) | Separate sections; merge in order |
| `README.md`, `AGENTS.md`, `docs/provenance.md` | 1, 2, 4 (README); 1, 4 (AGENTS.md, provenance) | Package 4 owns the structure; packages 1 and 2 add their lines |
| `CHANGELOG.md`, the status lines and summary of `spec.md`, the rulings of `blueprint.md` | all | Written once at step 7 from each package's prepared text |

## Decisions

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | Accept `libm` (ruling R-7)? Package 1 needs a cube root for the reported tetrahedral mean ratio. Package 3 needs `ln` and `pow`. | (a) add `libm` with `default-features = false`; (b) a hand-written Newton cube root, with the `verify` math moved into tests | (a). It is pure Rust, MIT or Apache-2.0, and builds for `wasm32`. Record it in `docs/provenance.md` and check it against `deny.toml`. It replaces package 1's hand-written cube root and lets `verify` stay a public library module. Add a cross-target bit test per function ([rules and enforcement](blueprint.md#122-rules-and-enforcement)). |
| D2 | `caekit-mesh` may not depend on `caekit-numerics` | (a) a private `norm2` copy in mesh, bit-tested against `fmath` through a dev-dependency; (b) add the dependency edge and update the architecture | (a) |
| D3 | Fuzzing tool for DOC-006 | (a) a deterministic mutation fuzzer on stable Rust with fixed seeds and budgets, plus a non-required CI job; (b) `cargo-fuzz` (nightly, C++ libFuzzer) | (a): no new dependency, and it stays pure Rust |
| D4 | CG exit on a non-positive Jacobi diagonal | (a) report the threshold and the true residual ‖b‖, and amend the SOL-005 acceptance text; (b) keep the current exit | (a) |
| D5 | DIS-006 edge numbering | (a) defer it to the first P2 space; (b) add an edge API now | (a): no edge entity exists yet |
| D6 | CON-006 balance bound test, which AIA-004 depends on | (a) add it now, for force and energy balances; (b) derivation only, so AIA-004 stays Partial | (a) |
| D7 | Loading route for the `elastic_solid` study | (a) schema v1 only: one traction region per loaded boundary triangle, no solver change; (b) a closure API in `elastic_solid.rs`, which closes VER-004 | (a) |
| D8 | Bitwise-equal residual histories between native and WASI | (a) report them only, until R-2 is decided; (b) assert them now | (a) |
| D9 | DET-012 WebAssembly dependency-graph check | (a) a follow-up after the merge; (b) part of package 1 | (a) |

## Risks

- A rounding change in a norm may change an iteration count on a borderline stage. Report it and investigate; never loosen a tolerance.
- The coarsest level (n = 3) of the `elastic_solid` study may be pre-asymptotic. Fix the fallback level set (n = 4, 8, 16, 32) before the first run, not after it.
- The Python interpreter-lock test depends on timing. It needs a solve of at least about 100 ms, and slow runners may make it flaky.
- The new tolerance-reason check fails until every package's hand-off comments are merged, so it is run last.

## Expected result

| Outcome | Requirements |
| --- | --- |
| Implemented (19 MUST) | DET-003, DET-004, DET-005, DOC-001, DOC-002, DOC-004, DOC-005, CNV-004, OUT-003, OUT-004, BND-002, VER-002, VER-003, EVD-001, EVD-002, EVD-008, AIA-004, AIA-008, EXT-005 |
| Still Partial (3 MUST) | DOC-006, SOL-005, DIS-006. SOL-005 closes at step 5 if D4 = (a) and the acceptance text is amended. |
| SHOULDs closed on the way | CNV-006, CON-006, EXT-009, VER-010 (Rust only) |

The MUST counts (Implemented / Partial / Planned) would move from 49 / 21 / 12 to 68 / 3 / 11, or
to 69 / 2 / 11 if SOL-005 closes at step 5. DET-004 is the only unconditional Planned entry; the 11
Planned entries that remain are all conditional.
