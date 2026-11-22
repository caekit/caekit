//! Linear three-dimensional elasticity on positively oriented tetrahedra.

use serde::{Deserialize, Serialize};

use caekit_mesh::tetra::{CheckedTetraMesh, Tet, TetraTopology, cross, dot, subtract};
use caekit_mesh::{GeometryRevision, MeshMetrics, TetraMesh};
use caekit_numerics::fmath::norm2;
use caekit_numerics::{ConstrainedSystem, Convergence, LinearSettings, StopReason, norm};
use caekit_types::error::{finite, nonempty, positive};
use caekit_types::{Balance, Output, Quantity, SampleLocation, Solution};
use caekit_types::{Error, MaterialSource};

use caekit_types::Pressure;

/// Static linear elasticity of one connected isotropic solid (the `elastic_solid` physics case).
///
/// Small-strain equilibrium is solved for the displacement of every mesh vertex with linear,
/// constant-strain tetrahedra. A converged solve reports, for each global axis, a force balance in
/// N and a moment balance in N m between the applied loads and the support reactions, with moments
/// taken about the first mesh vertex.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElasticSolid {
    /// Identifier and revision of the geometry the mesh and conditions belong to; both strings must
    /// be nonempty.
    pub geometry: GeometryRevision,
    /// Tetrahedral mesh in global Cartesian coordinates in m, with zero-based connectivity and
    /// every exterior triangle assigned to one named region. Boundary conditions refer to these
    /// region names.
    pub mesh: TetraMesh,
    /// Isotropic elastic constants of the whole solid.
    pub material: IsotropicElasticMaterial,
    /// Displacement constraints and surface loads on named exterior regions of the mesh. The
    /// prescribed displacement components together must restrain all six rigid-body modes. Traction
    /// and pressure loads on the same faces add, and faces without a load are traction-free.
    pub boundary_conditions: Vec<SolidBoundary>,
    /// Uniform body force per unit volume in N/m³ as global `[x, y, z]` components; each must be
    /// finite. Defaults to zero when omitted.
    #[serde(default)]
    pub body_force_n_m3: [f64; 3],
}

/// Mechanical inputs for an analysis that supplies its own solid mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolidMechanics {
    /// Isotropic elastic constants of the whole solid.
    pub material: IsotropicElasticMaterial,
    /// Displacement constraints and surface loads on named exterior regions of the mesh. The
    /// prescribed displacement components together must restrain all six rigid-body modes. Traction
    /// and pressure loads on the same faces add, and faces without a load are traction-free.
    pub boundary_conditions: Vec<SolidBoundary>,
    /// Uniform body force per unit volume in N/m³ as global `[x, y, z]` components; each must be
    /// finite. Defaults to zero when omitted.
    #[serde(default)]
    pub body_force_n_m3: [f64; 3],
}

/// Isotropic thermal expansion of a solid.
///
/// A temperature `T` produces the free strain `α (T − T_ref)` on each of the three normal strain
/// components and no shear strain.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThermalExpansion {
    /// Stress-free reference temperature `T_ref` in K; positive and finite.
    pub reference_temperature_k: f64,
    /// Linear expansion coefficient `α` in 1/K; finite, and may be zero or negative.
    pub linear_expansion_coefficient_per_k: f64,
}

impl ThermalExpansion {
    /// Checks that the reference temperature is positive and finite and that the coefficient is
    /// finite.
    ///
    /// A violation is an [`Error::InvalidModel`] with a path under `physics.expansion`.
    pub fn validate(self) -> Result<(), Error> {
        positive(
            "physics.expansion.reference_temperature_k",
            self.reference_temperature_k,
        )?;
        finite(
            "physics.expansion.linear_expansion_coefficient_per_k",
            self.linear_expansion_coefficient_per_k,
        )
    }
}

/// Linear isotropic elastic material given by Young's modulus and Poisson's ratio.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsotropicElasticMaterial {
    /// Label of the material; must contain a non-whitespace character. It does not enter the
    /// calculation.
    pub id: String,
    /// Young's modulus in Pa; positive and finite.
    pub young_modulus_pa: f64,
    /// Poisson's ratio, dimensionless; strictly between −1 and 0.5. The shear modulus derived from
    /// it and the modulus must be positive and finite, and the first Lamé parameter finite.
    pub poisson_ratio: f64,
    /// Citation and usage terms of the property values; both strings must be nonempty.
    pub source: MaterialSource,
}

