# Steady three-dimensional solid heat conduction

The `heat_conduction` case solves `−∇·(k∇T) = Q` on one connected
tetrahedral solid with constant isotropic conductivity. Use global coordinates
in metres, absolute temperatures in kelvin and conductivity `k > 0` in
W/(m·K). The optional constant `volumetric_heat_source_w_m3` defaults to zero.
Positive `Q` generates heat and negative `Q` removes it. This analysis uses
steady conduction in a single homogeneous solid. The cooling-channel model
has its own wall and coolant formulation.
[Multimaterial heat conduction with ideal contact](multimaterial-heat-conduction.md)

```sh
cargo run --locked -- check examples/heat_conduction.json
cargo run --locked -- run examples/heat_conduction.json
cargo test -p caekit --test heat_conduction --locked -- --nocapture
```

Set `physics.kind` to `heat_conduction` and supply `physics.geometry`,
`physics.mesh` and `physics.thermal`. The mesh uses the same positively
oriented tetrahedra and named exterior triangles as the elastic solid. The
`thermal.material` object holds `id`, `thermal_conductivity_w_m_k` and
`source` with a citation and usage terms. [Tetrahedral mesh format](elastic-solid.md)

## Boundary conditions

Assign at most one condition to each named region in `thermal.boundary_conditions`.
A region without an assignment is insulated.

| `kind` | Additional fields | Physical meaning |
| --- | --- | --- |
| `temperature` | `temperature_k` | Fix the surface temperature in K |
| `heat_flux` | `inward_heat_flux_w_m2` | Uniform flux in W/m², positive into the solid |
| `insulation` | None | Zero normal heat flux |
| `convection` | `ambient_temperature_k`, `film_coefficient_w_m2_k` | Inward heat flux `h(T_ambient − T_surface)`, with positive `h` in W/(m²·K) |

With outward unit normal `n` and physical flux vector `q = −k∇T`, the
inward flux is `−q·n`. Prescribed and ambient temperatures must be positive
and all inputs finite. Adjacent fixed-temperature surfaces must agree at each
shared vertex. Flux and convection are integrated over their own triangles
and may meet other conditions along edges. Validation requires a prescribed
temperature or a positive convection conductance on a finite-area surface
to fix the absolute temperature. A solved temperature at or below absolute
zero is a calculation error.

