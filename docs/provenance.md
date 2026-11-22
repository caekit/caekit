# Source and data provenance

- Project license terms are in [LICENSE-MIT](../LICENSE-MIT) and
  [LICENSE-APACHE](../LICENSE-APACHE). The latter is the
  [Apache Software Foundation text](https://www.apache.org/licenses/LICENSE-2.0.txt).
- The Rust dependencies are obtained unmodified from crates.io. The versions
  and checksums are in [Cargo.lock](../Cargo.lock). The direct dependencies are
  [Serde](https://github.com/serde-rs/serde), [Serde JSON](https://github.com/serde-rs/json),
  [Serde Path to Error](https://github.com/dtolnay/path-to-error) and
  [RustCrypto SHA-2](https://github.com/RustCrypto/hashes), each available
  under MIT or Apache-2.0. The locked graph includes the corresponding
  permissive dependencies, including the Unicode-3.0 terms applicable to
  `unicode-ident`. Inspect the exact resolved licenses and upstream
  repositories with `cargo metadata --locked --format-version 1`. `deny.toml` lists the
  licences that CI accepts for the locked graph.
- The Python package links [PyO3](https://github.com/PyO3/pyo3) and the WebAssembly
  package links [wasm-bindgen](https://github.com/wasm-bindgen/wasm-bindgen) with its
  `js-sys` crate, each available under MIT or Apache-2.0. Only the binding crates
  `caekit-py` and `caekit-wasm` depend on them; the solver crates do not.
- Mesh quality uses the MIT-licensed [Rust libm](https://docs.rs/crate/libm/0.2.16)
  cube root, and the solution-verification functions of `caekit-numerics` use its
  logarithm and power. The workspace disables its default features to select the same
  portable implementation for native and WebAssembly builds.
- The axial-bar properties and loads are synthetic inputs specified for the
  analytical fixture. They fall under the project license. The example uses
  a representative constant elastic modulus independent of any measured
  material curve.
- The parallel-plate channel example uses synthetic constant liquid properties
  and geometry under the project license. Its expected pressure loss and
  velocity profile follow from the analytical momentum equation.
- The cooling channel uses synthetic wall conductivity, fluid heat capacity
  and an assumed film coefficient under the project license. The analytic
  reference is a continuous thermal resistance and bulk energy balance.
- The elastic solid example uses a synthetic cuboid patch mesh, constant
  elastic properties and analytical traction and body-load cases under the
  project license.
- The homogeneous, multimaterial and thermoelastic examples use synthetic
  tetrahedral blocks, conductivity, elastic constants and expansion properties
  under the project license. The associated physics pages derive the analytical
  references from constant-property conduction and isotropic elasticity.

## Methods and cited sources

Every numerical method and every physical relation the code relies on has a row below.
The first column is the algorithm identifier of the [stage table](model.md) where one
exists. No source code of any cited work was consulted or copied: the methods were
implemented from the cited descriptions. The only third-party code that executes is the
dependencies listed above, whose licences `deny.toml` checks; the cube root, the logarithm
and the power come from `libm` under the MIT licence.

Property values come from no source: every input of the reference models is synthetic, as
the bullets above state, so the table cites methods only. Relations that a physics page
derives in full, such as the thermal-strain variance integral on the
[thermoelastic page](physics/thermoelastic-solid.md), depend on no entry.

| Method | Where used | Source | Identifier | Verifying test |
| --- | --- | --- | --- | --- |
| `jacobi_preconditioned_conjugate_gradient` | `pcg` in `crates/caekit-numerics/src/lib.rs`; the linear stages of the [stage table](model.md) | Hestenes, Stiefel (1952), conjugate gradients; Saad (2003), preconditioned conjugate gradient with a diagonal (Jacobi) preconditioner | doi:[10.6028/jres.049.044](https://doi.org/10.6028/jres.049.044); doi:[10.1137/1.9780898718003](https://doi.org/10.1137/1.9780898718003) | `crates/caekit-numerics/tests/sparse_system.rs`; every `crates/caekit/tests/<kind>.rs` |
| True residual recomputed at convergence, and stagnation at the rounding floor | `pcg` and `residual_roundoff_l2` in `crates/caekit-numerics/src/lib.rs` | Greenbaum (1997), attainable accuracy of recursively computed residuals; Higham (2002), rounding-error bounds of inner products and matrix-vector products | doi:[10.1137/S0895479895284944](https://doi.org/10.1137/S0895479895284944); doi:[10.1137/1.9780898718027](https://doi.org/10.1137/1.9780898718027) | `unattainable_true_residual_stops_after_repeated_roundoff_limited_corrections` in `sparse_system.rs` |
| Euclidean norm by scaling before squaring | `norm2` in `crates/caekit-numerics/src/fmath.rs` | Blue (1978), overflow-safe Euclidean norm. CAEKit uses a simpler variant with one scale, the largest absolute value | doi:[10.1145/355769.355771](https://doi.org/10.1145/355769.355771) | `scales_before_squaring` and `handles_zero_and_non_finite_values` in `fmath.rs` |
| Symmetric elimination of prescribed degrees of freedom | `ConstrainedSystem` in `crates/caekit-numerics/src/lib.rs` | MFEM documentation of essential boundary conditions imposed by modifying the linear system. The symmetric form is the method stated in the rustdoc of `ConstrainedSystem` | [mfem.org/fem_bc](https://mfem.org/fem_bc/) | `eliminates_prescribed_degrees_of_freedom_and_recovers_the_full_vector` in `sparse_system.rs` |
| Linear (P1) finite elements with constant gradients per cell, and their expected error orders | `crates/caekit-mesh/src/tetra.rs`; the solvers of `caekit-mechanics` and `caekit-thermal` | Ciarlet (2002), shape functions and error estimates of the finite element method | doi:[10.1137/1.9780898719208](https://doi.org/10.1137/1.9780898719208) | the refinement tests of `crates/caekit/tests/heat_conduction.rs`, `multimaterial_heat_conduction.rs`, `thermoelastic_solid.rs` and `laminar_channel.rs` |
| Patch test | The exact affine-displacement check of the elastic solid | Taylor, Simo, Zienkiewicz, Chan (1986) | doi:[10.1002/nme.1620220105](https://doi.org/10.1002/nme.1620220105) | `completes_the_three_dimensional_patch_test` in `crates/caekit/tests/elastic_solid.rs` |
| Mean-ratio tetrahedron quality `12 (3V)^(2/3) / Σ ℓ²`, equal to one for a regular tetrahedron | `MeshQuality` in `crates/caekit-mesh/src/tetra.rs` | Liu, Joe (1994), relationship between tetrahedron shape measures | doi:[10.1007/BF01955874](https://doi.org/10.1007/BF01955874) | `mean_ratio_is_one_for_a_regular_tetrahedron_and_falls_under_flattening` in `tetra.rs`; the `quality.minimum_mean_ratio` range check in `crates/caekit/tests/elastic_solid.rs` |
| Correctly rounded binary64 cube root, the same bits on every platform | `libm::cbrt` in `crates/caekit-mesh/src/tetra.rs` | `libm` 0.2.16 ports the CORE-MATH cube root (MIT licence); Sibidanov, Zimmermann, Glondu (2022) | doi:[10.1109/ARITH54963.2022.00014](https://doi.org/10.1109/ARITH54963.2022.00014) | `portable_cube_root_matches_the_binary64_reference` in `tetra.rs` |
| Isotropic linear elasticity with free thermal strain `ε_th = α (T − T_ref) I` and `σ = C : (ε − ε_th)` | `crates/caekit-mechanics/src/elastic_solid.rs`; the [elastic solid](physics/elastic-solid.md) and [thermoelastic](physics/thermoelastic-solid.md) pages | Hetnarski, Eslami (2019), *Thermal Stresses: Advanced Theory and Applications*, 2nd ed. | doi:[10.1007/978-3-030-10436-8](https://doi.org/10.1007/978-3-030-10436-8) | `crates/caekit/tests/elastic_solid.rs`, `thermoelastic_solid.rs` |
| Von Mises equivalent stress | `von_mises_stress` of `crates/caekit-mechanics/src/elastic_solid.rs` | von Mises (1913), *Nachr. Ges. Wiss. Göttingen, Math.-phys. Kl.*, 582–592; English translation as NASA TM-88448 (1986) | [ntrs.nasa.gov/citations/19860019802](https://ntrs.nasa.gov/citations/19860019802) | `crates/caekit/tests/elastic_solid.rs`, `thermoelastic_solid.rs` |
| Fully developed laminar flow between parallel plates, hydraulic diameter `2H` | `crates/caekit-fluids/src/laminar_channel.rs`; the [laminar channel](physics/laminar-channel.md) page | Shah, London (1978), *Laminar Flow Forced Convection in Ducts*, chapter "Parallel Plates", pp. 153–195. The page derives the closed form itself | ISBN 978-0-12-020051-1; doi:[10.1016/b978-0-12-020051-1.50011-5](https://doi.org/10.1016/b978-0-12-020051-1.50011-5) | `crates/caekit/tests/laminar_channel.rs` |
| Thermal resistance in series and the bulk energy balance of a heated stream | The analytical reference of the [cooling channel](physics/cooling-channel.md) page | Bergman, Lavine, Incropera, DeWitt, *Fundamentals of Heat and Mass Transfer*, 8th ed., Wiley | ISBN 978-1-119-72248-9 (print) | `crates/caekit/tests/cooling_channel.rs` |
| First-order upwind fluxes | The fluid energy sweep in `crates/caekit-thermal/src/lib.rs` | Courant, Isaacson, Rees (1952) | doi:[10.1002/cpa.3160050303](https://doi.org/10.1002/cpa.3160050303) | `thermal_mesh_refinement_recovers_first_order_enthalpy_advection` in `cooling_channel.rs` |
| `partitioned_gauss_seidel` | `crates/caekit-thermal/src/lib.rs`; `physics.coupling_solver` | Saad (2003), Gauss–Seidel iteration, applied blockwise to the wall and the coolant | doi:[10.1137/1.9780898718003](https://doi.org/10.1137/1.9780898718003) | `solves_nonmatching_meshes_and_closes_energy_balances` and `stops_with_trace_if_coupling_fails_to_converge` in `cooling_channel.rs` |
| Conservative transfer of heat between non-matching interface meshes | Interval intersection of the wall and coolant meshes (`overlap`) in `crates/caekit-thermal/src/interface.rs` | Farhat, Lesoinne, Le Tallec (1998); preCICE mapping documentation | doi:[10.1016/S0045-7825(97)00216-8](https://doi.org/10.1016/S0045-7825(97)00216-8); [precice.org/docs/configuration/basics/mapping](https://precice.org/docs/configuration/basics/mapping/) | `solves_nonmatching_meshes_and_closes_energy_balances` in `cooling_channel.rs` |
| Interface power corrected by a weighted least-squares projection onto the regional balances | `solid_interface_equilibration` in `crates/caekit-thermal/src/heat_conduction/region_energy.rs`; the [multimaterial page](physics/multimaterial-heat-conduction.md) | Odsæter, Wheeler, Kvamsdal, Larson (2017), postprocessing of a non-conservative flux by a weighted correction. The weights and the graph-Laplacian system are CAEKit's own specialization, described on the page | doi:[10.1016/j.cma.2016.11.018](https://doi.org/10.1016/j.cma.2016.11.018); [arXiv:1605.04076](https://arxiv.org/abs/1605.04076) | `a_cycle_of_region_pairs_keeps_the_flux_from_the_element_gradients` in `multimaterial_heat_conduction.rs` |
| Observed order (two levels, pairwise and least squares), Richardson extrapolation, grid convergence index and the monotone, oscillatory or divergent classification of a sequence | `crates/caekit-numerics/src/verify.rs`; the refinement studies in `crates/caekit/tests/` | Roache (1994), grid convergence index; Celik et al. (2008), three-grid procedure with safety factor 1.25; Eça, Hoekstra (2014), least-squares observed order; Stern, Wilson, Coleman, Paterson (2001), convergence ratio `R`; NASA Glenn NPARC Alliance, *Examining Spatial (Grid) Convergence*, worked example | doi:[10.1115/1.2910291](https://doi.org/10.1115/1.2910291); doi:[10.1115/1.2960953](https://doi.org/10.1115/1.2960953); doi:[10.1016/j.jcp.2014.01.006](https://doi.org/10.1016/j.jcp.2014.01.006); doi:[10.1115/1.1412235](https://doi.org/10.1115/1.1412235); [grc.nasa.gov/www/wind/valid/tutorial/spatconv.html](https://www.grc.nasa.gov/www/wind/valid/tutorial/spatconv.html) | `reproduces_the_nparc_spatial_convergence_example`, `classifies_monotone_oscillatory_and_divergent_sequences` and `grid_convergence_refuses_sequences_that_are_not_monotone` in `verify.rs`; the refinement test of each kind |
| Grundmann–Möller quadrature of degree `2s + 1` on the tetrahedron | The error integrals of the refinement study in `crates/caekit/tests/elastic_solid.rs` | Grundmann, Möller (1978), invariant integration formulas for the n-simplex | doi:[10.1137/0715019](https://doi.org/10.1137/0715019) | `grundmann_moeller_rules_integrate_monomials_up_to_their_degree` in `elastic_solid.rs` |
| Collapsed (Duffy) Gauss–Legendre quadrature on the tetrahedron | The error integrals of the refinement studies in `crates/caekit/tests/heat_conduction.rs`, `multimaterial_heat_conduction.rs` and `thermoelastic_solid.rs` | Duffy (1982), quadrature over a cube collapsed onto a pyramid or simplex | doi:[10.1137/0719090](https://doi.org/10.1137/0719090) | `the_study_quadrature_integrates_degree_seven_monomials_exactly` in `thermoelastic_solid.rs`; the cell-volume check of the study in `multimaterial_heat_conduction.rs`; `heat_conduction.rs` has no separate exactness test |
| Lamé solution of a thick-walled cylinder under inner and outer pressure, `u = (A r + B / r) e_r` in plane strain | The exact solution of the refinement study in `crates/caekit/tests/elastic_solid.rs`, derived in full on the [elastic solid](physics/elastic-solid.md#refinement-study) page | Timoshenko, Goodier, *Theory of Elasticity*, 3rd ed., McGraw-Hill, 1970 | ISBN 0-07-064720-8 | `lame_thick_cylinder_converges_at_the_formal_orders_in_l2_and_energy` and `the_order_study_rejects_the_affine_cylinder` in `elastic_solid.rs` |
| SHA-256 of the typed model (`model_sha256`) | `solve` in `crates/caekit/src/lib.rs`, with the RustCrypto `sha2` crate | NIST FIPS 180-4, Secure Hash Standard | doi:[10.6028/NIST.FIPS.180-4](https://doi.org/10.6028/NIST.FIPS.180-4) | `reference_models_keep_their_version_1_digest` in `crates/caekit/tests/model_identity.rs` |
| IEEE 754 binary64 arithmetic with round-to-nearest | All solver crates; the `ieee754_binary64` field of the run report | IEEE 754-2019, Standard for Floating-Point Arithmetic | doi:[10.1109/IEEESTD.2019.8766229](https://doi.org/10.1109/IEEESTD.2019.8766229); [standards.ieee.org/ieee/754/6210](https://standards.ieee.org/ieee/754/6210/) | `scripts/verify-reference-cases.mjs` compares the native and WASI builds to a relative 1e-10 |

### How the citations were confirmed

On 2026-11-21 each DOI was looked up in the Crossref registry
(`https://api.crossref.org/works/<doi>`) and the title, authors, year and venue of the
record were compared with the row. Every DOI above is registered and matched, apart from
the exceptions below. The preCICE, MFEM, arXiv and NASA pages were opened and their
titles and subjects checked. The IEEE 754 page resolves.

Not read in full, so only the bibliographic record is confirmed and not an equation number
or section: the paywalled and book-length sources (Saad, Higham, Ciarlet, Hetnarski and
Eslami, Shah and London, Bergman et al., Liu and Joe, Roache, Celik et al.). The
mean-ratio formula in the Liu and Joe row was matched against secondary sources that cite
the paper, not against the paper. Crossref lists no authors for the record of Celik et al.
(2008) or of FIPS 180-4. The ISBN of Bergman et al. is the print ISBN that several
retailers list for the 8th edition, not a publisher record.

On the same day the sources added with the refinement studies were checked the same way:
the Crossref records of Eça and Hoekstra (2014), Stern et al. (2001), Grundmann and Möller
(1978) and Duffy (1982) match their rows, and the ISBN of Timoshenko and Goodier is that of
the 3rd edition in its [Open Library record](https://openlibrary.org/isbn/0070647208). None
of these was read in full. The quadrature rules are verified independently of their papers
by the monomial-exactness tests in the table, and the Lamé solution is derived on its page.
The NPARC page was read on 2026-11-21; its printed values are the ones that
`reproduces_the_nparc_spatial_convergence_example` reproduces.
