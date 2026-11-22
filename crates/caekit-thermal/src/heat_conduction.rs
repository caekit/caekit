//! Steady isotropic heat conduction with linear tetrahedral elements.

mod region_energy;
mod volume;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::ConductiveMaterial;
use caekit_mesh::tetra::{CheckedTetraMesh, Tet, TetraFaceAdjacency, TetraTopology, Triangle, dot};
use caekit_mesh::{GeometryRevision, MeshMetrics, TetraMesh};
use caekit_numerics::{ConstrainedSystem, Convergence, LinearSettings, StopReason};
use caekit_types::error::{finite, positive};
use caekit_types::{Balance, Error, Output, Quantity, SampleLocation, Solution};
use region_energy::RegionEnergy;
use volume::{CellMaterials, CellProperties};
pub use volume::{MultimaterialThermal, ThermalVolumeRegion};

/// Steady heat conduction in one homogeneous solid, the `heat_conduction` physics case.
///
/// Solves `-div(k grad T) = Q` with constant isotropic conductivity `k` and uniform source `Q` on a
/// tetrahedral mesh with linear (P1) finite elements. The fields are the keys of the `physics`
/// object in the JSON model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeatConduction {
    /// Identity of the geometry that the mesh and its region assignments belong to. `id` and
    /// `revision` must be nonempty.
    pub geometry: GeometryRevision,
    /// Tetrahedral mesh of the solid in global coordinates in m. Its named exterior triangle
    /// regions are the regions that boundary conditions refer to.
    pub mesh: TetraMesh,
    /// Material, boundary conditions and volumetric heat source of the solid.
    pub thermal: SolidThermal,
}

/// Steady heat conduction through several materials in ideal thermal contact, the
/// `multimaterial_heat_conduction` physics case.
///
/// All volume regions share one conforming tetrahedral mesh, so the temperature is continuous
/// across material interfaces while conductivity and heat source are constant within each volume
/// region. The fields are the keys of the `physics` object in the JSON model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MultimaterialHeatConduction {
    /// Identity of the geometry that the mesh and its region assignments belong to. `id` and
    /// `revision` must be nonempty.
    pub geometry: GeometryRevision,
    /// Tetrahedral mesh of the whole solid in global coordinates in m. Its cell order defines the
    /// zero-based indices in `thermal.volume_regions[].cells`, and its named exterior triangle
    /// regions are the regions that boundary conditions refer to.
    pub mesh: TetraMesh,
    /// Materials, their assignment to cells by volume region, and exterior boundary conditions.
    pub thermal: MultimaterialThermal,
}

/// Thermal definition of a homogeneous solid, read from `physics.thermal` in the `heat_conduction`
/// and `thermoelastic_solid` cases.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolidThermal {
    /// The single material of the solid. Its conductivity applies to every cell.
    pub material: ConductiveMaterial,
    /// Conditions on named exterior regions of the mesh, at most one per region. A region without a
    /// condition is insulated. At least one `temperature` or `convection` condition is required to
    /// determine the absolute temperature.
    pub boundary_conditions: Vec<ThermalBoundary>,
    /// Uniform volumetric heat generation `Q` in W/m³. Positive values generate heat and negative
    /// values remove it. Must be finite. Defaults to 0 when omitted.
    #[serde(default)]
    pub volumetric_heat_source_w_m3: f64,
}