/// Displacement constraint or surface load on a named exterior region of a solid mesh.
///
/// In JSON the variant is selected by the `kind` key in snake_case (`displacement`, `traction` or
/// `pressure`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SolidBoundary {
    /// Prescribes displacement components in m along the global x, y and z axes at every vertex of
    /// the region's faces.
    ///
    /// At least one of the three components must be given and given values must be finite. A vertex
    /// shared with another displacement condition must receive the same value for the same
    /// component.
    Displacement {
        /// Name of an exterior face region of the mesh.
        region: String,
        /// A null component remains free.
        components_m: [Option<f64>; 3],
    },
    /// Applies a uniform surface traction vector to the region's faces.
    Traction {
        /// Name of an exterior face region of the mesh.
        region: String,
        /// Traction, a force per unit area, in Pa as global `[x, y, z]` components; each must be
        /// finite. The direction is fixed and does not follow the face normals.
        components_pa: [f64; 3],
    },
    /// Applies a pressure load normal to the region's faces.
    ///
    /// The traction on a face with outward unit normal `n` is `−(applied − ambient) n`, so a
    /// positive pressure difference pushes the surface into the solid.
    Pressure {
        /// Name of an exterior face region of the mesh.
        region: String,
        /// Pressure acting on the region, absolute or gauge with an explicit absolute reference;
        /// its absolute value must be positive and finite.
        applied: Pressure,
        /// Pressure subtracted from `applied`, subject to the same checks. Only the difference
        /// loads the solid, and it must be finite.
        ambient: Pressure,
    },
}

impl SolidBoundary {
    fn region(&self) -> &str {
        match self {
            Self::Displacement { region, .. }
            | Self::Traction { region, .. }
            | Self::Pressure { region, .. } => region,
        }
    }
}

#[derive(Clone, Copy)]
struct Lame {
    lambda: f64,
    mu: f64,
}

impl Lame {
    fn from_material(material: &IsotropicElasticMaterial) -> Result<Self, Error> {
        let nu = material.poisson_ratio;
        let modulus = material.young_modulus_pa;
        let mu = (0.5 * modulus) / (1.0 + nu);
        let lambda = 2.0 * (mu * nu) / (1.0 - 2.0 * nu);
        positive("elastic shear modulus", mu)?;
        finite("first Lame parameter", lambda)?;
        Ok(Self { lambda, mu })
    }

    fn stiffness(self, grad_a: [f64; 3], grad_b: [f64; 3], i: usize, j: usize) -> f64 {
        let shear = self.mu * grad_a[j] * grad_b[i];
        let dilatation = self.lambda * grad_a[i] * grad_b[j];
        let diagonal = if i == j {
            self.mu * dot(grad_a, grad_b)
        } else {
            0.0
        };
        shear + dilatation + diagonal
    }

    fn stress(self, strain: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
        let trace: f64 = (0..3).map(|i| strain[i][i]).sum();
        std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                let diagonal = if i == j { self.lambda * trace } else { 0.0 };
                2.0 * (self.mu * strain[i][j]) + diagonal
            })
        })
    }
}

struct ElasticInputs<'a> {
    material: &'a IsotropicElasticMaterial,
    boundary_conditions: &'a [SolidBoundary],
    body_force_n_m3: [f64; 3],
    path: &'static str,
}

/// Validated mechanical inputs bound to a checked tetrahedral mesh, ready to solve.
///
/// Created by [`SolidMechanics::prepare`]. It borrows the mesh, its checked topology and the
/// inputs, and follows the vertex and cell order of that mesh.
pub struct PreparedElasticSolid<'a> {
    inputs: ElasticInputs<'a>,
    mesh: &'a TetraMesh,
    topology: &'a TetraTopology<'a>,
    lame: Lame,
    prescribed: Vec<Option<f64>>,
}

struct ThermalStrain {
    nodal: Vec<f64>,
    three_bulk_modulus_pa: f64,
    free_expansion: Option<FreeExpansion>,
}

// For sliding supports on coordinate planes, a uniform expansion about the
// intersection of the planes has zero prescribed displacement and zero stress.
struct FreeExpansion {
    origin: [f64; 3],
    strain: f64,
}

