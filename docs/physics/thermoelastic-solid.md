# Three-dimensional linear thermoelasticity

The `thermoelastic_solid` case solves steady heat conduction and then
small-strain mechanical equilibrium on the same tetrahedral mesh. It uses
constant isotropic properties and one-way coupling from temperature to
mechanics. The heat stage supplies temperatures in the original node order
and both stages use the same checked topology and geometry revision.

```sh
cargo run --locked -- check examples/thermoelastic_solid.json
cargo run --locked -- run examples/thermoelastic_solid.json
cargo test --locked --test thermoelastic_solid -- --nocapture
```

The Rust API uses the same versioned input and report as the CLI.

```rust,no_run
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let json = std::fs::read("examples/thermoelastic_solid.json")?;
    let model = caekit::Model::from_json(&json)?;
    let report = caekit::solve(&model)?;
    assert_eq!(report.stop_reason, caekit::StopReason::Converged);
    println!("{:?}", report.quantities[&caekit::Output::CauchyStress]);
    Ok(())
}
```

## Model and stages

- `physics.geometry` and `physics.mesh` define the shared solid.
- `physics.thermal` uses the independent conduction case's thermal material,
  optional volumetric source and exterior thermal conditions. [Heat conduction](heat-conduction.md)
- `physics.mechanical` takes the elastic material and the existing surface
  tractions, pressures, partial displacements and optional `body_force_n_m3`.
  The displacement constraints must remove all six rigid-body modes.
  [Elastic solid](elastic-solid.md)
- `physics.expansion` specifies the stress-free `reference_temperature_k`
  in K and a finite, signed `linear_expansion_coefficient_per_k` in 1/K.
  Use the mechanical material's source to cite the elastic and expansion
  properties.

The top-level `linear_solver` sets the mechanical tolerance in N and
`physics.thermal_solver` sets the thermal tolerance in W. Each accepts
`relative_residual_tolerance`, `absolute_residual_tolerance` and
`max_iterations` and defaults independently. The trace shows the stage's
stop reason, iteration count, residual threshold and recomputed residual
norm. An unconverged heat stage ends the combined solve before equilibrium.
An unconverged equilibrium ends the run with both traces. Iteration-limit
reports have exit code 2 and carry the executed stages without physical
fields or balances. [Model and report format](../model.md)

## Formulation and outputs

At temperature `T`, the isotropic free strain is
`ε_th = α(T − T_ref) I`. Stress is `σ = C : [ε(u) − ε_th]`.
The stiffness uses the same linear elastic tensor as the standalone solid.
The thermal load adds `∫ ε(v):C:ε_th dV` to the mechanical right-hand
side. A tetrahedron's linear temperature gives an exact centroid mean
for this integral and for its internal force. Reactions are the resulting
internal forces minus external mechanical loads on constrained components.
Positive reaction components act on the solid along the global coordinate
directions. Force and moment balances use external mechanical loads and
these support reactions, with moments taken about the first mesh vertex.

When the displacement supports admit a uniform free expansion about a
common origin, the solver removes that affine displacement before assembly
and restores it in the reported displacements. Small external loads can
then set the residual scale independently of a large temperature rise.

For each externally loaded axis, a coupled run also checks the balance
between the recovered support reactions and applied forces. The global
allowance is `sqrt(node_count) * max(abs_tol, rel_tol * ||applied_axis||₂)`
plus a binary64 summation allowance. Failure withholds physical fields and
returns a `numerical_breakdown` diagnostic with the imbalance, tolerance
and worst tetrahedral mean ratio. The trace still gives the algebraic
residual. Highly elongated elements can limit its attainable precision.

The stored elastic energy is `½ ∫ ε_el:C:ε_el dV`. For linear temperature
inside a cell, the squared strain can vary quadratically. CAEKit integrates
the mean elastic-strain contribution plus the exact contribution of the
temperature variation. The latter uses the thermal-strain variance
`Σ_i (α(T_i − T_ref) − mean)^2/20` times `9 K_bulk V/2`.

| Output | Sampling | Unit |
| --- | --- | --- |
| `solid_temperature` | Vertex, absolute temperature | K |
| `solid_heat_flux` | Cell centroid, global vector | W/m² |
| `boundary_heat_rate` | Exterior region, positive into the solid | W |
| `displacement_vector` | Vertex, global vector | m |
| `nodal_reaction` | Vertex, global vector with zero on free components | N |
| `cauchy_stress` | Cell centroid, global tensor with tension positive | Pa |
| `von_mises_stress` | Cell centroid | Pa |
| `elastic_energy` | Integral over the solid | J |