/// Thermal condition on one named exterior region of the tetrahedral mesh.
///
/// In JSON the `kind` key selects the variant in snake_case (`temperature`, `heat_flux`,
/// `insulation` or `convection`) and the remaining keys are the variant's fields. Each region may
/// carry at most one condition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ThermalBoundary {
    /// Fixes the temperature at every vertex of the region; `"kind": "temperature"` in JSON.
    ///
    /// Regions that share a vertex must prescribe exactly the same temperature there.
    Temperature {
        /// Name of the exterior region in `mesh.boundary_regions` that the condition applies to.
        region: String,
        /// Prescribed absolute surface temperature in K. Must be positive and finite.
        temperature_k: f64,
    },
    /// Applies a uniform normal heat flux over the region; `"kind": "heat_flux"` in JSON.
    HeatFlux {
        /// Name of the exterior region in `mesh.boundary_regions` that the condition applies to.
        region: String,
        /// Positive into the solid, opposite the outward normal.
        /// Uniform over the region, in W/m². Must be finite.
        inward_heat_flux_w_m2: f64,
    },
    /// Sets zero normal heat flux on the region; `"kind": "insulation"` in JSON.
    ///
    /// This is the same condition as leaving the region unassigned, but it counts as the region's
    /// one condition.
    Insulation {
        /// Name of the exterior region in `mesh.boundary_regions` that the condition applies to.
        region: String,
    },
    /// Exchanges heat with an ambient fluid through a film; `"kind": "convection"` in JSON.
    ///
    /// The inward heat flux is `film_coefficient_w_m2_k * (ambient_temperature_k - T_surface)`. The
    /// region must have a positive total area.
    Convection {
        /// Name of the exterior region in `mesh.boundary_regions` that the condition applies to.
        region: String,
        /// Absolute temperature of the ambient fluid in K. Must be positive and finite.
        ambient_temperature_k: f64,
        /// Film coefficient `h` in W/(m²·K). Must be positive and finite.
        film_coefficient_w_m2_k: f64,
    },
}

impl ThermalBoundary {
    fn region(&self) -> &str {
        match self {
            Self::Temperature { region, .. }
            | Self::HeatFlux { region, .. }
            | Self::Insulation { region }
            | Self::Convection { region, .. } => region,
        }
    }
}

/// Temperature samples follow the checked mesh's vertex order.
/// Keeping the offset separate preserves temperature differences for coupling.
pub struct SolidTemperature {
    datum_k: f64,
    offsets_k: Vec<f64>,
}

impl SolidTemperature {
    /// Returns the reference temperature in K from which the offsets are measured.
    ///
    /// It is the value of the first `temperature` condition in boundary-condition order. Without
    /// such a condition it is the film-conductance-weighted mean surface temperature implied by the
    /// global heat balance, or the first convection ambient temperature if that mean is not
    /// positive and finite.
    pub fn datum_k(&self) -> f64 {
        self.datum_k
    }

    /// Returns the solved temperature minus [`SolidTemperature::datum_k`] in K, one value per mesh
    /// vertex in vertex order.
    pub fn offsets_k(&self) -> &[f64] {
        &self.offsets_k
    }
}

/// A validated conduction problem bound to a checked tetrahedral mesh, ready to assemble and solve.
///
/// It holds the conductivity and heat source of each cell, the condition of each assigned exterior
/// region, the prescribed vertex temperatures and the temperature datum. Create it with
/// [`SolidThermal::prepare`]; it borrows the mesh and the thermal definition.
pub struct PreparedConduction<'a> {
    mesh: &'a TetraMesh,
    topology: &'a TetraTopology<'a>,
    materials: CellMaterials<'a>,
    conditions: BTreeMap<&'a str, &'a ThermalBoundary>,
    prescribed: Vec<Option<f64>>,
    datum_k: f64,
}

impl HeatConduction {
    /// Validates the geometry identity, the mesh and the thermal definition without solving.
    ///
    /// The thermal checks are those of [`SolidThermal::prepare`]. Returns the mesh metrics, or the
    /// first [`Error::InvalidModel`] found, with a path under `physics.geometry`, `physics.mesh` or
    /// `physics.thermal`.
    pub fn validate(&self) -> Result<MeshMetrics, Error> {
        let mesh = self.checked_mesh()?;
        self.thermal.prepare(&mesh)?;
        Ok(mesh.into_metrics())
    }

