# Changelog

All notable changes to CAEKit are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Every crate and package of the
workspace carries the same version. Before 1.0 a minor version may change the Rust API;
version 1 of the model document keeps its meaning across all of them.

## [Unreleased]

### Added

- Versioned model document (schema version 1) with strict decoding: unknown versions,
  fields, physics kinds and condition kinds are errors that name the path of the field.
- Run report (schema version 1) with the SHA-256 identity of the typed model, mesh
  metrics, execution record, convergence evidence with a recomputed true residual, signed
  conservation balances and the requested quantities with units and sample positions.
- Physics kinds `axial_bar`, `laminar_channel`, `cooling_channel`, `elastic_solid`,
  `heat_conduction`, `multimaterial_heat_conduction` and `thermoelastic_solid`, each
  verified against an analytical reference.
- Rust crates `caekit`, `caekit-mechanics`, `caekit-fluids`, `caekit-thermal`,
  `caekit-mesh`, `caekit-numerics` and `caekit-types`, with documentation of every public
  item.
- `caekit-cli`: the `caekit` command with `check` and `run`, and exit codes 0, 1 and 2 for
  a converged run, an error and a run that stopped without convergence.
- Python package `caekit`: `solve` and `check` for mappings, JSON text, bytes and paths,
  with `InvalidModelError` and `CalculationError`.
- npm package `caekit-wasm` for Node.js and browsers: `solve`, `check` and `version`, with
  TypeScript declarations of the result document.
- Native and WASI builds that agree to a relative 1e-10 on every reference model.
- `caekit_numerics::verify`: solution-verification functions for refinement studies. They
  give the observed order between two levels, pairwise and fitted by least squares over three
  or more levels, Richardson extrapolation and the grid convergence index of a three-grid
  study with safety factor 1.25. They classify a sequence as monotone, oscillatory or
  divergent and return no grid convergence index unless it is monotone. They also give the
  algebraic error budget `δ = 0.01 · ln r / 2` of a study, with a warning when a solve at a
  hundredth of the tolerance moves the error norm by more. `band_check` applies the band
  rule of a refinement study of four or more levels at one ratio and returns a `BandCheck`
  with the fitted order, the spread, the band, the shrink factor and the verdict.
  Logarithms and powers come from `libm` (`SPEC-VER-010`).

### Changed

- Use portable binary64 norms, signed-zero handling and tetrahedral quality
  calculations in the reference solver (`SPEC-DET-003`).
- Enforce library restrictions on platform math, unordered collections and host
  facilities with Clippy (`SPEC-DET-004`).
- Compare stage iteration counts and stop reasons exactly between native and
  WASI. The reference check also prints report digests and residual-history
  agreement (`SPEC-DET-005`).
- Enforce withholding of physical quantities and balances during report
  assembly when any solver stage fails. Test the contract for every model kind
  (`SPEC-CNV-004`).
- Verify that conjugate gradient stops on an indefinite search direction and
  reports the true residual of the last iterate (`SPEC-SOL-005`).
- Test strict schema decoding across every reference model, including unknown
  fields at every object level and version dispatch precedence (`SPEC-DOC-001`).
- Check the SI unit suffixes and explicit dimensionless fields of the typed
  reference models, including defaulted settings (`SPEC-DOC-002`).
- Freeze v1 model serialization and the report keys, labels and output order
  of converged and stopped reference runs (`SPEC-DOC-004`).
- Document and test steady report size bounds for requested entities and stage
  residual histories (`SPEC-OUT-004`).
- Report the input paths of invalid face assignments, pressure readings and
  conflicting or unknown boundary conditions (`SPEC-DOC-005`).
- Check the report and execution metadata keys of every Python reference run
  against the v1 contract (`SPEC-OUT-003`).
- Test all Python input forms for both entry points and verify that another
  Python thread can run before the native solve returns (`SPEC-BND-002`).
- Exercise the model reader and solver independently with reproducible,
  bounded input mutations in CI (`SPEC-DOC-006`).
- Provide deterministic edge and face IDs and local-to-global cell maps for
  checked tetrahedral meshes (`SPEC-DIS-006`).
- Require extension checklist review and record the checks and evidence for
  each applicable extension (`SPEC-EXT-005`).
