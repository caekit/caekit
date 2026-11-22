# Conjugate cooling channel

The `cooling_channel` case couples a constant-property laminar channel and a
conductive solid wall. The flow solve supplies the mass flow to a separate
fluid energy mesh. The wall conducts through its thickness. The stream
transports heat by axial bulk enthalpy advection. The model assumes a uniform
hot-face temperature, constant wall conductivity, constant fluid heat
capacity, negligible axial conduction and a prescribed fluid-side film
coefficient. The film coefficient must come from an appropriate correlation
or measurement in engineering use. This model has no turbulence, phase
change, viscous dissipation, or temperature feedback into the momentum solve.

Each axial wall panel is a one-dimensional conduction column with linear
finite elements through the thickness. For constant conductivity and
source-free steady conduction, eliminating interior temperatures gives the
exact element-chain conductance `k A / thickness`. The normal mesh supplies
positions for the reconstructed wall temperature. The axial wall and coolant
meshes can have different subdivisions of the same physical interval. The
model ties the wetted wall face to one explicitly named hydraulic wall.

The solver intersects the two sets of interface intervals. Each overlap
uses the same area for the heat leaving the solid and entering the fluid.
For area `A_ij`, the contribution is `h A_ij (T_wall,j − T_fluid,i)`.
The fluid energy equation uses first-order upwind finite-volume fluxes.
Partitioned Gauss–Seidel alternates wall conduction with the downstream
fluid energy sweep. It checks the combined L2 norm of the wall-cell and
fluid-cell energy residuals in watts after each sweep. The convergence
threshold is the larger of the absolute tolerance in watts and the
relative tolerance times the initial norm. The result also reports separate
global balances for the solid, the interface, the fluid and the whole system.
Positive interface heat flux denotes heat supplied to the coolant.
Conservative transfer of extensive quantities across non-matching meshes is
treated for fluid-structure loads by Farhat, Lesoinne and Le Tallec (1998),
doi:10.1016/S0045-7825(97)00216-8, and is also described in the
[preCICE mapping documentation](https://precice.org/docs/configuration/basics/mapping/).
The paper treats loads and motion, not heat; the transfer of heat rate here is the same
area-weighted idea applied to an extensive quantity.

An independent reference follows from the continuous thermal circuit.
The area resistance `R'' = thickness/k + 1/h` and energy capacity flow
`C = mass_flow * cp` give
`T_out = T_hot − (T_hot − T_in) exp(−width * length/(R'' C))`.
The relation is the series thermal resistance and the bulk energy balance of a
heated stream (Bergman, Lavine, Incropera and DeWitt, *Fundamentals of Heat and
Mass Transfer*, 8th ed.; see [provenance](../provenance.md)).
The flow and material properties in the example are synthetic. The
reference determines the temperature and the heat duty `C(T_out − T_in)`.
For the inputs of `examples/cooling_channel.json` (`T_hot` = 360 K, `T_in` = 298 K,
thickness 4 mm, `k` = 15 W/(m K), `h` = 400 W/(m² K), `cp` = 4180 J/(kg K),
mass flow 0.01 kg/s, width 0.1 m, length 2 m) the expression gives
`T_out` = 349.0018 K and a duty of 2131.88 W (derived, evaluated in
`node`). The saved model solves to 348.6613 K and 2117.64 W
(`cargo run --locked -- run examples/cooling_channel.json`), 0.34 K below the reference.
The 31-wall/48-fluid-panel example accepts 0.5 K temperature error and
requires each reported energy balance to close to within 1e-6 W.
A matching-grid test doubles the number of panels from 12 to 96 and verifies
the first-order decrease in the outlet-temperature error. The
[order study](#order-study) below fits the order of the whole coolant
temperature profile. These are numerical checks against the reduced equations.

```sh
cargo run --locked -- check examples/cooling_channel.json
cargo run --locked -- run examples/cooling_channel.json
cargo test --locked --test cooling_channel
```

## Order study

The study checks that the coolant temperature converges at the formal order of
the scheme, and that it would fail where the scheme is exact. It is the test
`thermal_mesh_refinement_recovers_first_order_enthalpy_advection`
([SPEC-VER-002](../design/spec.md#spec-ver-002),
[SPEC-VER-003](../design/spec.md#spec-ver-003)). The method is
[blueprint section 11.2](../design/blueprint.md#112-refinement-studies).

**Exact solution.** With negligible axial conduction the heat flux from the hot
face into the coolant at `x` is `(T_hot − T(x))/R''`, with the area resistance
`R'' = thickness/k + 1/h` of the wall in series with the film. The energy
balance of the stream, `C dT/dx = (width/R'') (T_hot − T)` with `T(0) = T_in`,
has the solution

`T(x) = T_hot − (T_hot − T_in) exp(−m x)`, `m = width/(R'' C)`.

For the example, `R'' = 0.004/15 + 1/400 m²K/W`, `C = 0.01 · 4180 W/K` and
`m L = 1.729`. The profile is not piecewise constant, so no mesh of the scheme
represents it. The reference is this continuous circuit, not the energy balance
of the discrete system.

**Formal order.** The coolant unknowns are cell values and the energy flux is
first-order upwind: the equation of cell `i` is the implicit-Euler form
`C (T_i − T_{i−1}) = Σ_j h A_ij (T_wall,j − T_i)`, whose truncation error is
`O(h)`. The wall panels see the piecewise-constant coolant temperature through
the interval overlaps, which is also `O(h)`. The error of the piecewise-constant
profile in the L2 norm is therefore of order 1, and the expected order is
`p_f = 1`.

**Norm and meshes.** The measure is the RMS error of the piecewise-constant
coolant temperature over the channel, `sqrt((1/L) ∫ (T_h − T)² dx)` in K,
integrated cell by cell with a 5-point Gauss–Legendre rule (exact to degree 9,
far below the discretization error on every level). It is an integral norm, not
a value at the outlet or at a cell centre. The meshes are nonuniform and
nonmatching. Level `k` has `8 · 2^k` wall panels placed at
`L (ξ + 0.4 sin(2πξ)/(2π))` and `12 · 2^k` coolant cells placed at
`L (ξ + 0.6 ξ (1 − ξ))`, with `ξ = i/n`. Each is a fixed smooth map of `ξ`, so
the family is self-similar: cell sizes follow the same distribution on every
level (the map derivative lies in `[0.6, 1.4]` for the wall and `[0.4, 1.6]` for
the coolant, so the ratio of the largest to the smallest cell tends to 2.3 and
4) and the refinement ratio is `r = 2`. The nominal spacing of level `k` is
`2^−k`; only ratios of spacings enter the fit.

**Fit and band.** The study applies the band rule of
[blueprint § 11.2](../design/blueprint.md#112-refinement-studies) through
`caekit_numerics::verify::band_check`, whose rustdoc derives it. The order is
the least-squares slope of `ln e` against `ln h` over the finest three levels
(levels 4 to 6 of the table, 192 to 768 coolant cells), which at constant ratio
is the mean of the two finest pairwise orders. The band is two-sided,
`p_f ± δ` with `δ = Δp + 1.5 s`:

- `Δp = 0.01` is the share of the algebraic error of
  [SPEC-VER-003](../design/spec.md#spec-ver-003), checked below.
- `s` is the measured spread of the two finest pairwise orders,
  `|0.996768 − 0.998393|`; the command below prints it. The factor 1.5 covers
  the fit when the distances of the pairwise orders from `p_f` shrink by
  `ρ ≥ 2` per level, as a leading correction of order `h¹` or higher does at
  `r = 2`.

The test requires `ρ ≥ 2` unless the finest distance is already within `Δp`. A
super-optimal order is outside the band and is a defect. No bound on `s` is
stated in advance for this study, and the band is never chosen to make an order
pass.

The pairwise orders approach 1 from below. The distances `1 − p̂` of the table,
0.052792, 0.026190, 0.013008, 0.006440, 0.003232 and 0.001607 (computed from
the printed orders), shrink by 2.0157, 2.0134, 2.0199, 1.9926 and 2.0112 per
level: about the factor 2 of a correction of order `h¹` to the error
`e = C₁h(1 + a h)`, with one pair 0.4 % below it. That is the measured
behaviour; it was not derived further, and the rule looks only at the finest pair.
The ratio of the finest two is the `ρ` of the rule, 2.0107 (from the unrounded
orders), and the finest distance is 0.001607, within `Δp`, so the check does not
depend on `ρ`. The two coarsest levels (12 and 24 coolant cells) are
pre-asymptotic: a fit over all seven levels gives 0.984750, which the finest
three levels do not use. It is printed for comparison only.

**Algebraic error.** Each level is solved a second time with both coupling
tolerances divided by 100 (relative `1e-10 → 1e-12`, absolute `1e-10 W →
1e-12 W`; the first solve uses the defaults of
[model.md](../model.md#numerical-stages)). The error norm may change by a
relative `δ = Δp ln r/2 = 0.01 · ln 2/2 = 3.465736e-3` at most
(`caekit_numerics::verify::algebraic_check`), the value for which the algebraic
error moves the observed order by at most 0.01. The largest change measured is
`3.686e-9`, about `9.4e5` times below the budget. The partitioned iteration
needs 7 or 8 sweeps at the default tolerance and 9 at the tight one.

**Measured.** From the command below (`Wall` and `Coolant` are cell counts):

| Level | Wall | Coolant | h | Sweeps | RMS error (K) | Pairwise order | RMS error, tight (K) | Relative change |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | 8 | 12 | 1 | 8 | 2.879807 | | 2.879807 | 1.035e-11 |
| 1 | 16 | 24 | 1/2 | 8 | 1.493569 | 0.947208 | 1.493569 | 7.393e-12 |
| 2 | 32 | 48 | 1/4 | 8 | 7.604652e-1 | 0.973810 | 7.604652e-1 | 7.895e-12 |
| 3 | 64 | 96 | 1/8 | 8 | 3.836764e-1 | 0.986992 | 3.836764e-1 | 1.123e-11 |
| 4 | 128 | 192 | 1/16 | 7 | 1.926965e-1 | 0.993560 | 1.926965e-1 | 1.019e-9 |
| 5 | 256 | 384 | 1/32 | 7 | 9.656432e-2 | 0.996768 | 9.656432e-2 | 1.906e-9 |
| 6 | 512 | 768 | 1/64 | 7 | 4.833598e-2 | 0.998393 | 4.833598e-2 | 3.686e-9 |

The values of the band rule, from the same command:

| Quantity | Value |
| --- | --- |
| Fitted order over the finest three levels | **0.997581** |
| Spread `s` of the finest two pairwise orders | 0.001624 |
| Half-width `δ = 0.01 + 1.5 s` | 0.012437 |
| Band `1 ± δ` | [0.987563, 1.012437] |
| Shrink factor `ρ` | 2.0107 |
| Finest distance `1 − p̂` | 0.001607 |

The fitted order lies 0.002419 from the formal order, inside the band, and
`ρ ≥ 2`, so the study passes the rule. Over all seven levels the order is
0.984750, for comparison only. The errors decrease at every level, so the study
measures discretization error.

**Trap check.** The test
`the_order_study_rejects_a_solution_the_scheme_represents_exactly` runs the same
meshes with the hot face at the inlet temperature, 298 K. The coolant then stays
at 298 K, a constant that a piecewise-constant space represents exactly, so the
error is 0 on all seven levels and there is nothing to fit. The study returns an
error (`NotPositive { quantity: "error", index: 0 }`) instead of an order, so it
fails. A cooling channel has no source term; the exchange with the wall is what
makes the solution non-constant, and removing its driving temperature difference
is the matching exactness case. The test also shows that the band is two-sided:
errors that fall as `h²` fit an order of 2.000000 and errors that do not fall fit
0.000000. The rule rejects both, with the half-width 0.010000 of zero spread
(`fitted order 2 outside 1 +- 0.01` and `fitted order 0 outside 1 +- 0.01`).

**Matching uniform meshes.** The test
`outlet_temperature_error_halves_on_matching_uniform_meshes` keeps the earlier
check of the outlet temperature on 12, 24, 48 and 96 matching uniform panels:
the error ratios are 1.967187, 1.983352 and 1.991616, required to lie in
`[1.9, 2.1)`. This is a point value on uniform meshes, so it is a consistency
check; the order claim rests on the study above.

Reproduce every number of this section, with the printed table:

```sh
cargo test -p caekit --test cooling_channel --locked -- --nocapture --test-threads=1 outlet_temperature thermal_mesh the_order_study
```

Measured on Windows 11 with an Intel Core i7-9750H and 15.8 GB, rustc 1.100.0,
debug profile, after the build, as printed by the test harness: the three tests of the
command above finished in 0.09 s, and all seven tests of the file (`cargo test -p
caekit --test cooling_channel --locked`) in 0.05 s. Repeated runs vary by a few
hundredths of a second.