    /// Validates the case, then assembles and solves the P1 finite-element conduction system.
    ///
    /// Prescribed temperatures are eliminated symmetrically and the free equations are solved by
    /// Jacobi-preconditioned conjugate gradient under `settings`, with residuals in W. Returns the
    /// mesh metrics and a [`Solution`] with method
    /// `steady_isotropic_heat_conduction_p1_tetrahedral_fem` and the `solid_heat_conduction`
    /// convergence stage. If that stage converges, the solution also carries the `solid_energy`
    /// balance in W (volumetric generation, prescribed inward flux, convection and
    /// prescribed-temperature reaction) and those of [`SolidThermal::OUTPUTS`] that appear in
    /// `outputs`:
    ///
    /// - [`Output::SolidTemperature`]: absolute temperature in K at the mesh vertices, in vertex
    ///   order.
    /// - [`Output::SolidHeatFlux`]: the vector `-k grad T` in W/m² at the cell centroids, in cell
    ///   order, with global components.
    /// - [`Output::BoundaryHeatRate`]: integrated heat rate in W for every named exterior region of
    ///   the mesh, positive into the solid.
    ///
    /// Other entries of `outputs` are ignored. `settings` is not validated here; invalid settings
    /// stop the stage with the `numerical_breakdown` reason at iteration zero.
    ///
    /// Fails with the errors of [`HeatConduction::validate`], with [`Error::NonFiniteResult`] if an
    /// assembled or recovered value is not finite, and with [`Error::InvalidModel`] if a solved
    /// temperature is not above absolute zero.
    pub fn solve(
        &self,
        settings: LinearSettings,
        outputs: &[Output],
    ) -> Result<(MeshMetrics, Solution<Convergence>), Error> {
        let mesh = self.checked_mesh()?;
        let (solution, _) = self.thermal.prepare(&mesh)?.solve(settings, outputs)?;
        Ok((mesh.into_metrics(), solution))
    }

    fn checked_mesh(&self) -> Result<CheckedTetraMesh<'_>, Error> {
        self.geometry.validate("physics.geometry")?;
        self.mesh.checked("physics.mesh")
    }
}

impl MultimaterialHeatConduction {
    /// Outputs this case can report: nodal temperature, cell heat flux, heat rate by exterior
    /// region and heat rate by volume-region interface.
    pub const OUTPUTS: &[Output] = &[
        Output::SolidTemperature,
        Output::SolidHeatFlux,
        Output::BoundaryHeatRate,
        Output::InterfaceHeatRate,
    ];

    /// Validates the geometry identity, the mesh, the materials, the volume regions and the
    /// boundary conditions without solving.
    ///
    /// Requires at least one material, with unique nonempty IDs, positive finite conductivities and
    /// nonempty citations and usage terms; volume regions with unique nonempty names, a known
    /// `material_id`, a finite heat source and a nonempty cell list; every mesh cell assigned to
    /// exactly one region; and boundary conditions that satisfy the rules of
    /// [`SolidThermal::prepare`]. Returns the mesh metrics, or the first [`Error::InvalidModel`]
    /// found, with a path under `physics.geometry`, `physics.mesh` or `physics.thermal`.
    pub fn validate(&self) -> Result<MeshMetrics, Error> {
        self.geometry.validate("physics.geometry")?;
        let mesh = self.mesh.checked("physics.mesh")?;
        self.prepare(&mesh)?;
        Ok(mesh.into_metrics())
    }

    /// Validates the case, solves for the temperature and recovers heat rates per exterior region,
    /// volume region and interface.
    ///
    /// The temperature solve and its outputs are those of [`HeatConduction::solve`], with each cell
    /// using the conductivity and heat source of its volume region. In addition to the global
    /// `solid_energy` balance, the solution carries one `solid_energy:<name>` balance in W per
    /// volume region with the terms `volumetric_generation`, `exterior_heat_input` and
    /// `interface_heat_input`, each positive into the region.
    ///
    /// The power across each pair of adjoining regions is first integrated from the mean normal
    /// flux of the two cells at every shared face. If at least one such pair exists, a second
    /// stage, `solid_interface_equilibration`, corrects these powers under the same `settings` so
    /// that every regional balance closes up to the mean imbalance over all regions.
    /// [`Output::InterfaceHeatRate`] reports one value in W per pair, with the two region names in
    /// lexicographic order and positive from the first to the second.
    ///
    /// If either stage does not converge, the solution carries only the convergence stages that
    /// ran. Fails with the errors of [`MultimaterialHeatConduction::validate`] and with the
    /// calculation errors listed on [`HeatConduction::solve`].
    pub fn solve(
        &self,
        settings: LinearSettings,
        outputs: &[Output],
    ) -> Result<(MeshMetrics, Solution<Convergence>), Error> {
        self.geometry.validate("physics.geometry")?;
        let (mesh, faces) = self.mesh.checked_with_face_adjacency("physics.mesh")?;
        let (solution, _) =
            self.prepare(&mesh)?
                .solve_with_face_adjacency(settings, outputs, Some(&faces))?;
        Ok((mesh.into_metrics(), solution))
    }

