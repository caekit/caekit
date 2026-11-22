# Axial bar elasticity

The `axial_bar` case models static small-strain axial deformation of a straight
bar with uniform area and a positive constant Young's modulus. It assumes
elastic behaviour, axial loading and zero body force. Each endpoint requires
an axial force or a displacement in the positive local axis direction. A free
endpoint uses a force of zero. At least one displacement fixes rigid translation.

The equilibrium equation is `d(E A du/dx)/dx = 0`. Each linear finite element
of length `h` forms the matrix `(EA/h) [[1, -1], [-1, 1]]`. Displacement
conditions are eliminated symmetrically. The reduced sparse system is solved
with Jacobi-preconditioned conjugate gradient in binary64. The element stress
is `E (u_right − u_left)/h`. The constrained endpoint reaction uses the
outward face sign and has units of newtons.

The analytic case has a 1 m length, 100 mm² area, modulus 200 GPa and a 1 kN
tensile load, the inputs of `examples/axial_bar.json`. The independent expression
`F L/(E A) = 1000 N · 1 m/(200e9 Pa · 1e-4 m²) = 5e-5 m` gives 0.05 mm extension.
The stress `F/A = 1000 N/1e-4 m² = 1e7 Pa` is 10 MPa, and the clamp reaction is
−1 kN by the force balance of the bar. These values are derived from the inputs;
the run of the example below measures 5.000000000000001e-5 m, 1e7 Pa in the first
element and −1000 N (`cargo run --locked -- run examples/axial_bar.json`). The exact displacement
is linear, so its spatial approximation is exact on any conforming mesh in
this case. The test `solves_analytic_tension_case` uses the unequal element lengths
of the example (the mesh vertices are at 0, 0.125, 0.375, 0.625, 0.875 and 1 m)
and limits the extension error to 1e-13 m, stress error to 1e-3 Pa and reaction
error to 1e-7 N. These bounds test the arithmetic and assembly against closed-form values.

```sh
cargo run --locked -- check examples/axial_bar.json
cargo run --locked -- run examples/axial_bar.json
cargo test --locked --test axial_bar
```
