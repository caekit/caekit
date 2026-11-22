# Three-dimensional linear elasticity

The `elastic_solid` case solves equilibrium of a single connected isotropic
elastic solid under small strain and displacement. Supply an elastic modulus,
a Poisson ratio strictly between −1 and 0.5 and a tetrahedral mesh in global
Cartesian coordinates. The solver uses a displacement formulation with
constant strain per tetrahedron. Use it for linear elastic response. Contact,
large deformation, material nonlinearity and dynamics require different
formulations. Linear tetrahedra can be too stiff in bending or near the
incompressible limit. Refine the mesh and check the engineering output.

The mesh uses zero-based vertex indexes and a positive determinant of the
three edges from the first node. Each exterior triangle must belong to one
named region. The order of triangle vertices is arbitrary. Outward normals
are oriented from the adjacent volume cell. Mesh checks report the smallest
and largest cell edges and the tetrahedron with the lowest mean-ratio
quality. The quality is the mean ratio `12 (3V)^(2/3) / Σ ℓ²`, with `V` the volume and
`ℓ` the six edge lengths, of Liu and Joe (1994), doi:10.1007/BF01955874; it is one for
a regular tetrahedron and tends to zero under degeneration. (For edge `a` the
volume is `a³/(6√2)`, so `(3V)^(2/3) = a²/2` and the ratio is `12 (a²/2)/(6 a²) = 1`.
[Provenance](../provenance.md) records how the citation was checked.) Topology checks require a face-connected mesh and detect
inverted cells, repeated cells, nonmanifold faces, invalid regions and
vertices excluded from the volume mesh. They assume a conforming,
non-self-intersecting volume.

Specify displacement components in metres or leave selected components free
with JSON `null`. Overlapping assignments at shared vertices must be
consistent. Displacement constraints must restrain translation and rotation.
Traction vectors are in pascals. Pressure traction is
`−(p_applied − p_ambient) * outward_normal` with explicit absolute or gauge
pressure input as in the laminar channel. Tractions and pressures add.
Unloaded components use the natural zero-traction condition. A constant
body force can be specified in N/m³.

The weak form integrates `ε(v):σ(u)` over the domain. Stress is
`σ = λ tr(ε) I + 2 μ ε` and `ε = (grad(u) + grad(u)ᵀ)/2`.
Linear tetrahedral shape functions give constant gradients per cell.
Three-dimensional displacements are assembled into a sparse system with
symmetric elimination of prescribed components. Surface and body loads
are integrated with consistent basis weights. Constrained reactions are
recovered by subtracting applied forces from the internal forces. Reports
give displacement and reaction vectors at vertices, stress tensors and
von Mises scalars at cell centroids, strain energy and force and moment
balances. Moments use the first mesh vertex as the reference point.

The same constraints, loads and elastic material are available for
temperature-driven deformation. [Thermoelasticity](thermoelastic-solid.md)

The reference block is 2 m by 0.5 m by 0.4 m. Its 10 MPa applied traction
and 200 GPa modulus give `σ L/E = 1e7 Pa · 2 m/2e11 Pa` = 1e-4 m of extension independent
of the solver. Analytical transverse strains are −0.31 times the axial strain (the
Poisson ratio of `examples/elastic_solid.json`), support
reaction is `−σ A = −1e7 Pa · 0.5 m · 0.4 m` = −2 MN and strain energy is
`F u/2 = 2e6 N · 1e-4 m/2` = 100 J (all derived). The run of the saved model
measures 9.999999999999988e-5 m, −1999999.9999999974 N and 99.99999999999973 J
(`cargo run --locked -- run examples/elastic_solid.json`; the native and WASI
reference check prints the same reaction and energy). The affine displacement and
constant stress form an exact patch test for linear elements, after Taylor, Simo,
Zienkiewicz and Chan (1986), doi:10.1002/nme.1620220105, so it cannot
show an order defect; the refinement study below does. The test
also reverses the load through the pressure boundary, checks independent
force and moment integrals for a body load and rejects invalid
assignments and missing rigid support.