    fn prepare<'a>(
        &'a self,
        checked: &'a CheckedTetraMesh<'a>,
    ) -> Result<PreparedConduction<'a>, Error> {
        PreparedConduction::new(
            checked,
            &self.thermal.boundary_conditions,
            CellMaterials::ByRegion(self.thermal.resolve(checked.mesh().cells.len())?),
        )
    }
}

impl SolidThermal {
    /// Outputs a homogeneous conduction solve can report: nodal temperature, cell heat flux and
    /// heat rate by exterior region.
    pub const OUTPUTS: &[Output] = &[
        Output::SolidTemperature,
        Output::SolidHeatFlux,
        Output::BoundaryHeatRate,
    ];
    const METHOD: &str = "steady_isotropic_heat_conduction_p1_tetrahedral_fem";

    /// Validates this thermal definition against a checked mesh and binds the two into a solvable
    /// problem.
    ///
    /// Checks the material (nonempty ID, positive finite conductivity, nonempty citation and usage
    /// terms), that the heat source is finite, and every boundary condition. A condition's region
    /// must be an exterior region of the mesh that has no earlier condition, and its values must
    /// satisfy the ranges given on [`ThermalBoundary`]. Prescribed temperatures must agree at
    /// shared vertices. A convection region must have a positive area and a positive finite
    /// conductance on every face. At least one `temperature` or `convection` condition is required.
    ///
    /// Returns the first [`Error::InvalidModel`] found, with a path under `physics.thermal`.
    pub fn prepare<'a>(
        &'a self,
        checked: &'a CheckedTetraMesh<'a>,
    ) -> Result<PreparedConduction<'a>, Error> {
        volume::validate_material("physics.thermal.material", &self.material)?;
        finite(
            "physics.thermal.volumetric_heat_source_w_m3",
            self.volumetric_heat_source_w_m3,
        )?;
        PreparedConduction::new(
            checked,
            &self.boundary_conditions,
            CellMaterials::Uniform(CellProperties {
                conductivity: self.material.thermal_conductivity_w_m_k,
                generation: self.volumetric_heat_source_w_m3,
            }),
        )
    }
}