impl FreeExpansion {
    fn compatible(vertices: &[[f64; 3]], prescribed: &[Option<f64>], strain: f64) -> Option<Self> {
        if strain == 0.0 {
            return None;
        }
        let mut origin = [None; 3];
        for (point, fixed) in vertices.iter().zip(prescribed.chunks_exact(3)) {
            for axis in 0..3 {
                if fixed[axis].is_some() && *origin[axis].get_or_insert(point[axis]) != point[axis]
                {
                    return None;
                }
            }
        }
        Some(Self {
            origin: origin.map(|x| x.unwrap_or(0.0)),
            strain,
        })
    }
}

impl ThermalStrain {
    fn mean(values: [f64; 4]) -> f64 {
        values.into_iter().map(|v| v * 0.25).sum()
    }

    fn cell_moments(&self, nodes: [usize; 4]) -> (f64, f64) {
        let values = nodes.map(|node| self.nodal[node]);
        let mean = Self::mean(values);
        // The exact P1 tetrahedral variance follows from E[N_i^2]=1/10 and E[N_i N_j]=1/20.
        let variance = values
            .into_iter()
            .map(|value| {
                let centered = value - mean;
                centered * centered
            })
            .sum::<f64>()
            / 20.0;
        (mean, variance)
    }
}

impl ElasticSolid {
    /// Quantities this case can report.
    ///
    /// `displacement_vector` in m and `nodal_reaction` in N are global vectors at the mesh
    /// vertices. A reaction is the force a support exerts on the solid and is zero on unconstrained
    /// components. `cauchy_stress`, a global tensor with tension positive, and `von_mises_stress`,
    /// both in Pa, are sampled at the cell centroids. `elastic_energy` is the strain energy of the
    /// whole solid in J.
    pub const OUTPUTS: &[Output] = &[
        Output::DisplacementVector,
        Output::CauchyStress,
        Output::VonMisesStress,
        Output::NodalReaction,
        Output::ElasticEnergy,
    ];
    const METHOD: &str = "static_small_strain_isotropic_p1_tetrahedral_fem";

    /// Checks the mesh, geometry revision, material, body force and boundary conditions and returns
    /// the mesh metrics.
    ///
    /// Fails with [`Error::InvalidModel`] for an invalid mesh topology or region assignment, an
    /// empty geometry, material or source string, a modulus that is not positive and finite, a
    /// Poisson ratio outside the open interval from −1 to 0.5, a non-finite load or displacement,
    /// an unknown exterior region, a displacement condition without a component, conflicting
    /// displacements at a shared vertex, or constraints that leave a rigid-body mode free. Paths of
    /// model fields start with `physics.`. Fails with [`Error::InsufficientMemory`] if the
    /// constraint storage cannot be allocated.
    pub fn validate(&self) -> Result<MeshMetrics, Error> {
        let mesh = self.checked_mesh()?;
        self.inputs().prepare(&mesh)?;
        Ok(mesh.into_metrics())
    }

    /// Solve a case accepted by `validate` with valid linear solver settings.
    pub fn solve(
        &self,
        settings: LinearSettings,
        outputs: &[Output],
    ) -> Result<Solution<Convergence>, Error> {
        self.solve_with_metrics(settings, outputs)
            .map(|(_, result)| result)
    }

    /// Validates and solves the case and also returns the metrics of the checked mesh.
    ///
    /// `outputs` selects which of the displacement, reaction, stress and von Mises fields are
    /// built. The strain energy and the force and moment balances are always produced for a
    /// converged solve. If the linear solve, with residuals in N, does not converge, the solution
    /// holds only its convergence trace. Errors are those of [`ElasticSolid::validate`], and
    /// [`Error::NonFiniteResult`] if a balance sum is not finite.
    pub fn solve_with_metrics(
        &self,
        settings: LinearSettings,
        outputs: &[Output],
    ) -> Result<(MeshMetrics, Solution<Convergence>), Error> {
        let mesh = self.checked_mesh()?;
        let result = self
            .inputs()
            .prepare(&mesh)?
            .solve(settings, outputs, None)?;
        Ok((mesh.into_metrics(), result))
    }

    fn checked_mesh(&self) -> Result<CheckedTetraMesh<'_>, Error> {
        let mesh = self.mesh.checked("physics.mesh")?;
        self.geometry.validate("physics.geometry")?;
        Ok(mesh)
    }

    fn inputs(&self) -> ElasticInputs<'_> {
        ElasticInputs {
            material: &self.material,
            boundary_conditions: &self.boundary_conditions,
            body_force_n_m3: self.body_force_n_m3,
            path: "physics",
        }
    }
}