```sh
cargo run --locked -- check examples/elastic_solid.json
cargo run --locked -- run examples/elastic_solid.json
cargo test --locked --test elastic_solid
```

## Refinement study

The Lamé thick cylinder is the order study of this kind
([SPEC-VER-002](../design/spec.md#spec-ver-002),
[SPEC-VER-003](../design/spec.md#spec-ver-003)). It uses schema version 1
inputs only: one pressure region and zero displacement components, no
solver change. Reproduce every number of this section, tables and orders
included, with

```sh
cargo test -p caekit --test elastic_solid --locked lame_thick_cylinder_converges_at_the_formal_orders_in_l2_and_energy -- --nocapture
cargo test -p caekit --test elastic_solid --locked the_order_study_rejects_the_affine_cylinder -- --nocapture
```

**Problem.** A quarter, `0 ≤ θ ≤ π/2`, of a cylinder with inner radius
`a = 1 m` and outer radius `b = 2 m`, `E = 200 GPa` and `ν = 0.3`
(synthetic). A gauge pressure `p_i = 10 MPa` acts on the inner surface and
the outer surface is free. The plane `x = 0` has `u_x = 0`, the plane `y = 0`
has `u_y = 0`, and both end faces have `u_z = 0`, which is plane strain.
These conditions restrain the six rigid-body modes.

**Exact solution.** With zero body force and a purely radial displacement
`u = u(r) e_r`, equilibrium reduces to `d/dr [(1/r) d(r u)/dr] = 0`, so
`u = A r + B/r`. The strains are `ε_rr = A − B/r²` and `ε_θθ = A + B/r²`, and
in plane strain the stresses are `σ_rr = 2(λ + μ)A − 2μB/r²`,
`σ_θθ = 2(λ + μ)A + 2μB/r²` and `σ_zz = 2λA`, with no shear. The conditions
`σ_rr(a) = −p_i` and `σ_rr(b) = −p_o` give

```text
A = (p_i a² − p_o b²) / (2 (λ + μ) (b² − a²))
B = (p_i − p_o) a² b² / (2 μ (b² − a²))
```

The study has `p_o = 0`, `λ + μ = E/(2(1 + ν)(1 − 2ν)) = 1.923077e11 Pa` and
`μ = E/(2(1 + ν)) = 7.692308e10 Pa`, so `A = 8.666667e-6` and
`B = 8.666667e-5 m²`. In Cartesian components
`u = (A + B/r²)(x, y, 0)` and `ε_ij = (A + B/r²) δ_ij − 2B x_i x_j / r⁴` in the
plane. The `B/r` part is not a polynomial, so no mesh represents it exactly.
The test checks `σ_rr(a) = −p_i` and `σ_rr(b) = 0` from this strain and
Hooke's law, independently of the formulas for `A` and `B`. The symmetry
and end conditions are those of the exact solution: it has `u_x = 0` and no
shear stress on `x = 0`, `u_y = 0` and no shear stress on `y = 0`, and
`u_z = 0` and no shear stress on the end faces.

**Why it can fail.** Both norms are integral norms of the error over the
meshed volume:

- the relative L² norm `‖u − u_h‖ / ‖u‖`, expected order `p = 2` (Ciarlet
  2002, section 3.2). The symmetry planes make the quarter one reflection
  of the full annulus, a smooth domain, so the regularity that the
  duality argument needs holds;
- the relative energy norm `‖u − u_h‖_E / ‖u‖_E` with
  `‖v‖_E² = ∫ (λ (tr ε(v))² + 2μ ε(v):ε(v)) dV`, expected order `p = 1`
  (Ciarlet 2002, section 3.2). `ε(u_h)` is constant in each cell.

The curved surfaces are meshed as flat facets whose vertices lie on the
circles, so the meshed volume differs from the cylinder by a sliver of
width `O(h²)`. The pressure acts on the facets as `−p_i n_h` with the facet
normal `n_h`. The exact formula for `u` is smooth for `r > 0`, so the norms
integrate it over the meshed volume. The faceting adds a geometric error;
the measured orders below show that it does not lower the orders.

**Levels.** The levels were fixed before the first run: `n = 4, 8, 16, 32`
radial divisions, `2n` circumferential divisions and two layers through the
thickness, ratio `r = 2`, with `24 n²` tetrahedra (384 to 24,576). The
coarsest level has four divisions, not three, because three was flagged as
likely pre-asymptotic. Every hexahedron of the mapped grid is split into six
tetrahedra around its main diagonal, which is conforming. The mesh is
non-uniform and the same at every level up to scale. The radial map
`ξ − 0.3 ξ (1 − ξ)` makes cells near the inner surface 0.7 times and near
the outer surface 1.3 times the mean radial size. The angular map
`η + 0.1 sin(π η)` varies the angular size between `1 − 0.1π` and `1 + 0.1π`
times the mean. The layer thickness is `(b − a)/n`. The middle-plane
vertices are free in `z`, so plane strain is solved, not imposed at every
vertex. The relative norms do not depend on the thickness, because the exact
solution does not depend on `z`.

The first run used `n` circumferential divisions instead of `2n`. Its cells
were 1.57 to 3.14 times longer circumferentially than radially before
grading. The lowest mean-ratio quality was 0.321616 to 0.305582, against
0.602850 to 0.584159 with `2n`. The L² pairwise orders were 1.779275,
1.894238 and 1.959197, against 1.916785, 1.965272 and 1.987057 with `2n`.
The circumferential count was doubled for cell quality. The levels `n` did
not change. Reproduce the first run by setting `ANGULAR_FACTOR` to 1 in
`crates/caekit/tests/elastic_solid.rs`. With that setting the test fails
(`norm 0: pre-asymptotic spread 0.06495896502904497 exceeds the stated 0.025`):
the measured L² spread exceeds the bound that this page states. The band
rule itself accepts that run: its L² fit over the finest three levels,
1.926717, lies inside `2 ± 0.107438`, the band of its own spread, and the
distances shrink by `ρ = 2.5920`. So the change of mesh family narrows the
band but is not what makes the study pass.

**Quadrature.** Each cell integral uses the Grundmann–Möller rule with
`s = 3` (35 points, some weights negative), exact to degree `2s + 1 = 7`,
above the `2p + 3 = 5` of the blueprint at `p = 1` (Grundmann and Möller
1978). The test `grundmann_moeller_rules_integrate_monomials_up_to_their_degree`
checks both rules against `a! b! c!/(a + b + c + 3)!` for every monomial up
to their degree. The integrands are not polynomials, because `u` contains
`B/r`. So every level is integrated again with the degree-5 rule (`s = 2`).
The relative change of each error norm must stay within the algebraic
budget `δ`, because a relative perturbation of an error norm shifts the
observed order exactly as an algebraic one does.

**Fit and band.** The study applies the band rule of
[blueprint § 11.2](../design/blueprint.md#112-refinement-studies) through
`caekit_numerics::verify::band_check`, whose rustdoc derives it. The order
is the least-squares slope of `ln e` against `ln h` over the finest three
levels, which at constant ratio is the mean of the two finest pairwise
orders. The band is two-sided, `p_f ± δ` with `δ = Δp + 1.5 s`: `Δp = 0.01`
is the algebraic share (derived below) and `s` the measured spread of the
two finest pairwise orders. The factor 1.5 covers the fit when the distances
of the pairwise orders from `p_f` shrink by `ρ ≥ 2` per level, as a leading
correction of order `h¹` or higher does at `r = 2`. The test requires
`ρ ≥ 2` unless the finest distance is already within `Δp`. A super-optimal
order is outside the band and is a defect.

This page also states a bound on `s` in advance, the measured spread
rounded up to the next 0.005: 0.025 for L² and 0.015 for energy. The test
fails when the measured spread exceeds it, so the band stays inside the
stated band `p_f ± (Δp + 1.5 · bound)`, `2 ± 0.0475` and `1 ± 0.0325`. The
band is never chosen to make an order pass.

The least-squares slope over all four levels is printed for comparison only:
1.957261 for L² and 0.975130 for energy. The rule fits the finest three
levels because a fit over four levels lies at least `2.095 s` from `p_f`
under the same model (blueprint § 11.2).

**Algebraic error.** Every level is solved a second time with the relative
residual tolerance divided by 100 (1e-10 to 1e-12). The relative change of
the measured error norm must stay at or below `δ = Δp ln r / 2`. A relative
perturbation `±δ` of the error of both levels shifts the observed order by at
most `ln((1 + δ)/(1 − δ))/ln r ≈ 2δ/ln r`. So `|Δp̂| ≤ 0.01` needs
`δ ≤ 0.01 ln 2 / 2 = 3.465736e-3` at `r = 2`
(`caekit_numerics::verify::algebraic_error_budget`).

**Measured errors.**

| n | Cells | Lowest mean ratio | CG iterations (1e-10, 1e-12) | L² relative error | Energy relative error |
| ---: | ---: | ---: | --- | ---: | ---: |
| 4 | 384 | 0.602850 | 81, 90 | 3.259487e-2 | 1.752020e-1 |
| 8 | 1,536 | 0.592787 | 162, 176 | 8.632557e-3 | 9.057990e-2 |
| 16 | 6,144 | 0.587048 | 333, 359 | 2.210720e-3 | 4.592261e-2 |
| 32 | 24,576 | 0.584159 | 684, 738 | 5.576606e-4 | 2.308853e-2 |

| Norm | Pairwise orders (4→8, 8→16, 16→32) | Spread | `ρ` | Band | Stated band | Fitted order (finest three) |
| --- | --- | ---: | ---: | --- | --- | ---: |
| L² | 1.916785, 1.965272, 1.987057 | 0.021785 | 2.6831 | 2 ± 0.042677 | 2 ± 0.0475 | 1.976164 |
| Energy | 0.951756, 0.979986, 0.992029 | 0.012042 | 2.5107 | 1 ± 0.028063 | 1 ± 0.0325 | 0.986007 |

The pairwise orders approach the formal orders from below. Each distance to
the formal order shrinks by a factor of 2.4 to 2.7 per level, between the
factor 2 of a correction of order `h¹` and the factor 4 of one of order `h²`.
The fitted orders lie 0.024 (L²) and 0.014
(energy) from the formal orders, inside half-widths of 0.042677 and 0.028063.

The relative change of the error norm under the tighter solve and under the
degree-5 rule stays below the budget `δ = 3.465736e-3`:

| n | L² algebraic change | Energy algebraic change | L² quadrature change | Energy quadrature change |
| ---: | ---: | ---: | ---: | ---: |
| 4 | 3.946e-12 | 2.693e-15 | 6.197e-6 | 1.236e-6 |
| 8 | 3.265e-12 | 1.079e-13 | 1.267e-6 | 7.677e-8 |
| 16 | 6.021e-11 | 1.330e-14 | 2.944e-7 | 4.747e-9 |
| 32 | 1.378e-10 | 3.546e-14 | 7.183e-8 | 2.948e-10 |

**Trap check.** The elastic solid has no source term. What takes the
solution out of the discrete space is the `B/r` part. With equal pressure
`p_o = p_i` on both surfaces, `B = 0`, and `u = A (x, y, 0)` is affine. The
stress is then constant, and the facet tractions `−p n_h` are exactly
`σ n_h`, so linear tetrahedra reproduce the solution even on the faceted
mesh. The same study on the same levels fails:

| n | L² relative error | Energy relative error |
| ---: | ---: | ---: |
| 4 | 1.329641e-11 | 5.775740e-11 |
| 8 | 6.500436e-12 | 3.262410e-11 |
| 16 | 9.339690e-12 | 4.373733e-11 |
| 32 | 9.976885e-12 | 4.151355e-11 |

The errors are solver and rounding noise. The fitted orders over the finest
three levels are −0.309026 (L²) and −0.173822 (energy), outside the bands
`2 ± 0.651435` and `1 ± 0.757314` of their own spreads. The spreads
(0.427623 and 0.498210) exceed their stated bounds,
and the error norms change by relative amounts of 98.9 to 130.0 under the
tighter solve, far above `δ`. The test
`the_order_study_rejects_the_affine_cylinder` requires the band, the first
check, to reject both norms.