- Link specification selection and completion rules from the contributor
  instructions and check the MUST summary against the requirement entries
  (`SPEC-AIA-008`).
- Verify `elastic_solid` with a refinement study of the Lamé thick cylinder in schema
  version 1 inputs: four graded tetrahedral levels, displacement L² order 1.976 and
  energy-norm order 0.986 inside the band rule below, a quadrature check and an affine trap
  case that the study rejects (`SPEC-VER-002`).
- Turn the refinement studies of `heat_conduction`, `multimaterial_heat_conduction`,
  `thermoelastic_solid`, `laminar_channel` and `cooling_channel` into studies that can fail.
  Each runs on non-uniform or perturbed self-similar levels and measures integral norms. It
  solves every level again at a hundredth of the solver tolerance, against the algebraic
  budget, and must reject an exactly represented trap case. The earlier uniform-mesh checks
  are kept (`SPEC-VER-003`).
- Apply one band rule to every refinement study, ruling R-9 (option A): the order fitted by
  least squares over the finest three levels must lie inside `p_f ± (0.01 + 1.5 s)`, where
  `s` is the spread of the two finest pairwise orders, and the distances of those orders from
  `p_f` must shrink by at least 2 per level unless the finest one is at most 0.01. The six
  studies call `caekit_numerics::verify::band_check`; the per-kind band variants are removed.
  The multimaterial study perturbs its vertices by 0.1 h instead of 0.2 h, because at 0.2 h
  its random mesh realizations failed the shrink condition. Fitted orders and bands: 1.975 in
  2 ± 0.077 and 0.986 in 1 ± 0.047 (heat conduction, L² and energy); 1.996 in 2 ± 0.020,
  0.992 in 1 ± 0.018 and 1.001 in 1 ± 0.019 (multimaterial, temperature and each layer's
  flux); 1.938 in 2 ± 0.076 and 1.008 in 1 ± 0.016 (thermoelastic, L² and H¹); 1.976 in
  2 ± 0.043 and 0.986 in 1 ± 0.028 (elastic solid, L² and energy); 1.997 in 2 ± 0.015 and
  1.999 in 2 ± 0.011 (laminar, pressure loss and velocity L²); 0.998 in 1 ± 0.012 (cooling
  channel, coolant temperature L²) (`SPEC-VER-003`).
- State once, on the model page, what the Rust API, the CLI, Python and WebAssembly
  return, raise or exit with for each run outcome. The Python and WebAssembly pages and
  package READMEs link to that table (`SPEC-CNV-006`).
- Derive that a balance whose test functions sum to one equals the summed free residual,
  so `|imbalance| ≤ √n_free · ‖r‖₂`. Check the bound for every applicable balance of the
  seven reference models (`SPEC-CON-006`).
- Tell automated clients to accept a result only when `stop_reason` is `converged` (CLI
  exit code 0), and that neither a closed balance nor a converged run validates it
  (`SPEC-AIA-004`).
- State the reason of every tolerance where it is set: named constants with their scale,
  derivation or measured margin in the tests, documented library defaults of
  `LinearSettings` and `CouplingSettings`, and one comment per unit floor of the native–WASI
  comparison. The library constants keep their values and bits. No tolerance was loosened
  (`SPEC-EVD-002`).
- Audit every number on the physics pages, in the physics tables of the READMEs and in
  `docs/model.md` against the command that reproduces it or its derivation. Corrected the
  first-run `elastic_solid` order, the laminar and cooling timings, the axial-bar tolerance
  attribution and the README laminar reference (2.4 Pa analytical, 2.395 Pa solved)
  (`SPEC-EVD-001`).
- Collect the cited methods in a table in `docs/provenance.md`, each with a Crossref-checked
  DOI, an ISBN or a URL, the code that uses it and the test that verifies it. The physics
  pages cite their sources through it (`SPEC-EVD-008`, `SPEC-EXT-009`).
- `scripts/check-docs.mjs` also checks that test tolerances state a reason, that every row of
  the methods table has an identifier and a test and every reported algorithm has a row, and
  that every refinement study names a command whose tests exist (`SPEC-EVD-001`,
  `SPEC-EVD-002`, `SPEC-EXT-009`).

[Unreleased]: https://github.com/caekit/caekit/commits/master