impl SolidMechanics {
    /// Validates these inputs against a checked mesh and binds them to it for solving.
    ///
    /// The checks are those of [`ElasticSolid::validate`] for the material, body force and boundary
    /// conditions, with paths of model fields under `physics.mechanical`.
    pub fn prepare<'a>(
        &'a self,
        mesh: &'a CheckedTetraMesh<'a>,
    ) -> Result<PreparedElasticSolid<'a>, Error> {
        ElasticInputs {
            material: &self.material,
            boundary_conditions: &self.boundary_conditions,
            body_force_n_m3: self.body_force_n_m3,
            path: "physics.mechanical",
        }
        .prepare(mesh)
    }
}

impl<'a> ElasticInputs<'a> {
    fn prepare(self, checked: &'a CheckedTetraMesh<'a>) -> Result<PreparedElasticSolid<'a>, Error> {
        let path = self.path;
        nonempty(&format!("{path}.material.id"), &self.material.id)?;
        positive(
            &format!("{path}.material.young_modulus_pa"),
            self.material.young_modulus_pa,
        )?;
        if !(-1.0 < self.material.poisson_ratio && self.material.poisson_ratio < 0.5) {
            return Err(Error::invalid(
                format!("{path}.material.poisson_ratio"),
                "must be strictly between -1 and 0.5",
            ));
        }
        let lame = Lame::from_material(self.material)?;
        self.material
            .source
            .validate(&format!("{path}.material.source"))?;
        for (i, value) in self.body_force_n_m3.iter().enumerate() {
            finite(&format!("{path}.body_force_n_m3[{i}]"), *value)?;
        }
        let (mesh, topology) = (checked.mesh(), checked.topology());
        let prescribed = self.prescribed(mesh.vertices_m.len(), topology)?;
        if !supports_all_rigid_modes(&mesh.vertices_m, &prescribed) {
            return Err(Error::invalid(
                format!("{path}.boundary_conditions"),
                "prescribed displacement components must restrain all six rigid-body modes",
            ));
        }
        Ok(PreparedElasticSolid {
            inputs: self,
            mesh,
            topology,
            lame,
            prescribed,
        })
    }

    fn prescribed(
        &self,
        node_count: usize,
        topology: &TetraTopology<'_>,
    ) -> Result<Vec<Option<f64>>, Error> {
        let dofs = node_count
            .checked_mul(3)
            .ok_or_else(|| Error::InsufficientMemory {
                resource: "solid degrees of freedom".to_owned(),
            })?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(dofs)
            .map_err(|_| Error::InsufficientMemory {
                resource: "solid degrees of freedom".to_owned(),
            })?;
        values.resize(dofs, None);

        for (i, boundary) in self.boundary_conditions.iter().enumerate() {
            let path = format!("{}.boundary_conditions[{i}]", self.path);
            let region = boundary.region();
            let Some(faces) = topology.boundaries.get(region) else {
                return Err(Error::invalid(
                    format!("{path}.region"),
                    format!("unknown exterior region {region:?}"),
                ));
            };
            match boundary {
                SolidBoundary::Displacement { components_m, .. } => {
                    if !components_m.iter().any(Option::is_some) {
                        return Err(Error::invalid(
                            path,
                            "prescribe at least one displacement component on the region",
                        ));
                    }
                    for (axis, value) in components_m.iter().enumerate() {
                        let Some(value) = value else { continue };
                        finite(&format!("{path}.components_m[{axis}]"), *value)?;
                        for node in faces.iter().flat_map(|face| face.nodes) {
                            let existing = &mut values[3 * node + axis];
                            if existing.is_some_and(|other| other != *value) {
                                return Err(Error::invalid(
                                    format!("{path}.components_m[{axis}]"),
                                    format!(
                                        "conflicting displacement at vertex {node}, component {axis}"
                                    ),
                                ));
                            }
                            *existing = Some(*value);
                        }
                    }
                }
                SolidBoundary::Traction { components_pa, .. } => {
                    for (axis, value) in components_pa.iter().enumerate() {
                        finite(&format!("{path}.components_pa[{axis}]"), *value)?;
                    }
                }
                SolidBoundary::Pressure {
                    applied, ambient, ..
                } => {
                    applied.validate(&format!("{path}.applied"))?;
                    ambient.validate(&format!("{path}.ambient"))?;
                    finite(
                        &format!("{path}.pressure_difference"),
                        applied.difference_pa(*ambient),
                    )?;
                }
            }
        }
        Ok(values)
    }
}