impl<'a> PreparedConduction<'a> {
    fn new(
        checked: &'a CheckedTetraMesh<'a>,
        boundary_conditions: &'a [ThermalBoundary],
        materials: CellMaterials<'a>,
    ) -> Result<Self, Error> {
        let (mesh, topology) = (checked.mesh(), checked.topology());
        let mut prescribed = vec![None; mesh.vertices_m.len()];
        let mut conditions = BTreeMap::new();
        let mut fixed_datum = None;
        let mut ambient_datum = None;
        for (i, condition) in boundary_conditions.iter().enumerate() {
            let path = format!("physics.thermal.boundary_conditions[{i}]");
            let region = condition.region();
            let faces = topology.boundaries.get(region).ok_or_else(|| {
                Error::invalid(
                    format!("{path}.region"),
                    format!("unknown exterior region {region:?}"),
                )
            })?;
            if conditions.insert(region, condition).is_some() {
                return Err(Error::invalid(
                    format!("{path}.region"),
                    format!("region {region:?} already has a thermal boundary condition"),
                ));
            }
            match *condition {
                ThermalBoundary::Temperature { temperature_k, .. } => {
                    positive(&format!("{path}.temperature_k"), temperature_k)?;
                    for node in faces.iter().flat_map(|face| face.nodes) {
                        if prescribed[node].is_some_and(|other| other != temperature_k) {
                            return Err(Error::invalid(
                                format!("{path}.temperature_k"),
                                format!("conflicting prescribed temperatures at vertex {node}"),
                            ));
                        }
                        prescribed[node] = Some(temperature_k);
                    }
                    fixed_datum.get_or_insert(temperature_k);
                }
                ThermalBoundary::HeatFlux {
                    inward_heat_flux_w_m2,
                    ..
                } => finite(
                    &format!("{path}.inward_heat_flux_w_m2"),
                    inward_heat_flux_w_m2,
                )?,
                ThermalBoundary::Insulation { .. } => {}
                ThermalBoundary::Convection {
                    ambient_temperature_k,
                    film_coefficient_w_m2_k,
                    ..
                } => {
                    positive(
                        &format!("{path}.ambient_temperature_k"),
                        ambient_temperature_k,
                    )?;
                    positive(
                        &format!("{path}.film_coefficient_w_m2_k"),
                        film_coefficient_w_m2_k,
                    )?;
                    positive(
                        &format!("{path} convection surface area"),
                        faces.iter().map(|face| face.area).sum(),
                    )?;
                    if faces.iter().any(|face| {
                        let weight = film_weight(face.area, film_coefficient_w_m2_k);
                        !weight.is_finite() || weight <= 0.0
                    }) {
                        return Err(Error::invalid(
                            path,
                            "convection face conductance must be positive and finite",
                        ));
                    }
                    ambient_datum.get_or_insert(ambient_temperature_k);
                }
            }
        }
        let datum_k = match (fixed_datum, ambient_datum) {
            (Some(k), _) => k,
            (None, Some(ambient)) => convection_datum(ambient, &materials, topology, &conditions),
            (None, None) => {
                return Err(Error::invalid(
                    "physics.thermal.boundary_conditions",
                    "a prescribed temperature or positive convection is required to determine absolute temperature",
                ));
            }
        };
        Ok(Self {
            mesh,
            topology,
            materials,
            conditions,
            prescribed,
            datum_k,
        })
    }
    /// Assembles and solves the conduction system and reports the requested outputs.
    ///
    /// Returns the [`Solution`] described on [`HeatConduction::solve`] together with the nodal
    /// temperatures as a [`SolidTemperature`], for use by a coupled solve. The temperatures are
    /// `None`, and the solution carries only its convergence stage, if the linear solve does not
    /// converge. Fails with [`Error::NonFiniteResult`] if an assembled or recovered value is not
    /// finite, and with [`Error::InvalidModel`] if a solved temperature is not above absolute zero.
    pub fn solve(
        &self,
        settings: LinearSettings,
        outputs: &[Output],
    ) -> Result<(Solution<Convergence>, Option<SolidTemperature>), Error> {
        self.solve_with_face_adjacency(settings, outputs, None)
    }

