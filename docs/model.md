# Model and result documents

A model is UTF-8 JSON. Dispatch uses the integer `schema_version` before the
remaining fields are decoded. CAEKit reads version 1. Unknown schema versions,
fields, physics kinds and condition kinds are errors. Future readers must
preserve the meaning of earlier versions or provide an explicit migration.

Validation errors locate an input using dotted JSON paths with array indices.
For example, an invalid exterior face is reported at
`physics.mesh.boundary_regions[0].faces[0]` when the first face of the first
region is invalid. A nonpositive absolute pressure derived from a gauge
reading points to the gauge object's `value_pa`. Computed-value errors use
the name and location of the computed value.

### Input mutation checks

The separate model mutation CI job replays fixed sequences over the seven
reference models. Each model supplies 512 byte mutations for
`Model::from_json` and 256 typed mutations for `caekit::solve`. That yields
3,584 reader calls and 1,792 solver calls (`READ_CASES_PER_MODEL` = 512 and
`SOLVE_CASES_PER_MODEL` = 256 in `crates/caekit/tests/model_mutations.rs`, times seven
models). The solve target bypasses the model
reader's validation and caps each iterative budget at four. Failures identify
the corpus entry and mutation index.

Run the same cases with
`cargo +stable test --locked -p caekit --test model_mutations -- --ignored`.

| Field | Meaning |
| --- | --- |
| `name` | Human-readable run name |
| `units` | `si`. Dimensional input keys carry an SI unit suffix, with the stage-defined tolerance documented below |
| `physics` | Tagged physical case with geometry identity, mesh, material and conditions |
| `linear_solver` | Optional iterative stopping settings. If present, supply all fields |
| `outputs` | Requested quantities for the selected physics case |

Coordinates use a global Cartesian frame in metres. One-dimensional cases
place an oriented local axis in that frame. The origin and unit direction map
local mesh coordinates to global sample positions. Each mesh lists ordered
vertices including both endpoints. Conditions refer to named boundary regions
defined by that mesh. The tetrahedral mesh uses zero-based volume connectivity
and named exterior triangle sets. Assignments belong to the supplied geometry
revision.

### Mesh and degree-of-freedom numbering

The vertex index is the degree of freedom for a scalar P1 field. Vector P1
unknowns are node-major, at `components * vertex + component`. Cells keep
their document order. The Rust checked tetrahedral mesh can number its edges
and faces on request. An edge uses an ascending pair of vertex indices and a
face uses an ascending triple. Global IDs are the zero-based positions of
those keys in lexicographic order, independent of boundary region order.
Local edge order is `(0,1), (0,2), (0,3), (1,2), (1,3), (2,3)`. Local face
`i` is opposite local vertex `i`. When a P2 space is introduced, its edge
degrees of freedom follow the vertices at `vertex_count + edge_id`.

The `heat_conduction` and `thermoelastic_solid` cases put thermal material and
boundary conditions under `physics.thermal`. The applied
`inward_heat_flux_w_m2` is positive into the solid, opposite to the outward
face normal. Heat rates by region use the same sign. Unassigned thermal
surfaces are insulated. The thermoelastic case adds `physics.mechanical` and
`physics.expansion` and uses one shared mesh and geometry revision.
[Heat conduction](physics/heat-conduction.md) · [Thermoelasticity](physics/thermoelastic-solid.md)

`multimaterial_heat_conduction` extends schema version 1 with a
`physics.thermal.materials` list and named `volume_regions` with per-cell
ownership. It adds the optional `interface_heat_rate` output in W with
explicit directions between volume regions. Its report includes a
`solid_energy:<region>` balance for each volume. Earlier models continue
to use their version 1 shape, report fields and typed-model hash.
[Multimaterial heat conduction](physics/multimaterial-heat-conduction.md)

Rust callers use the same `Model::from_json`, `Model::validate` and
`caekit::solve` entry points. Update exhaustive matches for
`Physics::MultimaterialHeatConduction`, `Output::InterfaceHeatRate` and
`Quantity::ByInterface`. The multimaterial configuration can be constructed
from `caekit::physics::heat_conduction` types.