## Analytical checks

- **Free expansion.** A 1 m bar heated by 50 K with
  `α = 12 × 10⁻⁶/K` extends by 0.60 mm. On the three coordinate planes
  through the origin, the respective normal displacements are fixed and
  the tangential components can slide. The affine displacement
  `u = α ΔT [x,y,z]` is compatible with those supports. Stress, support
  reaction and elastic energy vanish.
- **Axially restrained expansion.** Adding axial restraint at the other
  end leaves transverse expansion free. With `E = 200 GPa`,
  `ν = 0.3` and a 100 mm² cross section, the example has
  `σ_xx = −E α ΔT = −200e9 Pa · 12e-6/K · 50 K = −120 MPa`. The left and right supports exert
  `σ A = 120e6 Pa · 1e-4 m²` = +12 kN and −12 kN in the x direction and the elastic energy is
  `σ² V/(2E) = (120e6 Pa)² · 1e-4 m³/(2 · 200e9 Pa)` = 3.6 J.
  The lateral strain is `(1 + ν) α ΔT`. The saved model measures −120000000.00000006 Pa,
  end reactions 12000 and −12000 N and 3.600000000000001 J
  (`cargo run --locked -- run examples/thermoelastic_solid.json`).
- **Temperature gradient.** A 1 m block with sliding side restraints has
  `T − T_ref = g x`, `g = 50 K/m` and zero axial displacement at
  `x = 0`. Equilibrium and the free far end give `σ_xx = 0` and
  `u_x = [(1 + ν)/(1 − ν)] α g x²/2`. The tests sample inside every
  tetrahedron and at the interior sections `x = 0.25, 0.5, 0.75 m`.

The two affine mechanical checks use 5e-12 m for displacement, 0.05 Pa for
stress and 5e-6 N for support forces. The absolute allowances cover linear
solver and roundoff error on the 0.60 mm, 120 MPa and 12 kN scales, including
components expected to vanish. The free and restrained energy tolerances
are 1e-10 J and 1e-8 J against the 3.6 J restrained scale.

The uniform-mesh check `uniform_block_matches_the_gradient_displacement_at_the_interior_sections`
keeps the older pointwise form of the temperature-gradient case: the measured
interior RMS displacement errors for 2, 4 and 8 divisions per axis were 24.00,
6.53 and 1.71 μm (first measured on Linux; printed again on Windows 11 by the command at
the top of this page). The test requires at least
a factor of 2.7 per halving of spacing, with a 5 μm finest-grid tolerance at
the internal sections and on the interior RMS error. This allows
discretization error while resolving the quadratic profile against the
analytical 557 μm tip displacement. The order itself is judged by the study
below, not by this ratio. The single-cell
energy check uses a fully restrained unit tetrahedron with the same
temperature gradient and verifies 4,500 J to 1e-8 J against the analytical
integral `∫ x² dV = 1/60`. The value follows from `½ ∫ 9 K_bulk (α g x)² dV` with
`K_bulk = E/(3(1 − 2ν))` = 1.6667e11 Pa, `α g` = 6e-4/m and the unit tetrahedron:
`½ · 1.5e12 Pa · 3.6e-7 · (1/60)` = 4,500 J (derived). Setting `α = 0` or `T = T_ref` recovers the
standalone mechanical quantities and balances.

## Refinement study

