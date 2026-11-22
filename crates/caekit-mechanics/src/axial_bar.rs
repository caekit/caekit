//! Small-strain static elasticity for a straight bar with uniform cross-section.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use caekit_mesh::{AxisFrame, GeometryRevision, LineMesh, MeshMetrics};
use caekit_numerics::{ConstrainedSystem, Convergence, LinearSettings, StopReason};
use caekit_types::error::{finite, nonempty, positive};
use caekit_types::{Balance, Output, Quantity, SampleLocation, Solution};
use caekit_types::{Error, MaterialSource};

/// Static axial deformation of a straight elastic bar (the `axial_bar` physics case).
///
/// The bar has a uniform cross-section and a constant Young's modulus and carries no body force. It
/// is discretized with two-node linear elements along its local axis, and loads and supports act
/// only at the two mesh endpoints. A converged solve reports the `global_axial_force` balance in N
/// between the applied end forces and the support reactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxialBar {
    /// Geometry revision, placement of the bar axis, length and cross-section area.
    pub geometry: BarGeometry,
    /// Axial mesh in the local coordinate in m: at least two strictly increasing vertices running
    /// from exactly 0 to `geometry.length_m`. Its two distinct, nonempty endpoint region names are
    /// the regions that `boundary_conditions` refer to.
    pub mesh: LineMesh,
    /// Elastic material of the whole bar.
    pub material: ElasticMaterial,
    /// Exactly one condition for each of the two mesh endpoints, in any order. At least one must be
    /// a displacement, which removes rigid translation; a free end is a force of zero.
    pub boundary_conditions: Vec<BarBoundary>,
}

/// Identity, placement and dimensions of an axial bar.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarGeometry {
    /// Identifier and revision of the geometry the mesh and conditions belong to; both strings must
    /// be nonempty.
    pub identity: GeometryRevision,
    /// Origin in m and unit direction of the bar axis in the global Cartesian frame. The local
    /// coordinate `x` maps to `origin_m + x * direction`, and positive displacements and forces act
    /// along this direction.
    pub frame: AxisFrame,
    /// Bar length in m; positive and finite. The last mesh vertex must equal it exactly.
    pub length_m: f64,
    /// Uniform cross-section area in m²; positive and finite.
    pub cross_section_area_m2: f64,
}

/// Linear elastic material of an axial bar.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElasticMaterial {
    /// Label of the material; must contain a non-whitespace character. It does not enter the
    /// calculation.
    pub id: String,
    /// Young's modulus in Pa; positive and finite.
    pub young_modulus_pa: f64,
    /// Citation and usage terms of the property value; both strings must be nonempty.
    pub source: MaterialSource,
}

/// Condition at one endpoint of an axial bar.
///
/// In JSON the variant is selected by the `kind` key in snake_case (`displacement` or `force`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BarBoundary {
    /// Prescribes the axial displacement of the endpoint.
    Displacement {
        /// Name of the mesh endpoint region the condition applies to: the `start_region` or
        /// `end_region` of the bar mesh.
        region: String,
        /// Displacement in m, positive along the frame direction; must be finite.
        value_m: f64,
    },
    /// Applies an axial point force to the endpoint; a force of zero leaves the end free.
    Force {
        /// Name of the mesh endpoint region the condition applies to: the `start_region` or
        /// `end_region` of the bar mesh.
        region: String,
        /// Force in N, positive along the frame direction; must be finite.
        value_n: f64,
    },
}

impl BarBoundary {
    fn region(&self) -> &str {
        match self {
            Self::Displacement { region, .. } | Self::Force { region, .. } => region,
        }
    }
}

impl AxialBar {
    /// Quantities this case can report.
    ///
    /// `axial_displacement` is sampled at the mesh vertices in m and `axial_stress` at the element
    /// midpoints in Pa, with tension positive. `end_displacement` in m and `reaction_force` in N
    /// are keyed by endpoint region. A reaction is reported only for an endpoint with a
    /// displacement condition and is the force the support exerts on the bar, positive along the
    /// frame direction.
    pub const OUTPUTS: &[Output] = &[
        Output::AxialDisplacement,
        Output::AxialStress,
        Output::EndDisplacement,
        Output::ReactionForce,
    ];
    const METHOD: &str = "static_small_strain_axial_bar_p1_fem";