impl PreparedElasticSolid<'_> {
    /// Solve with temperatures from this mesh in its original vertex order.
    /// A temperature is the sum of the supplied datum and its nodal offset.
    pub fn solve_thermoelastic(
        &self,
        settings: LinearSettings,
        outputs: &[Output],
        expansion: ThermalExpansion,
        datum_k: f64,
        offsets_k: &[f64],
    ) -> Result<Solution<Convergence>, Error> {
        expansion.validate()?;
        positive("solid temperature datum", datum_k)?;
        if offsets_k.len() != self.mesh.vertices_m.len() {
            return Err(Error::invalid(
                "solid temperature field",
                "requires one finite temperature offset per mesh vertex",
            ));
        }
        if let Some(node) = offsets_k.iter().position(|offset| {
            let temperature = datum_k + offset;
            !temperature.is_finite() || temperature <= 0.0
        }) {
            return Err(Error::invalid(
                format!("solid temperature at vertex {node}"),
                "absolute temperature must be positive and finite",
            ));
        }
        if expansion.linear_expansion_coefficient_per_k == 0.0 {
            return self.solve(settings, outputs, None);
        }

        let datum_change = datum_k - expansion.reference_temperature_k;
        let mut nodal: Vec<_> = offsets_k
            .iter()
            .map(|offset| expansion.linear_expansion_coefficient_per_k * (datum_change + offset))
            .collect();
        if !nodal.iter().all(|strain| strain.is_finite()) {
            return Err(Error::NonFiniteResult {
                quantity: "solid thermal strain".to_owned(),
            });
        }
        if nodal.iter().all(|&strain| strain == 0.0) {
            return self.solve(settings, outputs, None);
        }

        let free_expansion =
            FreeExpansion::compatible(&self.mesh.vertices_m, &self.prescribed, nodal[0]);
        if let Some(free) = &free_expansion {
            for strain in &mut nodal {
                *strain -= free.strain;
            }
        }

        let material = self.inputs.material;
        let three_bulk_modulus_pa =
            material.young_modulus_pa / (1.0 - 2.0 * material.poisson_ratio);
        let thermal = ThermalStrain {
            nodal,
            three_bulk_modulus_pa,
            free_expansion,
        };
        self.solve(settings, outputs, Some(&thermal))
    }

    fn solve(
        &self,
        settings: LinearSettings,
        outputs: &[Output],
        thermal: Option<&ThermalStrain>,
    ) -> Result<Solution<Convergence>, Error> {
        let topology = self.topology;
        let vertices = &self.mesh.vertices_m;
        let loads = self.assemble_forces();
        let prescribed = &self.prescribed;
        let (translation, relative) = crate::relative_constraints::<3>(prescribed);
        let equivalent_loads = thermal
            .map(|strain| self.add_thermal_loads(&loads, strain))
            .transpose()?;
        let mut system =
            ConstrainedSystem::new(relative, equivalent_loads.as_deref().unwrap_or(&loads));
        for (tet, nodes) in topology.elements.iter().zip(&self.mesh.cells) {
            for (a, node_a) in nodes.iter().enumerate() {
                for (b, node_b) in nodes.iter().enumerate() {
                    for i in 0..3 {
                        for j in 0..3 {
                            let stiffness = tet.volume
                                * self
                                    .lame
                                    .stiffness(tet.gradients[a], tet.gradients[b], i, j);
                            system.add(3 * node_a + i, 3 * node_b + j, stiffness);
                        }
                    }
                }
            }
        }

        let (displacement, convergence) = system.solve(settings, "solid_elastic_equilibrium", "N");
        let stop_reason = convergence.stop_reason;
        let mut result = Solution::new(ElasticSolid::METHOD, vec![convergence]);
        if stop_reason != StopReason::Converged {
            if thermal.is_some() {
                let trace = &mut result.convergence[0];
                let reason = trace
                    .diagnostic
                    .as_deref()
                    .unwrap_or("Mechanical equilibrium did not converge.");
                trace.diagnostic = Some(self.mesh_diagnostic(reason));
            }
            return Ok(result);
        }

        let mut u: Vec<[f64; 3]> = displacement
            .chunks_exact(3)
            .map(|values| [values[0], values[1], values[2]])
            .collect();
        let mut internal = vec![[0.0; 3]; vertices.len()];
        let mut stresses = Vec::new();
        let mut von_mises = Vec::new();
        let want_stress = outputs.contains(&Output::CauchyStress);
        let want_von_mises = outputs.contains(&Output::VonMisesStress);
        let mut energy = 0.0;
        for (tet, &nodes) in topology.elements.iter().zip(&self.mesh.cells) {
            let mut strain = strain(tet, nodes.map(|i| u[i]));
            if let Some(thermal) = thermal {
                let (mean, variance) = thermal.cell_moments(nodes);
                for (axis, row) in strain.iter_mut().enumerate() {
                    row[axis] -= mean;
                }
                // Integrate the part of elastic energy from the varying isotropic strain.
                // Its linear cross-term about the centroid has zero mean.
                energy += 1.5 * tet.volume * (thermal.three_bulk_modulus_pa * variance);
            }
            let stress = self.lame.stress(strain);
            for (a, node) in nodes.into_iter().enumerate() {
                for (i, row) in stress.iter().enumerate() {
                    internal[node][i] += tet.volume * dot(*row, tet.gradients[a]);
                }
            }
            energy += 0.5
                * tet.volume
                * stress
                    .iter()
                    .zip(strain)
                    .map(|(stress, strain)| dot(*stress, strain))
                    .sum::<f64>();
            if want_stress {
                stresses.push(stress);
            }
            if want_von_mises {
                von_mises.push(von_mises_stress(stress));
            }
        }
        let reactions: Vec<[f64; 3]> = internal
            .iter()
            .enumerate()
            .map(|(node, force)| {
                std::array::from_fn(|i| {
                    let dof = 3 * node + i;
                    if prescribed[dof].is_some() {
                        force[i] - loads[dof]
                    } else {
                        0.0
                    }
                })
            })
            .collect();
        let (balances, failure) =
            mechanical_balances(vertices, &loads, &reactions, thermal.map(|_| settings))?;
        if let Some(reason) = failure {
            let trace = &mut result.convergence[0];
            trace.stop_reason = StopReason::NumericalBreakdown;
            trace.diagnostic = Some(self.mesh_diagnostic(&reason));
            return Ok(result);
        }
        result.balances = balances;
        result
            .quantities
            .insert(Output::ElasticEnergy, Quantity::scalar("J", energy));
        if outputs.contains(&Output::DisplacementVector) {
            for ((node, fixed), point) in u.iter_mut().zip(prescribed.chunks_exact(3)).zip(vertices)
            {
                for axis in 0..3 {
                    if let Some(free) = thermal.and_then(|t| t.free_expansion.as_ref()) {
                        node[axis] += free.strain * (point[axis] - free.origin[axis]);
                    }
                    node[axis] = fixed[axis].unwrap_or(node[axis] + translation[axis]);
                }
            }
            result.quantities.insert(
                Output::DisplacementVector,
                Quantity::VectorField {
                    unit: "m".to_owned(),
                    location: SampleLocation::Node,
                    positions_m: vertices.clone(),
                    values: u,
                },
            );
        }
        if outputs.contains(&Output::NodalReaction) {
            result.quantities.insert(
                Output::NodalReaction,
                Quantity::VectorField {
                    unit: "N".to_owned(),
                    location: SampleLocation::Node,
                    positions_m: vertices.clone(),
                    values: reactions,
                },
            );
        }
        if want_stress {
            result.quantities.insert(
                Output::CauchyStress,
                Quantity::TensorField {
                    unit: "Pa".to_owned(),
                    location: SampleLocation::Cell,
                    positions_m: topology.elements.iter().map(|tet| tet.centroid).collect(),
                    values: stresses,
                },
            );
        }
        if want_von_mises {
            result.quantities.insert(
                Output::VonMisesStress,
                Quantity::field(
                    "Pa",
                    SampleLocation::Cell,
                    topology.elements.iter().map(|tet| tet.centroid).collect(),
                    von_mises,
                ),
            );
        }
        Ok(result)
    }

    fn mesh_diagnostic(&self, reason: &str) -> String {
        let quality = self
            .topology
            .metrics
            .quality
            .as_ref()
            .expect("checked tetrahedral topology records mesh quality");
        format!(
            "{reason} Minimum tetrahedral mean ratio {:.6e} at cell {}.",
            quality.minimum_mean_ratio, quality.cell_index
        )
    }

    fn add_thermal_loads(&self, loads: &[f64], thermal: &ThermalStrain) -> Result<Vec<f64>, Error> {
        let mut rhs = loads.to_vec();
        for (tet, &nodes) in self.topology.elements.iter().zip(&self.mesh.cells) {
            let mean = ThermalStrain::mean(nodes.map(|node| thermal.nodal[node]));
            let thermal_normal_stress = thermal.three_bulk_modulus_pa * mean;
            for (a, node) in nodes.into_iter().enumerate() {
                for (axis, gradient) in tet.gradients[a].into_iter().enumerate() {
                    rhs[3 * node + axis] += tet.volume * (thermal_normal_stress * gradient);
                }
            }
        }
        if rhs.iter().all(|value| value.is_finite()) {
            Ok(rhs)
        } else {
            Err(Error::NonFiniteResult {
                quantity: "solid thermoelastic loads".to_owned(),
            })
        }
    }

    fn assemble_forces(&self) -> Vec<f64> {
        let topology = self.topology;
        let mut loads = vec![0.0; self.prescribed.len()];
        for (tet, nodes) in topology.elements.iter().zip(&self.mesh.cells) {
            for node in nodes {
                for (i, force) in self.inputs.body_force_n_m3.iter().enumerate() {
                    loads[3 * node + i] += (0.25 * tet.volume) * force;
                }
            }
        }
        for condition in self.inputs.boundary_conditions {
            if let SolidBoundary::Displacement { .. } = condition {
                continue;
            }
            for face in &topology.boundaries[condition.region()] {
                let traction = match condition {
                    SolidBoundary::Traction { components_pa, .. } => *components_pa,
                    SolidBoundary::Pressure {
                        applied, ambient, ..
                    } => {
                        let delta = applied.difference_pa(*ambient);
                        face.outward.map(|n| -delta * n)
                    }
                    SolidBoundary::Displacement { .. } => {
                        unreachable!("displacement is not a load")
                    }
                };
                for node in face.nodes {
                    for (i, component) in traction.into_iter().enumerate() {
                        loads[3 * node + i] += (face.area / 3.0) * component;
                    }
                }
            }
        }
        loads
    }
}

