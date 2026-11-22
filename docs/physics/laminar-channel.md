# Fully developed laminar channel flow

The `laminar_channel` case describes incompressible, Newtonian flow of constant
density and viscosity between two parallel stationary plates. Both transverse
endpoints must be assigned a no-slip condition. The axial flow is steady and
fully developed. The width is a span multiplier for the two-dimensional
solution. Side walls, entrance development, compressibility, secondary motion
and turbulent stresses are outside this model. Apply the model where the flow
remains laminar and the width and developed length justify the approximation.

Specify upstream and downstream pressure, or the inlet volumetric flow and
outlet pressure. Absolute pressure uses pascals. A gauge reading also requires
its reference absolute pressure. The reported port pressures are absolute.
Pressure loss preserves small differences between readings even when the
absolute port pressures round to the same binary64 value. The axial
direction points from the inlet to the outlet. A solved
case has positive flow in this direction.

For gap `H` and length `L`, the transverse momentum equation is
`−μ u'' = (p_in − p_out)/L`. The cell-centred finite-volume method integrates
this equation over each interval. Interior viscous face fluxes are equal and
opposite in neighbouring cells. A wall flux spans the distance from the cell
centre to the no-slip face. The matrix is symmetric positive definite and the
sparse solver checks its force-per-area residual in pascals. The prescribed
volume case solves a reference gradient of 1 Pa/m and scales the velocity
field and gradient by the discrete integral of that field. Its convergence
history is for the reference gradient equation.

The reported wall drags come from the wall viscous fluxes. The momentum balance
compares these forces with the pressure force on the channel cross-section.
Constant volumetric flow gives the same mass flow at both ports. The reported
Reynolds number uses the mean velocity and parallel-plate hydraulic diameter
`2H`.