    fn solve_with_face_adjacency(
        &self,
        settings: LinearSettings,
        outputs: &[Output],
        faces: Option<&TetraFaceAdjacency<'_>>,
    ) -> Result<(Solution<Convergence>, Option<SolidTemperature>), Error> {
        let loads = self.assemble_loads()?;
        let prescribed = self
            .prescribed
            .iter()
            .map(|value| value.map(|k| k - self.datum_k))
            .collect();
        let mut system = ConstrainedSystem::new(prescribed, &loads);
        for (cell, (tet, nodes)) in self
            .topology
            .elements
            .iter()
            .zip(&self.mesh.cells)
            .enumerate()
        {
            let k = self.materials.at(cell).conductivity;
            for (a, &row) in nodes.iter().enumerate() {
                for (b, &column) in nodes.iter().enumerate() {
                    let entry = (tet.volume * dot(tet.gradients[a], tet.gradients[b])) * k;
                    if !entry.is_finite() {
                        return Err(nonfinite("solid thermal stiffness"));
                    }
                    system.add(row, column, entry);
                }
            }
        }
        for (&region, &condition) in &self.conditions {
            if let ThermalBoundary::Convection {
                film_coefficient_w_m2_k,
                ..
            } = condition
            {
                for face in &self.topology.boundaries[region] {
                    let weight = film_weight(face.area, *film_coefficient_w_m2_k);
                    for row in face.nodes {
                        for column in face.nodes {
                            system.add(
                                row,
                                column,
                                if row == column { 2.0 * weight } else { weight },
                            );
                        }
                    }
                }
            }
        }

        let (offsets_k, convergence) = system.solve(settings, "solid_heat_conduction", "W");
        let converged = convergence.stop_reason == StopReason::Converged;
        let mut solution = Solution::new(SolidThermal::METHOD, vec![convergence]);
        if !converged {
            return Ok((solution, None));
        }
        let mut temperatures_k = Vec::new();
        let want_temperature = outputs.contains(&Output::SolidTemperature);
        for (node, (&offset, prescribed)) in offsets_k.iter().zip(&self.prescribed).enumerate() {
            let temperature = prescribed.unwrap_or(self.datum_k + offset);
            if !temperature.is_finite() {
                return Err(nonfinite("solid temperature"));
            }
            if temperature <= 0.0 {
                return Err(Error::invalid(
                    format!("solid temperature at vertex {node}"),
                    "solution must be above absolute zero",
                ));
            }
            if want_temperature {
                temperatures_k.push(temperature);
            }
        }
        if want_temperature {
            solution.quantities.insert(
                Output::SolidTemperature,
                Quantity::field(
                    "K",
                    SampleLocation::Node,
                    self.mesh.vertices_m.clone(),
                    temperatures_k,
                ),
            );
        }
        self.heat_rates(&offsets_k, outputs, settings, faces, &mut solution)?;
        if solution
            .convergence
            .last()
            .is_some_and(|stage| stage.stop_reason != StopReason::Converged)
        {
            solution.balances.clear();
            solution.quantities.clear();
            return Ok((solution, None));
        }
        Ok((
            solution,
            Some(SolidTemperature {
                datum_k: self.datum_k,
                offsets_k,
            }),
        ))
    }

    fn assemble_loads(&self) -> Result<Vec<f64>, Error> {
        let mut loads = vec![0.0; self.prescribed.len()];
        for (cell, (tet, nodes)) in self
            .topology
            .elements
            .iter()
            .zip(&self.mesh.cells)
            .enumerate()
        {
            let generation = self.materials.at(cell).generation;
            for &node in nodes {
                loads[node] += (tet.volume / 4.0) * generation;
            }
        }
        for (&region, &condition) in &self.conditions {
            for face in &self.topology.boundaries[region] {
                let load = match *condition {
                    ThermalBoundary::HeatFlux {
                        inward_heat_flux_w_m2,
                        ..
                    } => (face.area / 3.0) * inward_heat_flux_w_m2,
                    ThermalBoundary::Convection {
                        ambient_temperature_k,
                        film_coefficient_w_m2_k,
                        ..
                    } => {
                        4.0 * film_weight(face.area, film_coefficient_w_m2_k)
                            * (ambient_temperature_k - self.datum_k)
                    }
                    _ => continue,
                };
                for node in face.nodes {
                    loads[node] += load;
                }
            }
        }
        if loads.iter().all(|value| value.is_finite()) {
            Ok(loads)
        } else {
            Err(nonfinite("solid thermal loads"))
        }
    }