/// Smallest norm that a rigid-mode row may keep after projection on the accepted rows for the
/// row to count as independent.
///
/// Dimensionless. Reason: the rows are built from coordinates divided by the size of the body, so
/// each starts with a norm of at least one (its translation or rotation part has unit magnitude
/// or more). A row that lies in the span of the accepted rows keeps only rounding after the
/// projection, a few epsilon times the row norm, that is about 1e-15; 1e-10 is five orders above
/// that. It is also the least independence the check demands: a row below 1e-10 would mean that
/// a prescribed displacement constrains a rigid mode through a lever arm 1e-10 times the size of
/// the body, which no meaningful support does.
const RIGID_MODE_INDEPENDENCE: f64 = 1.0e-10;

fn supports_all_rigid_modes(vertices: &[[f64; 3]], prescribed: &[Option<f64>]) -> bool {
    let reference = vertices[0];
    let size = vertices
        .iter()
        .map(|&p| norm(&subtract(p, reference)))
        .fold(0.0, f64::max);
    let mut basis: Vec<[f64; 6]> = Vec::with_capacity(6);
    for (node, &point) in vertices.iter().enumerate() {
        let [x, y, z] = subtract(point, reference).map(|p| p / size);
        let modes = [
            [1.0, 0.0, 0.0, 0.0, z, -y],
            [0.0, 1.0, 0.0, -z, 0.0, x],
            [0.0, 0.0, 1.0, y, -x, 0.0],
        ];
        for (component, mut row) in modes.into_iter().enumerate() {
            if prescribed[3 * node + component].is_none() {
                continue;
            }
            for mode in &basis {
                let projection: f64 = row.iter().zip(mode).map(|(a, b)| a * b).sum();
                for (value, basis_value) in row.iter_mut().zip(mode) {
                    *value -= projection * basis_value;
                }
            }
            let magnitude = norm(&row);
            if magnitude > RIGID_MODE_INDEPENDENCE {
                basis.push(row.map(|x| x / magnitude));
            }
            if basis.len() == 6 {
                return true;
            }
        }
    }
    false
}

