# MUST implementation handoff

State on 2026-11-22. The changes are committed together and not yet reviewed. The plan
that produced it is [next steps](next-steps.md); the statuses are those of the
[specification](spec.md#summary-of-must-requirements), and each change has an entry under
[Unreleased](../../CHANGELOG.md#unreleased).

## Completed

Of the 22 MUST requirements that were open and binding at the start, 21 are Implemented and
one is Partial. The summary now counts 82 MUST entries: 70 unconditional (69 Implemented,
1 Partial) and 12 conditional (1 Implemented, 11 Planned).

| Requirements | Changes |
| --- | --- |
| DET-003, DET-004, DET-005 | Portable norms and mesh quality, Clippy restrictions, and exact native–WASI comparison of stage decisions |
| CNV-004, SOL-005 | Withhold fields and balances after a failed stage. Verify CG breakdown on an indefinite search direction. |
| DOC-001, DOC-002 | Check strict schema decoding and SI naming across the saved models and their defaulted settings. |
| DOC-004, DOC-005, DOC-006 | Freeze v1 model bytes and report structure, improve validation paths, and add deterministic reader and solver mutation tests. |
| OUT-003, OUT-004, BND-002 | Check report metadata in the installed Python wheel and TypeScript declarations, test report size bounds, and verify Python input forms and interpreter-lock release. |
| DIS-006 | Number tetrahedral edges and faces on demand, independent of boundary region ordering. |
| EXT-005, AIA-008 | Require extension checklist review, link contributor rules to the specification, and check that MUST statuses match the summary. |
| VER-002, VER-003 (with the SHOULD VER-010) | `caekit_numerics::verify`; the Lamé thick-cylinder study of `elastic_solid`; the five other studies rebuilt with a check solve at a hundredth of the tolerance and a trap; all six apply the one band rule of ruling R-9 through `band_check` (2026-11-22) |
| AIA-004 (with the SHOULDs CNV-006, CON-006) | Outcome table, the bound `\|imbalance\| ≤ √n_free · ‖r‖₂` with `crates/caekit/tests/balance_bounds.rs`, and the paragraph for automated clients in [model.md](../model.md#run-outcomes) |
| EVD-001 | Audit of every number on the physics pages, the README physics tables and `docs/model.md` |
| EVD-002 | A stated reason for every tolerance in code, tests, scripts and executed examples |
| SHOULD EXT-009 | Methods table in [provenance.md](../provenance.md#methods-and-cited-sources), checked against the reported algorithms |

`scripts/check-docs.mjs` now also checks the reasons of test tolerances, the methods table and
the commands of the refinement studies. Each new rule was checked against an injected defect
in a scratch copy of the tree, and all 10 defects were reported.

## Verification

Run on Windows 11 x86-64 with rustc 1.100.0 and Node.js 24.18.0 on 2026-11-22, after the band
rule of ruling R-9 was applied to every study, unless marked:

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test --workspace --exclude caekit-py --exclude caekit-wasm --locked` | pass: 118 passed, 0 failed, 3 ignored (multimaterial `measure_sourced_layers`; model_mutations 2 by design) |
| `node scripts/check-docs.mjs` | pass: 36 pages, 7 physics kinds and 11 workspace members |
| WASI tests (`cargo +stable test … --target wasm32-wasip1 --locked --tests`, README › WebAssembly) | pass: 102 passed, 0 failed, 3 ignored |
| `node scripts/verify-reference-cases.mjs target/debug/caekit target/wasm32-wasip1/debug/caekit.wasm` | pass 7, fail 0; equal report digests and exact residual histories |
| Mutation suite (`model_mutations -- --ignored`) | pass: 2 passed |
| `RUSTDOCFLAGS="-D warnings" cargo +stable doc --workspace --no-deps --locked` | pass |
| `cargo +stable build --lib -p caekit --target wasm32-unknown-unknown --release --locked` | pass |
| Rust 1.85.0: workspace tests and `cargo check -p caekit-py -p caekit-wasm` | pass: 118 passed, 0 failed |
| npm package (`build.mjs --check-size`, `node --test`, `package_smoke.mjs --pack`) | pass: 26 passed; one 568,123-byte module |
| Python wheel (`maturin build`, installed, `pytest crates/caekit-py/tests`) with Python 3.14.5 | pass: 20 passed |
| `node scripts/release.mjs check` | pass |
| `cargo deny check`, `release.mjs publish-crates --dry-run` | not run; `cargo-deny` is not installed on this machine |

The last eight rows were run on 2026-11-22 before the commit, with the same toolchain.
`maturin develop` failed in this checkout because `pip --version` printed a path that is not
UTF-8 (`Masaüstü`); building the wheel with `PYTHONUTF8=1` and installing it worked.

The first WASI run failed: the multimaterial study solved its levels on scoped threads, and
`wasm32-wasip1` cannot spawn one ("failed to spawn thread: Unsupported"). The study now solves
its levels one after another on WASI. In Git Bash on Windows the runner path of README ›
WebAssembly needs `$(pwd -W)` instead of `$PWD`.

## Open

- **EVD-008 (Partial, the only open binding MUST).** The book-length and paywalled sources are
  confirmed by their bibliographic record, not against the primary text. The entry lists them.
  It closes when the texts are read, or when a maintainer accepts record-level confirmation.
- **Studies that pass the band rule only through its exemption.** Ruling R-9 was accepted on
  2026-11-22 (option A, [blueprint § 11.2](blueprint.md#112-refinement-studies)) and all six
  studies apply it through `caekit_numerics::verify::band_check`. Where `ρ < 2` the finest
  distance `|d₃| ≤ 0.01` carries the shrink condition: `thermoelastic_solid` H¹ (`ρ` 1.6309) and
  all three `multimaterial_heat_conduction` norms (temperature `|d₃|` 0.007282, the closest to
  the limit). The `cooling_channel` `ρ` of 2.0107 is 0.5 % above 2.
- **Multimaterial perturbation.** The study moved from 0.2 h to 0.1 h after the 0.2 h run failed
  the shrink condition; the 0.2, 0.1 and 0.05 h runs are on
  [its page](../physics/multimaterial-heat-conduction.md). A maintainer should confirm the choice.
- **Traps with a wide band.** A trap's noise spread widens its own band:
  `laminar_channel` momentum imbalance is rejected only by the shrink condition, and the
  `multimaterial_heat_conduction` temperature and right-flux traps only by the algebraic check.
  The test name `the_band_rejects_the_study_without_the_source_term` is kept because SPEC-VER-003
  lists it.
- **Ruling R-7.** `libm` is used under decision D1 (a), but only `cbrt` has a cross-target bit
  test. `log` and `pow` are covered only by the WASI run of the `verify` tests.
- **Weak tolerance reasons**, stated where they are set: the `pcg` stagnation constants 0.75
  and 8, the margins of the unit floors of the native–WASI comparison, and margins measured
  natively on Windows only. `THIN_BAR_STRESS_TOL_PA` is never exercised.
- **Not tested on the CLI or the bindings:** numerical breakdown, non-finite results and
  insufficient memory. An allocation failure outside the two checked buffers aborts the
  process.
- **Not measured again:** the multimaterial solve-time and peak-RSS columns, which came from a
  Linux release build. Debug study tests take 27–49 s natively and up to 100 s under WASI.
- Python and WebAssembly bindings of `verify` do not exist (VER-010 is Rust only).
- The 11 conditional MUST requirements stay Planned until their constructs exist. DET-012
  (D9) is a follow-up.