    fn heat_rates(
        &self,
        offsets_k: &[f64],
        outputs: &[Output],
        settings: LinearSettings,
        faces: Option<&TetraFaceAdjacency<'_>>,
        solution: &mut Solution<Convergence>,
    ) -> Result<(), Error> {
        let mut regional = match (&self.materials, faces) {
            (CellMaterials::ByRegion(materials), Some(faces)) => {
                Some(RegionEnergy::new(materials, faces))
            }
            _ => None,
        };
        let want_flux = outputs.contains(&Output::SolidHeatFlux);
        let mut fluxes = Vec::new();
        let mut reactions = vec![0.0; self.prescribed.len()];
        let mut generation_w = 0.0;
        for (cell, (tet, &nodes)) in self
            .topology
            .elements
            .iter()
            .zip(&self.mesh.cells)
            .enumerate()
        {
            let material = self.materials.at(cell);
            let flux = heat_flux(
                tet,
                material.conductivity,
                nodes.map(|node| offsets_k[node]),
            );
            if !flux.iter().all(|component| component.is_finite()) {
                return Err(nonfinite("solid heat flux"));
            }
            let generated = tet.volume * material.generation;
            generation_w += generated;
            if let Some(regional) = &mut regional {
                regional.add_generation(cell, generated);
            }
            for (a, node) in nodes.into_iter().enumerate() {
                if self.prescribed[node].is_some() {
                    reactions[node] -= tet.volume * dot(tet.gradients[a], flux) + 0.25 * generated;
                }
            }
            if want_flux || regional.is_some() {
                fluxes.push(flux);
            }
        }

        let mut flux_w = 0.0;
        let mut convection_w = 0.0;
        let mut regions = BTreeMap::new();
        let want_rates = outputs.contains(&Output::BoundaryHeatRate);
        for (&region, faces) in &self.topology.boundaries {
            let mut power = 0.0;
            match self.conditions.get(region).copied() {
                Some(&ThermalBoundary::HeatFlux {
                    inward_heat_flux_w_m2,
                    ..
                }) => {
                    for (i, face) in faces.iter().enumerate() {
                        let load = (face.area / 3.0) * inward_heat_flux_w_m2;
                        for node in face.nodes {
                            reactions[node] -= load;
                        }
                        power += 3.0 * load;
                        if let Some(regional) = &mut regional {
                            regional.add_exterior_heat(region, i, 3.0 * load);
                        }
                    }
                    flux_w += power;
                }
                Some(&ThermalBoundary::Convection {
                    ambient_temperature_k,
                    film_coefficient_w_m2_k,
                    ..
                }) => {
                    let ambient_offset = ambient_temperature_k - self.datum_k;
                    for (i, face) in faces.iter().enumerate() {
                        let weight = film_weight(face.area, film_coefficient_w_m2_k);
                        let delta = face
                            .nodes
                            .map(|node| weight * (ambient_offset - offsets_k[node]));
                        let sum: f64 = delta.into_iter().sum();
                        for (node, own) in face.nodes.into_iter().zip(delta) {
                            reactions[node] -= sum + own;
                        }
                        power += 4.0 * sum;
                        if let Some(regional) = &mut regional {
                            regional.add_exterior_heat(region, i, 4.0 * sum);
                        }
                    }
                    convection_w += power;
                }
                _ => {}
            }
            if want_rates {
                regions.insert(region.to_owned(), power);
            }
        }

        let reaction_w = reactions
            .iter()
            .zip(&self.prescribed)
            .filter_map(|(&heat, value)| value.is_some().then_some(heat))
            .sum();
        solution.balances.push(Balance::new(
            "solid_energy",
            "W",
            [
                ("volumetric_generation", generation_w),
                ("prescribed_heat_flux", flux_w),
                ("convection", convection_w),
                ("prescribed_temperature_reaction", reaction_w),
            ],
        )?);
        if want_rates || regional.is_some() {
            self.distribute_reactions(&reactions, &mut regions, regional.as_mut(), &fluxes)?;
        }
        if want_rates {
            solution
                .quantities
                .insert(Output::BoundaryHeatRate, Quantity::by_region("W", regions));
        }
        if let Some(regional) = regional {
            regional.report(&fluxes, settings, outputs, solution)?;
        }
        if want_flux {
            solution.quantities.insert(
                Output::SolidHeatFlux,
                Quantity::VectorField {
                    unit: "W/m^2".to_owned(),
                    location: SampleLocation::Cell,
                    positions_m: self
                        .topology
                        .elements
                        .iter()
                        .map(|tet| tet.centroid)
                        .collect(),
                    values: fluxes,
                },
            );
        }
        Ok(())
    }