The default linear settings are a relative residual tolerance of 1e-10, an
absolute residual tolerance of zero and an iteration limit of 20,000 (the `Default` of the
settings in `crates/caekit-numerics/src/lib.rs`). For the
current reference algorithm the threshold is
`max(absolute_residual_tolerance, relative_residual_tolerance * ||b||₂)`
in the solved equation's residual units. A run reports this threshold and an
independently recomputed final residual `||b − Ax||₂`. History entries during
conjugate gradient are recursive residual estimates.

If repeated correction attempts stagnate above the requested tolerance within
the binary64 rounding scale of the residual evaluation, the stage stops with
`numerical_breakdown`. Its optional `diagnostic` explains the failure and
the trace retains the measured true residual and requested threshold.

In `heat_conduction` and `multimaterial_heat_conduction`, the top-level
`linear_solver` uses W for the absolute tolerance. In
`thermoelastic_solid`, `linear_solver` uses N and `physics.thermal_solver`
uses W. Each stage reports its own threshold,
iterations, stop reason and final true residual. The mechanical stage runs
after a converged thermal stage. An iteration-limit result includes the
traces of the stages that ran and withholds physical fields and balances.

The structural solvers remove a constant prescribed translation in each
direction before forming the reduced system. Stresses, reactions and
strain energy use these relative displacements. Reported nodal displacements
include the translation and preserve the original prescribed values.

The version 1 result records a SHA-256 digest of the compact serialization of
the typed model with defaults applied. Input indentation and object key order
do not affect the digest. Array order and explicit changes to physical or
numerical input affect the digest. Preserve the source model with the result.
The report includes geometry revisions, mesh metrics, the method and execution
metadata. Quantities carry units and their sampling locations. Signed balance
terms and the signed imbalance are reported independently of requested fields.
Material citations and usage terms are stored in the hashed source model.