For fully developed laminar flow the closed-form velocity is
`u(y) = (Δp/(2 μ L)) y (H − y)`. Its integral gives
`Δp = 12 μ L Q/(width H³)`. The parallel-plate solution is treated in Shah and
London (1978), chapter "Parallel Plates" (see [provenance](../provenance.md)).
The inputs of `examples/laminar_channel.json` are `L` = 2 m, `H` = 10 mm,
width 0.1 m, `μ` = 1e-3 Pa s, density 1000 kg/m³ and `Q` = 1e-5 m³/s. They give
`Δp = 12 · 1e-3 · 2 · 1e-5/(0.1 · 0.01³)` = 2.4 Pa analytical loss, `ρ Q` = 0.01 kg/s mass
flow, a mean velocity `Q/(width H)` = 0.01 m/s and a Reynolds number
`ρ U (2H)/μ` = 200 (derived). The saved model solves to 2.395322 Pa on its 32 uniform
cells (`cargo run --locked -- run examples/laminar_channel.json`). The 32-cell
pressure loss tolerance is 0.005 Pa: the derived error of that mesh,
`2 Δp/(n² + 2)` = 4.678e-3 Pa (see [why the order is 2](#why-the-order-is-2)), sits just
below it. Doubling the number of
uniform cells should reduce the pressure loss error by about four. The
verification exercises both driving modes and checks the pressure reference,
analytical profile and axial force balance.

## Refinement studies

The studies follow [SPEC-VER-003](../design/spec.md#spec-ver-003) and the method of
[blueprint 11.2](../design/blueprint.md#112-refinement-studies). The parabola is outside the
discrete space, because the finite-volume solution is one value per cell. Reproduce every
number in this section with

```sh
cargo test -p caekit --test laminar_channel --locked -- --nocapture --test-threads=1
```

Run on Windows 11 with an Intel Core i7-9750H and 15.8 GB, rustc 1.100.0 and the debug
profile, the eight tests of the file finished in 0.16 s and 0.18 s in two runs with the command
above and in 0.10 s and 0.11 s in two runs without `--nocapture --test-threads=1`, as printed
by the test harness.

### Why the order is 2

On a uniform mesh of `n` cells (`h = H/n`) the interior equations are satisfied exactly by the
cell-centre values of the parabola, because the second difference of a quadratic is exact.
The first cell is not exact. Its centres are at `h/2` and `3h/2`, and the wall conductance
is `2 μ/h`, so the discrete operator applied to the parabola gives `3 g h/4` where the
right-hand side is `g h`, with `g = Δp/L`. The residual `−g h/4` meets the wall stiffness
`2 μ/h` and is absorbed by a velocity offset `g h²/(8 μ)` that is uniform in all cells,
since a constant satisfies every interior equation. The flow response of the unit gradient
is the midpoint sum of the parabola, which exceeds the integral `g H³/(12 μ)` by
`g H h²/(24 μ)` (the midpoint rule error `h² (f'(b) − f'(a))/24`), plus the offset times `H`.
At a prescribed flow the loss is therefore

`Δp_h = Δp / (1 + 2/n²)`, with error `Δp − Δp_h` of magnitude `2 Δp/(n² + 2)`.

This is exact for uniform meshes and gives the formal order `p_f = 2` for the pressure loss.
The test `grid_refinement_recovers_second_order_pressure_loss` compares it with the solver
(`Δp = 2.4 Pa`):

| cells | measured error (Pa) | `2 Δp/(n² + 2)` (Pa) | relative difference |
| ---: | ---: | ---: | ---: |
| 8 | 7.272727272727e-2 | 7.272727272727e-2 | 1.259e-14 |
| 16 | 1.860465116279e-2 | 1.860465116279e-2 | 9.250e-14 |
| 32 | 4.678362573103e-3 | 4.678362573099e-3 | 7.308e-13 |
| 64 | 1.171303074669e-3 | 1.171303074671e-3 | 1.573e-12 |

The test requires a relative difference of 1e-9. It also keeps the earlier requirement that
neighbouring uniform errors have a ratio between 3.8 and 4.1.

On a graded mesh the face terms `μ u'' (h_{i+1} − h_i)/4` of the secant slopes add to the
wall residual. They are `O(h²)` per face when the cell sizes follow a smooth mapping, so the
order is expected to stay 2 (supraconvergence of the scheme); the fits below are the test of
that expectation.

### Levels and mesh

The vertices are `H φ(i/n)` with `φ(s) = s + c (sin(2π s − θ) + sin θ)/(2π)`, `c = 0.6` and
`θ = 0.7`. Then `φ(0) = 0`, `φ(1) = 1` and `φ' = 1 + c cos(2π s − θ)` runs from 0.4 to 1.6, so
cell sizes differ by a factor of 4 across the gap and the mesh is not symmetric. All levels use
the same mapping, so the family is self-similar with the ratio `r = 2` between levels of
16, 32, 64 and 128 cells. The nominal spacing `H/n` enters the fit, because only ratios of
spacings matter.

Two levels are left out of the study, and the test `levels_outside_the_study_are_measured`
measures both:

- The 8-cell level is pre-asymptotic: its pairwise order is 1.931316, 0.068684 below 2. The
  band rule fits the finest three levels only, so adding it changes nothing in the verdict. The
  test runs the pressure-loss study on 8 to 128 cells and requires the fitted order, the spread,
  the band half-width and the shrink factor to equal those of the four-level study bit for bit;
  the pair from 8 to 16 cells is 3.8789 times farther from 2 than the next pair. The study
  starts at 16 cells because four levels is the fewest that SPEC-VER-003 and `band_check` accept
  and the extra level adds no evidence to the rule.
- The 256-cell level converges at the relative tolerance 1e-10 but ends with
  `NumericalBreakdown` at 1e-12, the rounding floor of the algebraic check
  ([blueprint 10.2](../design/blueprint.md#102-algebraic-error)). The check solve of this level
  is impossible, so the study stops at 128 cells.

### Pressure loss

Volume-flow reference model on the graded mesh. The error is `|2.4 Pa − Δp_h|`, an integral
quantity (the inverse of the flow integral). The exact value is the plane Poiseuille loss
`12 μ L Q/(width H³)`.

| cells | error (Pa) | error at tolerance/100 (Pa) | relative change | pairwise order |
| ---: | ---: | ---: | ---: | ---: |
| 16 | 2.840533e-2 | 2.840533e-2 | 0.000e0 | |
| 32 | 7.189029e-3 | 7.189029e-3 | 0.000e0 | 1.982293 |
| 64 | 1.802824e-3 | 1.802824e-3 | 0.000e0 | 1.995538 |
| 128 | 4.510553e-4 | 4.510553e-4 | 9.846e-13 | 1.998882 |

Least-squares fitted order of the finest three levels (32, 64 and 128 cells): 1.997210.

### Velocity in the L² norm

Pressure-driven reference model on the same graded meshes. The error is
`sqrt(Σ h_i (ū_i − u_i)²)` in m/s, with `ū_i` the mean over cell `i` of the exact
`u(y) = (Δp/(2 μ L)) y (H − y)` and `u_i` the solved cell value. It is an integral norm of the
difference between the solution and the cell-mean projection of the exact solution. The full
`L²` distance to the exact function would be dominated by the first-order piecewise-constant
representation and would not test the scheme.

| cells | error (m/s) | error at tolerance/100 (m/s) | relative change | pairwise order |
| ---: | ---: | ---: | ---: | ---: |
| 16 | 1.356276e-5 | 1.356276e-5 | 0.000e0 | |
| 32 | 3.402962e-6 | 3.402962e-6 | 0.000e0 | 1.994788 |
| 64 | 8.515105e-7 | 8.515105e-7 | 0.000e0 | 1.998695 |
| 128 | 2.129258e-7 | 2.129258e-7 | 6.216e-15 | 1.999674 |

Least-squares fitted order of the finest three levels (32, 64 and 128 cells): 1.999184.

### Band

Both studies apply the one band rule of
[blueprint 11.2](../design/blueprint.md#112-refinement-studies) (ruling
[R-9](../design/blueprint.md#171-rulings)), implemented and derived in the rustdoc of
`caekit_numerics::verify::band_check`. The test calls that function and adds no rule of its own:

- the order is fitted by least squares over the finest three levels (32, 64 and 128 cells);
- the band is `[p_f − δ, p_f + δ]` with `p_f = 2` from the derivation above and
  `δ = 0.01 + 1.5 s`, where `s` is the difference between the two pairwise orders of the finest
  three levels and 0.01 is the order shift `Δp` that
  [SPEC-VER-003](../design/spec.md#spec-ver-003) allows;
- the band holds when the distances of the pairwise orders to `p_f` shrink by `ρ ≥ 2` per
  level (or the finest distance is at most `Δp`), and the test checks that condition.

| quantity | `s` | `δ` | band | fitted order | `ρ` | finest distance |
| --- | ---: | ---: | --- | ---: | ---: | ---: |
| pressure loss | 0.003344 | 0.015016 | 1.984984 to 2.015016 | 1.997210 | 3.9921 | 0.001118 |
| velocity, L² | 0.000979 | 0.011468 | 1.988532 to 2.011468 | 1.999184 | 3.9984 | 0.000326 |

Both fitted orders lie inside their bands and both shrink factors are above 2; the factor is
close to `r² = 4`, the shrink of a leading correction of order `h²`. The pairwise orders
approach 2 from below, so the data fit the assumption of the derivation. The `ρ` and the finest
distance are printed by the same command as the table.

The study keeps its levels: the rule applies to 16 to 128 cells and the fit lies inside the
band without any change of the level set.

### Algebraic error

Each level is solved again with the relative residual tolerance divided by 100 (1e-10 to
1e-12). The budget for the relative change of the error norm is `δ = Δp ln r/2`, which is
3.4657e-3 at `r = 2`. The largest measured changes are 9.846e-13 for the pressure loss and
6.216e-15 for the velocity, both at 128 cells. The other levels give identical errors at both
tolerances. Every change is far below the budget.

### Trap checks

The test `the_study_fails_for_quantities_the_scheme_reproduces_exactly` runs the same study
code on quantities without a discretization error and requires it to fail.

- Removing the source is not possible in this kind. The driving pressure gradient is the only
  source, and a drive without a positive pressure loss is refused with
  `physics.drive.pressure_drop_pa: must be positive and finite`. A zero gradient would leave the
  zero solution, which every mesh represents exactly. The exactness case is taken instead from
  quantities that the scheme reproduces by construction.
- The volume flow rate at a prescribed flow equals the prescribed rate, because the scheme scales
  the velocity by the flow integral. Its error is exactly 0 at every level, and the fit refuses
  it (`error 0 must be positive and finite`).
- The axial momentum imbalance closes to rounding. Its values at 16, 32, 64 and 128 cells are
  1.951564e-18, 1.344411e-17, 1.105886e-17 and 1.459336e-16 N, with a fitted order of
  -1.720133 and a pairwise spread of 4.003808, so the band is 2 ± 6.015712, wide enough to
  contain the fitted order. The rule rejects the study by its shrink condition: the distances
  of the pairwise orders to 2 shrink by 0.3003 per level, below 2 (the message
  `distances to the formal order shrink by 0.30028273597204463 per level, below 2`). The test
  requires that `passes()` is false. The band alone would not reject this case, which is why the
  rule checks the shrink factor as well.

```sh
cargo run --locked -- run examples/laminar_channel.json
cargo test --locked --test laminar_channel
```