    fn distribute_reactions(
        &self,
        reactions: &[f64],
        regions: &mut BTreeMap<String, f64>,
        mut regional: Option<&mut RegionEnergy<'_>>,
        fluxes: &[[f64; 3]],
    ) -> Result<(), Error> {
        let fixed_regions = self
            .conditions
            .iter()
            .filter(|(_, condition)| matches!(condition, ThermalBoundary::Temperature { .. }))
            .map(|(&region, _)| (region, &self.topology.boundaries[region]));
        // On a multimaterial boundary use the cell flux to distinguish faces of
        // different conductivities at a fixed vertex. Share just the mismatch by area.
        let trial_load = |energy: &RegionEnergy<'_>, region, i, face: &Triangle| {
            let cell = energy.exterior_cell(region, i);
            -(face.area / 3.0) * dot(fluxes[cell], face.outward)
        };
        let mut mismatch = regional.as_ref().map(|_| reactions.to_vec());
        let mut nodal_area = vec![0.0; self.prescribed.len()];
        for (region, faces) in fixed_regions.clone() {
            for (i, face) in faces.iter().enumerate() {
                if let Some((energy, mismatch)) = regional.as_deref().zip(mismatch.as_mut()) {
                    let trial = trial_load(energy, region, i, face);
                    for node in face.nodes {
                        mismatch[node] -= trial;
                    }
                }
                for node in face.nodes {
                    nodal_area[node] += face.area;
                }
            }
        }
        if !nodal_area.iter().all(|area| area.is_finite()) {
            return Err(nonfinite("prescribed-temperature tributary area"));
        }
        if mismatch.iter().flatten().any(|power| !power.is_finite()) {
            return Err(nonfinite("prescribed-temperature face reactions"));
        }
        for (region, faces) in fixed_regions {
            let mut heat = 0.0;
            for (i, face) in faces.iter().enumerate() {
                if let Some((energy, mismatch)) = regional.as_deref_mut().zip(mismatch.as_ref()) {
                    let trial = trial_load(energy, region, i, face);
                    let face_power = face
                        .nodes
                        .into_iter()
                        .map(|node| trial + mismatch[node] * (face.area / nodal_area[node]))
                        .sum();
                    energy.add_exterior_heat(region, i, face_power);
                    heat += face_power;
                } else {
                    for node in face.nodes {
                        heat += reactions[node] * (face.area / nodal_area[node]);
                    }
                }
            }
            if let Some(power) = regions.get_mut(region) {
                *power = heat;
            }
        }
        Ok(())
    }
}

fn film_weight(area: f64, coefficient: f64) -> f64 {
    // On a linear triangle, the consistent surface mass diagonal is twice the off-diagonal.
    (area / 12.0) * coefficient
}

// With no fixed-temperature boundary, energy conservation determines the
// h-weighted mean surface temperature. Remove this constant mode before PCG.
fn convection_datum(
    ambient: f64,
    materials: &CellMaterials<'_>,
    topology: &TetraTopology<'_>,
    conditions: &BTreeMap<&str, &ThermalBoundary>,
) -> f64 {
    let mut heat: f64 = topology
        .elements
        .iter()
        .enumerate()
        .map(|(cell, tet)| materials.at(cell).generation * tet.volume)
        .sum();
    let mut conductance = 0.0;
    for (&region, &condition) in conditions {
        for face in &topology.boundaries[region] {
            match *condition {
                ThermalBoundary::HeatFlux {
                    inward_heat_flux_w_m2,
                    ..
                } => {
                    heat += face.area * inward_heat_flux_w_m2;
                }
                ThermalBoundary::Convection {
                    film_coefficient_w_m2_k,
                    ambient_temperature_k,
                    ..
                } => {
                    let h_area = face.area * film_coefficient_w_m2_k;
                    conductance += h_area;
                    heat += h_area * (ambient_temperature_k - ambient);
                }
                _ => {}
            }
        }
    }
    let mean = ambient + heat / conductance;
    if conductance.is_finite() && mean.is_finite() && mean > 0.0 {
        mean
    } else {
        ambient
    }
}

fn heat_flux(tet: &Tet, k: f64, offsets: [f64; 4]) -> [f64; 3] {
    std::array::from_fn(|axis| {
        -k * (1..4)
            .map(|a| (offsets[a] - offsets[0]) * tet.gradients[a][axis])
            .sum::<f64>()
    })
}

fn nonfinite(quantity: &str) -> Error {
    Error::NonFiniteResult {
        quantity: quantity.to_owned(),
    }
}