Linear temperature shape functions give a constant gradient in each
tetrahedron. The weak form adds `∫ h T N_i dA` on the left and
`∫ h T_ambient N_i dA` on the right for convection. CAEKit integrates the
linear triangle mass matrix with its off-diagonal terms and splits a
constant cell source among the four vertices. Conjugate gradient solves
the sparse system after symmetric elimination of prescribed temperatures.
The solved unknowns are offsets from a prescribed temperature. When convection
provides the only temperature reference, the solver instead uses the film-
conductance-weighted mean surface temperature implied by the global heat balance.
This keeps the small internal gradients resolvable under weak convection.
[MFEM boundary-condition formulation](https://mfem.org/fem_bc/)

## Read the report

| Output | Sampling | Unit |
| --- | --- | --- |
| `solid_temperature` | Mesh vertices in input order | K |
| `solid_heat_flux` | Tetrahedron centroids in cell order, three global components | W/m² |
| `boundary_heat_rate` | One integrated power per named exterior region, positive inward | W |

The `solid_energy` balance adds volume generation, prescribed inward flux,
convection and prescribed-temperature reaction power. At a fixed-temperature
vertex the reaction is the power needed to satisfy the nodal heat equation.
The global balance counts it once. If several prescribed-temperature regions
meet at that vertex, the reported regional contributions split the reaction
in proportion to their incident triangle areas. The reported imbalance is
the signed sum in W. The `solid_heat_conduction` trace reports the true
algebraic residual of the free equations and its tolerance in W.

## Analytical checks

Every value below is derived from the inputs of the case. The tests
`linear_temperature_profile_flux_and_reactions`,
`prescribed_flux_and_convection_set_the_temperature_without_dirichlet` and
`uniform_source_converges_inside_elements_and_leaves_the_solid` compare the solver with
them, and the command at the top of this page runs them.

- **Linear profile.** A 0.1 m block with 0.01 m² cross section and
  `k = 20 W/(m·K)` has end temperatures of 400 K and 300 K and insulated
  sides. The internal vertex is at 350 K, the flux vector is
  `[20000, 0, 0]` W/m² and the boundaries carry +200 W and −200 W.
- **Prescribed flux and convection.** An inward flux of 1,000 W/m² at one
  end of the same block gives 10 W into the solid. The other end exchanges
  heat with a 300 K ambient and `h = 100 W/(m²·K)`. The film temperature
  rise is 10 K and the conductive temperature drop is 5 K, giving 315 K
  on the heated face, 310 K on the cooled face and −10 W by convection.
- **Volumetric source.** With ends at `T0` and `T1` the analytical
  temperature is `T(x) = T0 + (T1 − T0) x/L + Q x (L − x)/(2k)` and the flux
  is `q_x = k (T0 − T1)/L + Q (x − L/2)`, with `q_y = q_z = 0`. The block
  generates `Q L³ = 200 W` at `Q = 200,000 W/m³`. The test uses
  `T0 = 400 K` and `T1 = 300 K`.

The first two solutions are affine, so the tests allow absolute errors of
1e-8 K, 1e-6 W/m² and 1e-8 W for binary64 arithmetic and the linear solve.
The 2e-6 W balance allowance of the source case is 1e-8 of the 200 W source.
The material values and block are synthetic analytical inputs under the
project license. [Provenance](../provenance.md)

## Refinement study

The source case is the order study of this kind
([SPEC-VER-002](../design/spec.md#spec-ver-002),
[SPEC-VER-003](../design/spec.md#spec-ver-003)). Reproduce every number of
this section, tables and orders included, with

```sh
cargo test -p caekit --test heat_conduction --locked uniform_source_converges_inside_elements_and_leaves_the_solid -- --nocapture
cargo test -p caekit --test heat_conduction --locked the_source_study_rejects_the_exact_linear_solution -- --nocapture
```

**Why it can fail.** The quadratic term of `T` is outside the space of
piecewise-linear functions, so the discretization error is real. The exact
linear profile (no source) is inside that space and gives no order, which is
the trap below. Both norms are integral norms of the error over the volume:

- the L² norm `‖T − T_h‖`, in K·m^(3/2), expected order `p = 2` (Ciarlet 2002,
  Aubin–Nitsche argument on the convex block);
- the energy norm `(∫ k |∇(T − T_h)|² dV)^(1/2) = (∫ |q − q_h|²/k dV)^(1/2)`, in
  √(W·K), expected order `p = 1` (Ciarlet 2002). `q_h` is the reported cell flux.

**Levels.** Four levels of `n = 2, 4, 8, 16` divisions per axis, `h = L/n`,
refinement ratio `r = 2`, with `6n³` tetrahedra (48 to 24,576). The vertices
are moved so that no level is a uniform structured mesh: each coordinate of a
vertex that is not on the block surface plane of that axis moves by
`0.15 h` times a deterministic splitmix64 value in `[−1, 1)`. A vertex on a
face slides inside the face and an edge vertex along the edge, so the surface
stays planar and the named regions keep their meaning. The solver rejects a
cell of non-positive volume, so a passing run also shows that every cell stayed
positive.

**Quadrature.** Each cell integral uses the collapsed four-point
Gauss–Legendre rule per axis, exact to degree seven, the `2p + 3` of the
blueprint at `p = 2`. The integrands are polynomials of degree four at most.

**Fit and band.** The study applies the band rule of
[blueprint § 11.2](../design/blueprint.md#112-refinement-studies) through
`caekit_numerics::verify::band_check`, whose rustdoc derives it. The order
is the least-squares slope of `ln e` against `ln h` over the finest three
levels, which at constant ratio is the mean of the two finest pairwise
orders; the coarsest level enters only the first pairwise order. The band is
two-sided, `p_f ± δ` with `δ = Δp + 1.5 s`: `Δp = 0.01` is the algebraic
share (derived below) and `s` the measured spread, the absolute difference of
the two finest pairwise orders. The factor 1.5 covers the fit when the
distances of the pairwise orders from `p_f` shrink by `ρ ≥ 2` per level. The
test requires `ρ ≥ 2` unless the finest distance is already within `Δp`. A
band is never chosen to make an order pass, and a super-optimal order is
outside the band as a defect.

**Algebraic error.** Every level is solved a second time with the relative
residual tolerance divided by 100 (1e-10 to 1e-12). The relative change of
the measured error norm must stay at or below `δ = Δp ln r / 2`. A relative
perturbation `±δ` of the error of both levels shifts the observed order by at
most `ln((1 + δ)/(1 − δ))/ln r ≈ 2δ/ln r`, so `|Δp̂| ≤ 0.01` needs
`δ ≤ 0.01 ln 2 / 2 = 3.4657e-3` at `r = 2`
(`caekit_numerics::verify::algebraic_error_budget`).

**Measured errors.**

| n | h (m) | L² error (K·m^(3/2)) | Energy error (√(W·K)) |
| ---: | ---: | ---: | ---: |
| 2 | 0.050000 | 7.238469e-2 | 2.036400e1 |
| 4 | 0.025000 | 1.961678e-2 | 1.060330e1 |
| 8 | 0.012500 | 5.067975e-3 | 5.399077e0 |
| 16 | 0.006250 | 1.269206e-3 | 2.702937e0 |

| Norm | Pairwise orders (2→4, 4→8, 8→16) | Spread `s` | Half-width `δ` | Band | Fitted order (finest three) | Shrink factor `ρ` |
| --- | --- | ---: | ---: | --- | ---: | ---: |
| L² | 1.883597, 1.952607, 1.997483 | 0.044876 | 0.077314 | [1.922686, 2.077314] | 1.975045 | 18.83 |
| Energy | 0.941508, 0.973729, 0.998185 | 0.024456 | 0.046684 | [0.953316, 1.046684] | 0.985957 | 14.47 |

The pairwise orders approach the formal order from below as the mesh
refines, and their distances from it shrink by a factor 18.83 (L²) and 14.47
(energy) per level, far above the required 2. The fitted orders lie 0.0524
(L²) and 0.0326 (energy) inside the lower edge of their bands. The finest
distances `p_f − p̂₃` are 0.0025 and 0.0018, within `Δp`, so the band
holds for any shrink rate and the study also meets the rate condition.

The relative change of the error norm under the tighter solve is below the
budget `δ = 3.4657e-3` by seven orders of magnitude or more:

| n | L² relative change | Energy relative change |
| ---: | ---: | ---: |
| 2 | 0 | 0 |
| 4 | 1.183e-11 | 0 |
| 8 | 7.617e-11 | 3.290e-16 |
| 16 | 1.268e-10 | 0 |

The same run keeps the earlier balance checks on every level: the generation
term and the outward heat rate equal `Q L³` and the imbalance vanishes, each
within 2e-6 W. The tests replace the earlier factors above 2.7 and 1.65 per
halving, which bounded the ratio from one side only, with the band above.

**Trap check.** With `Q = 0` the exact solution is the linear profile, which
linear tetrahedra reproduce, so the error is not discretization error. The
same study, judged by the same rule (its band comes from its own spread),
fails:

| n | L² error (K·m^(3/2)) | Energy error (√(W·K)) |
| ---: | ---: | ---: |
| 2 | 1.334752e-15 | 5.880185e-14 |
| 4 | 1.074294e-11 | 5.147462e-9 |
| 8 | 3.529536e-11 | 1.897982e-8 |
| 16 | 7.820223e-11 | 3.971149e-8 |

The errors are rounding and solver noise that grows with the number of
unknowns. The fitted orders over the finest three levels are −1.4319 (L²) and
−1.4738 (energy). The noise makes the spread large, so the bands are wide,
`2 ± 0.8625` and `1 ± 1.2362`, and the fits are still outside them. The error
sequence is classified divergent, and at 4, 8 and 16 divisions the error norm
changes by relative changes of 67.8 to 156.3 under the tighter solve, far
above `δ`. The test
`the_source_study_rejects_the_exact_linear_solution` requires the band to
reject both norms.