The CLI `check` command writes validation and mesh metrics as JSON. The `run`
command writes the run report as JSON. What a run returns for each outcome,
and its exit code, is in [Run outcomes](#run-outcomes). An algebraic
convergence threshold measures iterative error. Engineering error also depends
on the physical assumptions, geometry, mesh and properties.

## Outputs

A model requests quantities by name in `outputs`. Each physics kind supports the names
listed for it; any other name, and a repeated name, is an error. The report holds exactly
the requested quantities under `quantities`. Unit strings are written in ASCII: `m^3/s`,
`W/m^2`, `N m` and `1` for a dimensionless number.

| Output | Physics kinds | Quantity kind | Unit | Meaning |
| --- | --- | --- | --- | --- |
| `axial_displacement` | `axial_bar` | `field` at nodes | m | Displacement along the positive local axis, in vertex order |
| `axial_stress` | `axial_bar` | `field` at cells | Pa | Element stress `E (u_right − u_left)/h`, tension positive |
| `end_displacement` | `axial_bar` | `by_region` | m | Axial displacement of the two ends, keyed by the mesh's end region names |
| `reaction_force` | `axial_bar` | `by_region` | N | Force that the support exerts on the bar, positive along the local axis, for each end with a prescribed displacement |
| `axial_velocity` | `laminar_channel`, `cooling_channel` | `field` at cells | m/s | Axial velocity at the transverse cell centres, positioned at mid-length |
| `volume_flow_rate` | `laminar_channel`, `cooling_channel` | `scalar` | m^3/s | Volume flow from the inlet to the outlet |
| `mass_flow_rate` | `laminar_channel`, `cooling_channel` | `scalar` | kg/s | Density times the volume flow rate |
| `pressure_loss` | `laminar_channel`, `cooling_channel` | `scalar` | Pa | Inlet pressure minus outlet pressure |
| `absolute_pressure` | `laminar_channel`, `cooling_channel` | `by_region` | Pa | Absolute pressure at the inlet and outlet regions |
| `channel_reynolds` | `laminar_channel`, `cooling_channel` | `scalar` | 1 | Reynolds number from the mean velocity and the hydraulic diameter of twice the gap |
| `wall_drag` | `laminar_channel`, `cooling_channel` | `by_region` | N | Viscous axial force that each plate exerts on the fluid, negative when it opposes the flow |
| `fluid_bulk_temperature` | `cooling_channel` | `field` at cells | K | Coolant bulk temperature at the cell centres of the coolant mesh |
| `wall_temperature` | `cooling_channel` | `field` at samples | K | Wall temperature for each axial wall panel, one value per vertex of the through-thickness mesh, starting at the hot face |
| `outlet_temperature` | `cooling_channel` | `scalar` | K | Coolant bulk temperature in the last cell of the coolant mesh |
| `heat_transfer_rate` | `cooling_channel` | `scalar` | W | Total heat rate from the wall to the coolant, positive when the coolant is heated |
| `interface_heat_flux` | `cooling_channel` | `field` at interface faces | W/m^2 | Heat flux leaving the wall towards the coolant on each axial wall panel, positive when the coolant is heated |
| `displacement_vector` | `elastic_solid`, `thermoelastic_solid` | `vector_field` at nodes | m | Global displacement components, in vertex order |
| `cauchy_stress` | `elastic_solid`, `thermoelastic_solid` | `tensor_field` at cells | Pa | Cauchy stress at the tetrahedron centroids, tension positive |
| `von_mises_stress` | `elastic_solid`, `thermoelastic_solid` | `field` at cells | Pa | Von Mises equivalent stress at the tetrahedron centroids |
| `nodal_reaction` | `elastic_solid`, `thermoelastic_solid` | `vector_field` at nodes | N | Support reaction acting on the solid, zero on unconstrained components |
| `elastic_energy` | `elastic_solid`, `thermoelastic_solid` | `scalar` | J | Elastic strain energy integrated over the solid |
| `solid_temperature` | `heat_conduction`, `multimaterial_heat_conduction`, `thermoelastic_solid` | `field` at nodes | K | Absolute temperature, in vertex order |
| `solid_heat_flux` | `heat_conduction`, `multimaterial_heat_conduction`, `thermoelastic_solid` | `vector_field` at cells | W/m^2 | Heat flux `q = −k∇T` at the tetrahedron centroids |
| `boundary_heat_rate` | `heat_conduction`, `multimaterial_heat_conduction`, `thermoelastic_solid` | `by_region` | W | Integrated heat rate through each named exterior region, positive into the solid |
| `interface_heat_rate` | `multimaterial_heat_conduction` | `by_interface` | W | Integrated heat rate across the shared faces of each pair of volume regions, positive from `from_region` to `to_region` |

A `field`, `vector_field` or `tensor_field` lists `positions_m` in the global frame next
to `values`, in the same order. A `by_region` quantity maps region names to values.

### Report size

Aside from metadata and balance records, a steady run report grows with the
requested outputs and the number of stages that ran. Each scalar has one value.
Each sampled field has one position and one value per entity in its sampling
domain. A nodal field uses the number of vertices and a cell field uses the
number of cells. Cooling-channel interface flux uses the number of axial wall
panels. Wall temperature uses that count multiplied by the number of vertices
through the wall thickness. Region outputs have at most one value per relevant
named region. Interface heat rates have at most one value per unordered pair of
volume regions, or `r (r - 1) / 2` for `r` regions.

Each executed stage adds a fixed set of fields and at most
`1 + max_iterations` residual values, including the initial residual. The
limit is taken from that stage's settings block. Test the bounds on all saved
models with `cargo test --locked -p caekit --test model_identity`.

## Numerical stages

Each entry of `convergence` is one numerical stage. The run's `stop_reason` is that of
the first stage that did not converge.

| Stage | Physics kinds | Residual unit | Algorithm | Settings |
| --- | --- | --- | --- | --- |
| `elastic_equilibrium` | `axial_bar` | N | `jacobi_preconditioned_conjugate_gradient` | `linear_solver` |
| `axial_momentum` | `laminar_channel`, `cooling_channel` with a pressure drive | Pa | `jacobi_preconditioned_conjugate_gradient` | `linear_solver` |
| `unit_gradient_axial_momentum` | `laminar_channel`, `cooling_channel` with a volume-flow drive | Pa | `jacobi_preconditioned_conjugate_gradient` | `linear_solver` |
| `coupled_wall_coolant_energy` | `cooling_channel` | W | `partitioned_gauss_seidel` | `physics.coupling_solver` |
| `solid_elastic_equilibrium` | `elastic_solid`, `thermoelastic_solid` | N | `jacobi_preconditioned_conjugate_gradient` | `linear_solver` |
| `solid_heat_conduction` | `heat_conduction`, `multimaterial_heat_conduction`, `thermoelastic_solid` | W | `jacobi_preconditioned_conjugate_gradient` | `linear_solver`; `physics.thermal_solver` in `thermoelastic_solid` |
| `solid_interface_equilibration` | `multimaterial_heat_conduction` | W | `jacobi_preconditioned_conjugate_gradient` | `linear_solver` |

The absolute residual tolerance of a stage is in its residual unit. The optional
`physics.coupling_solver` of the cooling channel takes `relative_residual_tolerance`,
`absolute_residual_tolerance_w` and `max_iterations` and defaults to 1e-10, 1e-10 W and
1,000 sweeps (the `Default` in `crates/caekit-thermal/src/lib.rs`).

### Portable binary64 calculations

Norms scale the components before squaring and accumulate in index order.
For tetrahedral geometry the scale is a power of two, which avoids rounding
components during normalization except when a scaled component underflows.
Using the maximum component itself as the scale rounded the geometry of the
multimaterial reference differently. Its interface equilibration then started
with a residual of zero rather than the previous `3.552713678800501e-15 W`
and took zero iterations instead of one. Scaling the geometry by a power of
two preserved the previous stage decisions without changing the tolerances. To reproduce
the first two sentences, replace the `scale` of `length` in `crates/caekit-mesh/src/lib.rs` by
`maximum` and run `cargo run --locked -- run examples/multimaterial_heat_conduction.json`:
the stage `solid_interface_equilibration` then reports `iterations` 0 and the history `[0]`.
With the power-of-two scale of the code it reports `iterations` 1 and the history
`[3.552713678800501e-15, 0]`, as the commit before this change does (measured on Windows 11
x86-64, rustc 1.100.0).
The solver normalizes signed zero when computing a threshold and retains the
original value in the typed model and its digest.

The [native and WASI reference check](../README.md#webassembly) measured equal
stage decisions, residual histories and report digests for all seven models on
Windows x86-64 and WASI after normalizing the platform fields (the last command of that
section, `node scripts/verify-reference-cases.mjs target/debug/caekit
target/wasm32-wasip1/debug/caekit.wasm`: 7 tests pass). Comparing the new
native reports with native reports captured before the change also preserved
all model digests and stage decisions. Quantities and balances stayed within
the reference check's existing tolerances. The comparison was repeated for this page:
the CLI built from the commit before this change (`git archive HEAD`) and from the working tree
give the same `model_sha256`, stage names, iteration counts, stop reasons and history
lengths for the seven saved models, and the quantities of five models are bit-identical. The
largest relative differences are 1.86e-16 (`elastic_solid`) and 1.24e-16
(`thermoelastic_solid`).

## Run outcomes

Each call ends in exactly one of these outcomes. The report's `stop_reason` is that of the
first stage that did not converge, or `converged` when every stage converged. The four
surfaces are `caekit::solve` in Rust, `caekit run` on the command line, `caekit.solve` in
Python and `solve` in the WebAssembly package.

| Outcome | Report | Rust | CLI `run` | Python | WebAssembly |
| --- | --- | --- | --- | --- | --- |
| Converged | `stop_reason` is `converged`. The report holds every stage trace, every balance and exactly the requested quantities | `Ok(report)` | Report on standard output, exit 0 | Returns the report | Returns the report |
| Iteration limit | `stop_reason` is `iteration_limit`. The report holds the traces of the stages that ran; the last one stopped at its `max_iterations` with its true residual above its threshold. `balances` is `[]` and `quantities` is `{}` | `Ok(report)` | Report on standard output, a one-line message on standard error, exit 2 | Returns the report | Returns the report |
| Numerical breakdown | `stop_reason` is `numerical_breakdown`. The report is as for the iteration limit. The failed stage may carry a `diagnostic`. Its `residual_threshold` is `null` when the stage stopped before forming it, on invalid input to the solve, and its `final_true_residual_l2` is `null` when the residual was not finite | `Ok(report)` | Report on standard output, a one-line message on standard error, exit 2 | Returns the report | Returns the report |
| Input error | No report. The message names the JSON path of the field. The same error reports a computed value that fails a physical check, such as a temperature at or below 0 K or a flow response that is not positive | `Err(Error::Json)`, `Err(Error::UnsupportedModelVersion)` or `Err(Error::InvalidModel)` | Message on standard error, nothing on standard output, exit 1 | Raises `InvalidModelError`, also for a mapping with no JSON form, such as one that holds NaN | Throws a `CaekitError` with `kind` `invalid_model` |
| Non-finite result | No report. An assembled or recovered value, a balance sum or a quantity of a converged run is NaN or infinite. A non-finite value inside an iterative solve is a numerical breakdown instead | `Err(Error::NonFiniteResult)` | Message on standard error, exit 1 | Raises `CalculationError` | Throws a `CaekitError` with `kind` `calculation` |
| Insufficient memory | No report. In a solve only two buffers are reserved with a check: the degrees of freedom of `elastic_solid` and `thermoelastic_solid`, and the `wall_temperature` samples of `cooling_channel` | `Err(Error::InsufficientMemory)` | Message on standard error, exit 1 | Raises `CalculationError` | Throws a `CaekitError` with `kind` `calculation` |
| Unreadable model or bad argument | No report | Not applicable: the caller passes a typed `Model` | A file that cannot be read, standard output that cannot be written or wrong arguments: message on standard error, exit 1 | Raises `TypeError` for an argument that is not one of the accepted forms; reading a path can raise `OSError` | Throws a `CaekitError` with `kind` `bad_argument` for an argument that is not a model document or an object that `JSON.stringify` rejects |

`caekit check` exits 0 for a valid model and 1 when it reports an error; Python and
WebAssembly `check` raise and throw with the same mapping as `solve`. The CLI does not tell
the outcomes with exit code 1 apart, and the bindings do not tell a non-finite result from
insufficient memory by type; the message names the value or the buffer.

Any allocation other than the two checked buffers is not caught. A failure there follows
Rust's default handling, which aborts the process: the CLI and the Python interpreter end
without exit code 0, 1 or 2, and in WebAssembly the abort is a trap that surfaces as a
`WebAssembly.RuntimeError` rather than a `CaekitError`. This path has not been tested.

Tests cover the converged and iteration-limit rows and the input error on all four
surfaces (`crates/caekit-cli/tests/reference_cases.rs`,
`crates/caekit-py/tests/test_caekit.py`, `crates/caekit-wasm/tests/examples_test.mjs` and
the Rust tests of each kind). The numerical breakdown is tested in Rust
(`crates/caekit/tests/thermoelastic_solid.rs`, `crates/caekit-numerics/tests/sparse_system.rs`);
the CLI and the bindings take the same branch as for the iteration limit, but no test drives
a breakdown through them. No test asserts a non-finite result or insufficient memory: their
mapping is in `crates/caekit-py/src/lib.rs` and `crates/caekit-wasm/src/lib.rs`.

### Automated clients

A program that drives CAEKit, including an AI assistant, accepts a result only when the
report's `stop_reason` is `converged`: CLI exit code 0, or a returned report with that value
in Python and WebAssembly, where a stopped run is returned, not raised. Exit code 2
(`iteration_limit` or `numerical_breakdown`) and exit code 1 (no report) are failures; the
client reports them and does not read quantities from that run. A balance that closes is not
proof that the result is correct: it bounds only the algebraic error of the solve, as
[What a closed balance proves](#what-a-closed-balance-proves) derives. A converged run is not
validation either. It shows that the discrete equations were solved to the reported
threshold, not that the model, the mesh, the properties or the physical assumptions answer
the engineering question. The analytical reference and refinement study of each kind on its
[physics page](physics/) verify the method, and only a comparison with measurement
validates a model of a real design.

## What a closed balance proves

A balance whose test functions sum to one closes to the algebraic residual of its stage. It
is consistency evidence for the solve, not correctness evidence for the field: a wrong
property, load or mesh gives a balance that closes just as well.

**Derivation.** Let a stage solve the assembled equations `K u = f` over free unknowns `F`
and prescribed unknowns `P`. Split the residual of the full system as
`f − K u = g − K₀ u`, where `K₀` is the part of the operator that the test vector `w` below
annihilates (`wᵀK₀ = 0`) and `g` holds every load, including the convection term
`f_h − M_h u` at the solved temperature. The solver eliminates `P` and solves `A x = b` with
`A = K_FF` and `b = f_F − K_FP u_P`, so its true residual is `r = b − A x = (g − K₀u)_F` and
the stage reports `final_true_residual_l2 = ‖r‖₂`. A reaction is the residual of an
eliminated equation, `R_P = (K₀u − g)_P`. A balance adds the totals of `g`, weighted by
`w`, and the weighted reactions:

```text
imbalance = wᵀg + w_Pᵀ R_P
          = w_Fᵀ g_F + w_Pᵀ (K₀u)_P
          = w_Fᵀ g_F − w_Fᵀ (K₀u)_F        (wᵀK₀u = 0)
          = w_Fᵀ r
```

For a conforming P1 space the basis functions sum to one, so `w = 1` at every scalar unknown
is a constant test function, and the conduction operator annihilates it. For elasticity,
`w = 1` on the unknowns of one displacement component is a rigid translation, and
`w_i = e_k × (x_i − x_0)` at vertex `i` is a rigid rotation about axis `k`; neither strains
the solid. A thermal-expansion load `∫ ε(w):C:ε_th dV` vanishes against both, so it does not
enter. Cauchy–Schwarz gives

```text
|imbalance| = |Σ_{i∈F} r_i| ≤ √n_free · ‖r‖₂               (w = 1)
|imbalance| ≤ ‖w_F‖₂ · ‖r‖₂ ≤ ρ · √n_free · ‖r‖₂            (rotation)
```

with `n_free` the number of free unknowns and `ρ` the largest distance of a vertex from the
reference vertex `x_0`. The finite-volume rows of the channel and cooling-channel kinds have
the same form with one cell indicator per row: interior face fluxes cancel in the sum, and
the boundary fluxes are the reported terms.

The derivation needs three things: the terms are totals of the assembled load and the
residual reactions ([SPEC-CON-003](design/spec.md#spec-con-003)), the test vector lies in the
discrete space and the operator annihilates it, and the residual belongs to the same iterate
as the balance. These balances satisfy them:

| Balance | Kinds | Test vector | Stage | Bound |
| --- | --- | --- | --- | --- |
| `global_axial_force` | `axial_bar` | 1 at each node | `elastic_equilibrium` | `√n_free · ‖r‖₂` |
| `channel_axial_momentum` | `laminar_channel`, `cooling_channel` | 1 in each cell | `axial_momentum` or `unit_gradient_axial_momentum` | `W L s · √n_cells · ‖r‖₂` |
| `wall_energy`, `coolant_energy`, `system_energy` | `cooling_channel` | 1 in each wall panel, each coolant cell, or both | `coupled_wall_coolant_energy` | `√n · ‖r‖₂` over the rows summed |
| `solid_force_x`, `solid_force_y`, `solid_force_z` | `elastic_solid`, `thermoelastic_solid` | Rigid translation | `solid_elastic_equilibrium` | `√n_free · ‖r‖₂` |
| `solid_moment_x`, `solid_moment_y`, `solid_moment_z` | `elastic_solid`, `thermoelastic_solid` | Rigid rotation about the first vertex | `solid_elastic_equilibrium` | `ρ · √n_free · ‖r‖₂` |
| `solid_energy` | `heat_conduction`, `multimaterial_heat_conduction`, `thermoelastic_solid` | 1 at each vertex | `solid_heat_conduction` | `√n_free · ‖r‖₂` |

The channel rows are forces per unit wall area in Pa, so the wall area `W L`, from `width_m`
and `length_m`, converts their sum to N. With a pressure drive `s = 1`; both reference models use the volume-flow drive,
so the test does not exercise that case. The volume-flow drive solves for a 1 Pa/m
reference gradient and scales the velocity by `s = Δp / (L · 1 Pa/m)`, which scales the
residual by the same factor. For the force balances `n_free` counts the free unknowns of all
three components, which also bounds the sum over one component.

Three balances do not satisfy the premise. `channel_mass` adds the same computed mass flow
with both signs and is zero by construction. `interface_energy` of the cooling channel sums
the same overlap heat rates from the wall side and the fluid side, so it closes to rounding
whatever the residual. The `solid_energy:<region>` balances of
`multimaterial_heat_conduction` use interface powers from the projection of the
`solid_interface_equilibration` stage, and a region indicator is not a sum of the P1 test
functions, because the vertices on an interface belong to both regions.

`crates/caekit/tests/balance_bounds.rs` checks each balance of the table on the seven
reference models. The identity is exact only in exact arithmetic, so the test adds a
binary64 allowance of `(entities + 1) ε` times the absolute size of the summed values, with
`entities` the number of mesh vertices or cells; the reason is stated where it is set. The
command
`cargo test --locked -p caekit --test balance_bounds -- --nocapture --test-threads=1`
printed these imbalances and bounds on Windows 11 x86-64 with rustc 1.100.0; `n_free`
follows from the model as the test counts it:

| Model | Balance | `n_free` | Imbalance magnitude | Bound |
| --- | --- | --- | --- | --- |
| `axial_bar` | `global_axial_force` | 5 | 0 N | 4.432e-12 N |
| `laminar_channel` | `channel_axial_momentum` | 32 | 2.602e-18 N | 7.643e-17 N |
| `cooling_channel` | `channel_axial_momentum` | 32 | 2.602e-18 N | 7.643e-17 N |
| `cooling_channel` | `wall_energy` | 31 | 6.264e-8 W | 1.200e-7 W |
| `cooling_channel` | `coolant_energy` | 48 | 4.547e-13 W | 1.493e-7 W |
| `cooling_channel` | `system_energy` | 79 | 6.264e-8 W | 1.916e-7 W |
| `elastic_solid` | `solid_force_x`, `_y`, `_z` | 12 | 2.561e-9, 1.242e-10, 7.761e-10 N | 6.400e-9 N |
| `elastic_solid` | `solid_moment_x`, `_y`, `_z` | 12 | 4.238e-10, 2.270e-9, 8.731e-10 N m | 1.344e-8 N m |
| `heat_conduction` | `solid_energy` | 1 | 1.563e-13 W | 0 W |
| `multimaterial_heat_conduction` | `solid_energy` | 4 | 4.441e-16 W | 1.088e-15 W |
| `thermoelastic_solid` | `solid_energy` | 4 | 0 W | 0 W |
| `thermoelastic_solid` | `solid_force_x`, `_y`, `_z` | 8 | 0, 7.451e-11, 1.552e-10 N | 8.866e-10 N |
| `thermoelastic_solid` | `solid_moment_x`, `_y`, `_z` | 8 | 5.858e-12, 2.173e-10, 4.346e-11 N m | 8.867e-10 N m |

Every imbalance is within its bound except the heat-conduction reference, which needs the
allowance. Its single free equation is solved to a true residual of exactly zero, and its
imbalance is the rounding of the ±200 W prescribed-temperature reactions, 7.8e-16 of their
size, within the allowance of 8.882e-13 W. The wall-energy imbalance of the cooling
channel reaches 0.52 of its bound, so the bound is not loose by orders of magnitude there.