fn strain(tet: &Tet, u: [[f64; 3]; 4]) -> [[f64; 3]; 3] {
    // Subtract a common displacement so rigid translation cancels before multiplication.
    let differences = u.map(|value| subtract(value, u[0]));
    let gradient: [[f64; 3]; 3] = std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            (1..4)
                .map(|a| differences[a][i] * tet.gradients[a][j])
                .sum()
        })
    });
    std::array::from_fn(|i| std::array::from_fn(|j| 0.5 * gradient[i][j] + 0.5 * gradient[j][i]))
}

fn von_mises_stress(s: [[f64; 3]; 3]) -> f64 {
    let diagonal = [s[0][0] - s[1][1], s[1][1] - s[2][2], s[2][2] - s[0][0]];
    norm(&[
        diagonal[0] * std::f64::consts::FRAC_1_SQRT_2,
        diagonal[1] * std::f64::consts::FRAC_1_SQRT_2,
        diagonal[2] * std::f64::consts::FRAC_1_SQRT_2,
        3.0_f64.sqrt() * s[0][1],
        3.0_f64.sqrt() * s[0][2],
        3.0_f64.sqrt() * s[1][2],
    ])
}

fn mechanical_balances(
    vertices: &[[f64; 3]],
    loads: &[f64],
    reactions: &[[f64; 3]],
    coupled_settings: Option<LinearSettings>,
) -> Result<(Vec<Balance>, Option<String>), Error> {
    let mut force = [[0.0; 3]; 2];
    let mut moment = [[0.0; 3]; 2];
    let origin = vertices[0];
    for ((&position, applied), &reaction) in
        vertices.iter().zip(loads.chunks_exact(3)).zip(reactions)
    {
        let lever = subtract(position, origin);
        for (group, contributions) in [[applied[0], applied[1], applied[2]], reaction]
            .into_iter()
            .enumerate()
        {
            let torque = cross(lever, contributions);
            for i in 0..3 {
                force[group][i] += contributions[i];
                moment[group][i] += torque[i];
            }
        }
    }
    let mut balances = Vec::new();
    for (i, axis) in ["x", "y", "z"].into_iter().enumerate() {
        let balance = Balance::new(
            &format!("solid_force_{axis}"),
            "N",
            [("applied", force[0][i]), ("reaction", force[1][i])],
        )?;
        if let Some(settings) = coupled_settings {
            let applied = loads.iter().skip(i).step_by(3);
            let scale = norm2(applied.clone().copied());
            if scale > 0.0 {
                // A sum of n residual components is bounded by sqrt(n) times
                // their L2 norm. Thermal equivalent loads are internal forces.
                let n = vertices.len() as f64;
                let sum_abs = applied.map(|f| f.abs()).sum::<f64>()
                    + reactions.iter().map(|r| r[i].abs()).sum::<f64>();
                // The second term is the rounding of the two running sums of the balance
                // (n - 1 additions each, relative error epsilon/2) plus the final subtraction, at
                // most (n/2) epsilon of the sum of the absolute terms; (n + 1) epsilon is more
                // than twice that.
                let limit = n.sqrt()
                    * settings
                        .absolute_residual_tolerance
                        .abs()
                        .max(settings.relative_residual_tolerance.abs() * scale)
                    + (n + 1.0) * f64::EPSILON * sum_abs;
                if !limit.is_finite() {
                    return Err(Error::NonFiniteResult {
                        quantity: format!("solid {axis}-force balance tolerance"),
                    });
                }
                if balance.imbalance.abs() > limit {
                    return Ok((
                        Vec::new(),
                        Some(format!(
                            "Recovered {axis}-force imbalance {:.6e} N exceeds the external-load balance tolerance {limit:.6e} N (applied {:.6e} N, reaction {:.6e} N).",
                            balance.imbalance, force[0][i], force[1][i]
                        )),
                    ));
                }
            }
        }
        balances.push(balance);
        balances.push(Balance::new(
            &format!("solid_moment_{axis}"),
            "N m",
            [("applied", moment[0][i]), ("reaction", moment[1][i])],
        )?);
    }
    Ok((balances, None))
}