The study is `a_temperature_gradient_drives_the_local_displacement_and_converges_with_refinement`
in `crates/caekit/tests/thermoelastic_solid.rs`. It follows
[SPEC-VER-003](../design/spec.md#spec-ver-003) and
[blueprint § 11.2](../design/blueprint.md#112-refinement-studies), and it uses the
fitting and budget functions of `caekit_numerics::verify`.

```sh
cargo test -p caekit --test thermoelastic_solid a_temperature_gradient_drives_the_local_displacement_and_converges_with_refinement --locked -- --nocapture
```

Every number in this section is printed by that command. They were taken on
Windows 11 with an Intel Core i7-9750H and 15.8 GB of memory, with rustc 1.100.0
in the unoptimized test profile; the study test took 39.34 s in the run that printed them, on a machine that
other builds shared (the whole test file took 35.10 s in another run).
The mesh and quadrature helpers of the test have their own tests,
`the_perturbed_family_keeps_every_cell_orientation_and_the_boundary_planes` and
`the_study_quadrature_integrates_degree_seven_monomials_exactly`.

**Exact solution.** The block is `1 × 0.5 × 0.5 m`. The planes `x = 0` and
`x = 1 m` have 300 K and 350 K, so `T − T_ref = g x` with `g = 50 K/m`. The
normal displacement is fixed on the four side planes and on `x = 0` (the
sliding supports of the case above). The temperature is linear, so the thermal
stage is exact in the discrete space (the test checks it to 2e-8 K at every
vertex). The lateral strains vanish, and the free far end with `dσ_xx/dx = 0`
gives `σ_xx = 0`, so `ε_xx = [(1 + ν)/(1 − ν)] α (T − T_ref)` and, with
`u_x(0) = 0`,

```text
u = (u_x(x), 0, 0),   u_x(x) = [(1 + ν)/(1 − ν)] α g x²/2
```

with `ν = 0.3` and `α = 12 × 10⁻⁶/K`. It satisfies equilibrium and every
boundary condition, so it is the solution of the case. It is quadratic in `x`,
so it lies outside the space of piecewise linear displacements (a patch test
would pass only for a uniform temperature, see the trap below).

**Levels.** Four self-similar levels of 2, 4, 8 and 16 divisions per axis
(`r = 2`; 48, 384, 3,072 and 24,576 tetrahedra; `h` is the division length
along `x`, 0.5 to 0.0625 m). Each level is the image of the structured mesh
under the same smooth map: the interior vertices move by
`(0.04, 0.02, 0.02) m · sin(πx/1) sin(πy/0.5) sin(πz/0.5)`, which vanishes on the
boundary. The cells are therefore not uniform and not aligned with the
solution, the boundary regions keep their planes, and the family stays
self-similar.

**Norms.** Over the whole block, evaluated with a degree-seven rule on each
tetrahedron (blueprint 11.2 asks for degree `2p + 3` with `p = 2`). The
integrands are polynomials of degree at most four, so the rule is exact for
them. Both include the lateral components, whose exact value is zero.

- `‖u − u_h‖_L²` in m^(5/2), formal order `p_f = 2`.
- `|u − u_h|_H¹ = (∫ |∇(u − u_h)|_F² dV)^(1/2)` in m^(3/2), formal order
  `p_f = 1`. With the displacement constraints it is equivalent to the energy
  norm by Korn's inequality.

For a linear tetrahedron and a smooth solution these are the orders of
Ciarlet (2002). [Provenance](../provenance.md)

**Measured errors.** Relative change is `|e − e_tight|/e_tight`, where `e_tight`
comes from the same level solved with the relative residual tolerance of both
stages divided by 100 (1e-10 to 1e-12).

| Divisions | `h` (m) | Cells | L² error | H¹ error | L² change | H¹ change |
| --- | --- | --- | --- | --- | --- | --- |
| 2 | 0.5 | 48 | 1.292440e-5 | 8.484245e-5 | 3.633e-13 | 2.176e-13 |
| 4 | 0.25 | 384 | 3.594664e-6 | 4.212637e-5 | 1.444e-11 | 5.397e-12 |
| 8 | 0.125 | 3,072 | 9.525491e-7 | 2.092519e-5 | 5.661e-11 | 1.061e-11 |
| 16 | 0.0625 | 24,576 | 2.447968e-7 | 1.042051e-5 | 3.243e-11 | 1.646e-11 |

**Band rule.** The study applies the one band rule of
[blueprint § 11.2](../design/blueprint.md#112-refinement-studies) (ruling R-9)
through `caekit_numerics::verify::band_check`, whose rustdoc derives it, and
the [elastic solid](elastic-solid.md#refinement-study) page applies the same
rule. The observed order `p̂` is the least-squares slope of `ln e` against
`ln h` over the finest three levels, which at constant ratio is the mean of the
two finest pairwise orders. The accepted fitted orders are `p_f ± δ` with
`δ = Δp + 1.5 s`.

- `Δp = 0.01` is the share of the algebraic error (below).
- `s` is the measured spread of the two finest pairwise orders, `|p̂₂ − p̂₃|`.
- The band is valid when the distances `d₂`, `d₃` of those two pairwise orders
  from `p_f` shrink by `ρ = d₂/d₃ ≥ 2` per level, the rate of a leading
  correction of order `h¹` or higher at `r = 2`. A finest distance
  `|d₃| ≤ Δp` needs no rate.

The test fails if the fit leaves the band or if the shrink condition is not met,
with the message of `BandCheck::failure`. A super-optimal order is outside the
band and is a defect. It also requires the half-width to stay at or below 0.5,
half the gap between the formal orders of the two norms: a wider band could not
tell a lost order from the expected one. This only narrows the acceptance.

**Fitted order.** Pairwise orders of successive levels (2→4, 4→8, 8→16) and the
fit of the finest three levels (4, 8 and 16 divisions):

| Norm | Pairwise orders | `s` | `δ` | Band | Fitted order | `d₃` | `ρ` |
| --- | --- | ---: | ---: | --- | ---: | ---: | ---: |
| L² | 1.846168, 1.915992, 1.960208 | 0.044217 | 0.076325 | 1.923675 to 2.076325 (`2 ± 0.076325`) | 1.938100 | 0.039792 | 2.1112 |
| H¹ | 1.010063, 1.009483, 1.005814 | 0.003668 | 0.015502 | 0.984498 to 1.015502 (`1 ± 0.015502`) | 1.007649 | −0.005814 | 1.6309 |

Both fitted orders are inside their bands (L² lies 0.061900 and H¹ 0.007649
from the formal order). L² approaches 2 from below with `ρ = 2.1112 ≥ 2`, so the
band applies. For H¹ the finest distance `|d₃| = 0.005814` is within
`Δp = 0.01`, so the rule needs no shrink rate there; the measured `ρ = 1.6309` is
below 2 because both distances are already within 0.01 of the formal order and
change by a few thousandths between levels. For comparison only, the
least-squares slope over all four levels is 1.908310 (L²) and 1.008556 (H¹).

**Algebraic error.** Each level is solved again with
`relative_residual_tolerance` of both stages divided by 100
(`verify::TOLERANCE_DIVISOR`). With `Δp = 0.01` and `r = 2`, the budget of
`verify::algebraic_error_budget` is `δ = Δp · ln r / 2 = 3.465736e-3`, from the
first-order shift `2δ/ln r` of an observed order when both error norms carry a
relative error of at most `δ` ([SPEC-VER-003](../design/spec.md#spec-ver-003)).
The largest relative change of an error norm in the table is `5.661e-11` (L²,
8 divisions) and `1.646e-11` (H¹, 16 divisions), about `6 × 10⁷` times below `δ`,
so the algebraic error does not move the observed orders. The test fails with the
warning of `AlgebraicCheck` if any level exceeds the budget.

**Trap.** With 350 K on both planes the temperature is uniform and `g = 0`. The
exact solution is `u_x = [(1 + ν)/(1 − ν)] α (350 − 300) x`, which is affine,
and linear tetrahedra represent it exactly (a patch test). The same four levels
measure only roundoff and the iterative error:

| Divisions | L² error | H¹ error |
| --- | --- | --- |
| 2 | 1.531360e-15 | 2.065180e-14 |
| 4 | 1.044785e-15 | 1.345734e-14 |
| 8 | 1.027605e-15 | 1.519178e-14 |
| 16 | 9.248314e-16 | 1.555163e-14 |

They are `10⁻⁸` or less of the finest error of the same norm in the study, and they do not fall
with `h`. The same rule is applied to them, with the formal orders of the real
study. Its fit over the finest three levels gives:

| Norm | Fitted order | `s` | Band of its own spread | Verdict of `band_check` |
| --- | ---: | ---: | --- | --- |
| L² | 0.087972 | 0.128104 | `2 ± 0.202156` | fitted order outside the band |
| H¹ | −0.104337 | 0.141123 | `1 ± 0.221684` | fitted order outside the band |

The test requires that the rule rejects the trap on the band (the message of
`BandCheck::failure` starts with `fitted order`), that the trap fit also lies
outside the band of the real study (`2 ± 0.076325`, `1 ± 0.015502`), and that the trap
errors are below `10⁻⁶` of the finest error of the real study. A model that the
discretization reproduces exactly cannot pass as an order study.

The examples use synthetic geometry and materials under the project
license. Their analytical checks follow from heat conduction and isotropic
elastic equilibrium. [Provenance](../provenance.md)