    /// Checks the geometry, material, mesh and endpoint conditions and returns the mesh metrics.
    ///
    /// Fails with [`Error::InvalidModel`], whose path starts with `physics.`, when a geometry,
    /// material or source string is empty, the frame origin is not finite or its direction is not a
    /// unit vector, the length, area or modulus is not positive and finite, the mesh does not run
    /// from 0 to the length in increasing steps between two distinct named endpoints, a condition
    /// names an unknown endpoint or holds a non-finite value, an endpoint has no condition or more
    /// than one, or no displacement is prescribed.
    pub fn validate(&self) -> Result<MeshMetrics, Error> {
        self.geometry
            .identity
            .validate("physics.geometry.identity")?;
        self.geometry.frame.validate("physics.geometry.frame")?;
        positive("physics.geometry.length_m", self.geometry.length_m)?;
        positive(
            "physics.geometry.cross_section_area_m2",
            self.geometry.cross_section_area_m2,
        )?;
        nonempty("physics.material.id", &self.material.id)?;
        positive(
            "physics.material.young_modulus_pa",
            self.material.young_modulus_pa,
        )?;
        self.material.source.validate("physics.material.source")?;

        let mesh = self.mesh.validate("physics.mesh", self.geometry.length_m)?;
        let mut assigned = [false; 2];
        let mut fixed = false;
        for (i, boundary) in self.boundary_conditions.iter().enumerate() {
            let path = format!("physics.boundary_conditions[{i}]");
            let end = match self.mesh.endpoint(boundary.region()) {
                Some(0) => 0,
                Some(_) => 1,
                None => {
                    return Err(Error::invalid(
                        path,
                        format!("unknown endpoint region {:?}", boundary.region()),
                    ));
                }
            };
            if std::mem::replace(&mut assigned[end], true) {
                return Err(Error::invalid(path, "endpoint already has a condition"));
            }
            match boundary {
                BarBoundary::Displacement { value_m, .. } => {
                    finite(&path, *value_m)?;
                    fixed = true;
                }
                BarBoundary::Force { value_n, .. } => finite(&path, *value_n)?,
            }
        }
        if !assigned.into_iter().all(|x| x) {
            return Err(Error::invalid(
                "physics.boundary_conditions",
                "each endpoint requires a displacement or force; specify zero for a free end",
            ));
        }
        if !fixed {
            return Err(Error::invalid(
                "physics.boundary_conditions",
                "prescribe at least one displacement to remove rigid translation",
            ));
        }
        Ok(mesh)
    }

    /// Solve a case accepted by `validate` with valid linear solver settings.
    pub fn solve(&self, settings: LinearSettings) -> Result<Solution<Convergence>, Error> {
        let vertices = &self.mesh.vertices_m;
        let mut prescribed = vec![None; vertices.len()];
        let mut loads = vec![0.0; vertices.len()];
        for boundary in &self.boundary_conditions {
            let node = self
                .mesh
                .endpoint(boundary.region())
                .expect("validated bar boundary condition names a mesh endpoint");
            match boundary {
                BarBoundary::Displacement { value_m, .. } => prescribed[node] = Some(*value_m),
                BarBoundary::Force { value_n, .. } => loads[node] = *value_n,
            }
        }

        let area = self.geometry.cross_section_area_m2;
        let modulus = self.material.young_modulus_pa;
        let ([translation], relative) = crate::relative_constraints(&prescribed);
        let mut system = ConstrainedSystem::new(relative, &loads);
        for (element, edge) in vertices.windows(2).enumerate() {
            let stiffness = modulus * area / (edge[1] - edge[0]);
            system.add(element, element, stiffness);
            system.add(element, element + 1, -stiffness);
            system.add(element + 1, element, -stiffness);
            system.add(element + 1, element + 1, stiffness);
        }

        let (mut displacements, convergence) = system.solve(settings, "elastic_equilibrium", "N");
        let reason = convergence.stop_reason;
        let mut result = Solution::new(Self::METHOD, vec![convergence]);
        if reason != StopReason::Converged {
            return Ok(result);
        }

        let frame = &self.geometry.frame;
        let stresses: Vec<_> = vertices
            .windows(2)
            .zip(displacements.windows(2))
            .map(|(edge, values)| modulus * ((values[1] - values[0]) / (edge[1] - edge[0])))
            .collect();
        for (u, fixed) in displacements.iter_mut().zip(prescribed) {
            *u = fixed.unwrap_or(*u + translation);
        }
        let mut reactions = BTreeMap::new();
        for boundary in &self.boundary_conditions {
            if let BarBoundary::Displacement { region, .. } = boundary {
                let force = if region == &self.mesh.start_region {
                    -stresses[0] * area
                } else {
                    stresses[stresses.len() - 1] * area
                };
                reactions.insert(region.clone(), force);
            }
        }
        result.balances.push(Balance::new(
            "global_axial_force",
            "N",
            [
                ("applied_force", loads.iter().sum()),
                ("support_reaction", reactions.values().sum()),
            ],
        )?);
        result.quantities.insert(
            Output::EndDisplacement,
            Quantity::ByRegion {
                unit: "m".to_owned(),
                values: BTreeMap::from([
                    (self.mesh.start_region.clone(), displacements[0]),
                    (
                        self.mesh.end_region.clone(),
                        displacements[vertices.len() - 1],
                    ),
                ]),
            },
        );
        result.quantities.insert(
            Output::AxialDisplacement,
            Quantity::Field {
                unit: "m".to_owned(),
                location: SampleLocation::Node,
                positions_m: vertices.iter().map(|&x| frame.position(x)).collect(),
                values: displacements,
            },
        );
        result.quantities.insert(
            Output::AxialStress,
            Quantity::Field {
                unit: "Pa".to_owned(),
                location: SampleLocation::Cell,
                positions_m: vertices
                    .windows(2)
                    .map(|edge| frame.position(edge[0] + 0.5 * (edge[1] - edge[0])))
                    .collect(),
                values: stresses,
            },
        );
        result.quantities.insert(
            Output::ReactionForce,
            Quantity::ByRegion {
                unit: "N".to_owned(),
                values: reactions,
            },
        );
        Ok(result)
    }
}
