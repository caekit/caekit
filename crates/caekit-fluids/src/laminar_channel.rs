//! Fully developed, incompressible flow between two stationary parallel plates.

use serde::{Deserialize, Serialize};

use caekit_mesh::{AxisFrame, GeometryRevision, LineMesh, MeshMetrics};
use caekit_numerics::{Convergence, LinearSettings, SparseAssembly, StopReason};
use caekit_types::error::{nonempty, positive};
use caekit_types::{Balance, Output, Quantity, SampleLocation, Solution};
use caekit_types::{Error, MaterialSource};

/// Steady, fully developed laminar flow of a Newtonian fluid between two stationary parallel
/// plates.
///
/// The fields are the keys of `physics` in a JSON model whose `physics.kind` is `laminar_channel`.
/// Density and viscosity are constant, both plates are no-slip walls, and the flow is driven by a
/// pressure difference or a prescribed volume flow. The axial velocity profile across the gap is
/// solved on the transverse `mesh` with a cell-centred finite-volume method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaminarChannel {
    /// Channel dimensions, placement in the global frame and the names of the inlet and outlet.
    pub geometry: ChannelGeometry,
    /// Transverse mesh across the gap. Its vertices must run from 0 to `geometry.gap_m`, and its
    /// two endpoint regions are the plate walls.
    pub mesh: LineMesh,
    /// Fluid with constant density and dynamic viscosity.
    pub material: NewtonianFluid,
    /// Wall conditions of the two plates: one entry for each endpoint region of `mesh`, in either
    /// order.
    pub walls: [NoSlipWall; 2],
    /// What drives the flow: pressures at both ports, or an inlet volume flow with an outlet
    /// pressure.
    pub drive: FlowDrive,
}

/// Dimensions and placement of a parallel-plate channel in the global Cartesian frame.
///
/// Channel coordinates are the axial distance along `axial_frame` and the transverse distance along
/// `transverse_direction`, both in metres from the frame origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelGeometry {
    /// Identifier and revision of the geometry these dimensions and region names belong to.
    pub identity: GeometryRevision,
    /// Origin of the channel coordinates and unit direction of the flow axis in the global frame.
    /// The origin is at the inlet end (axial coordinate 0) on the wall at transverse coordinate 0,
    /// and the direction points from the inlet to the outlet.
    pub axial_frame: AxisFrame,
    /// Unit vector across the gap as global Cartesian components, pointing from the wall at the
    /// first mesh vertex toward the wall at the last. Its norm must equal one to within 1e-12, and
    /// its dot product with the axial direction must not exceed 1e-12 in magnitude.
    pub transverse_direction: [f64; 3],
    /// Axial length of the channel in metres, over which the pressure loss occurs; positive and
    /// finite.
    pub length_m: f64,
    /// Distance between the two plates in metres; positive and finite. The transverse mesh must end
    /// exactly at this value.
    pub gap_m: f64,
    /// Spanwise width in metres; positive and finite. It scales the two-dimensional solution to
    /// flow rates and forces; side walls are not modelled.
    pub width_m: f64,
    /// Name of the inlet port. Must not be blank and must differ from `outlet_region` and from both
    /// wall region names of the mesh.
    pub inlet_region: String,
    /// Name of the outlet port. Must not be blank and must differ from `inlet_region` and from both
    /// wall region names of the mesh.
    pub outlet_region: String,
}

/// Largest accepted magnitude of the dot product of the axial and transverse directions.
///
/// Dimensionless (the cosine of the angle between two directions that are unit to within
/// 1e-12). Reason: it is the same bound as the unit-norm check of `AxisFrame` in `caekit-mesh`,
/// four orders above the binary64 rounding of a three-term dot product of unit vectors (a few
/// times 1.1e-16). Components written to 12 decimals are off by at most `ε = 5e-13`, which
/// moves the dot product `a · b` by at most `(‖a‖₁ + ‖b‖₁) ε` to first order: `1e-12` for
/// directions along the coordinate axes (`‖·‖₁ = 1`), which are accepted, and up to
/// `2 √3 ε = 1.7e-12` for diagonal directions, which need a 13th decimal.
const PERPENDICULAR_TOLERANCE: f64 = 1.0e-12;

impl ChannelGeometry {
    fn validate(&self, path: &str) -> Result<(), Error> {
        self.identity.validate(&format!("{path}.identity"))?;
        self.axial_frame.validate(&format!("{path}.axial_frame"))?;
        AxisFrame {
            origin_m: [0.0; 3],
            direction: self.transverse_direction,
        }
        .validate(&format!("{path}.transverse_direction"))?;
        let projection: f64 = self
            .axial_frame
            .direction
            .iter()
            .zip(self.transverse_direction)
            .map(|(a, b)| a * b)
            .sum();
        if projection.abs() > PERPENDICULAR_TOLERANCE {
            return Err(Error::invalid(path, "channel axes must be perpendicular"));
        }
        positive(&format!("{path}.length_m"), self.length_m)?;
        positive(&format!("{path}.gap_m"), self.gap_m)?;
        positive(&format!("{path}.width_m"), self.width_m)?;
        nonempty(&format!("{path}.inlet_region"), &self.inlet_region)?;
        nonempty(&format!("{path}.outlet_region"), &self.outlet_region)?;
        if self.inlet_region == self.outlet_region {
            return Err(Error::invalid(path, "inlet and outlet regions must differ"));
        }
        Ok(())
    }

    /// Maps channel coordinates to the global frame.
    ///
    /// Returns, in metres, the frame origin plus `axial_m` times the axial direction plus
    /// `transverse_m` times `transverse_direction`. The geometry is used as given; nothing is
    /// validated here.
    pub fn position(&self, axial_m: f64, transverse_m: f64) -> [f64; 3] {
        let origin = self.axial_frame.position(axial_m);
        std::array::from_fn(|i| origin[i] + transverse_m * self.transverse_direction[i])
    }
}

/// Incompressible Newtonian fluid with constant density and dynamic viscosity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewtonianFluid {
    /// Identifier of the fluid; must not be blank.
    pub id: String,
    /// Mass density in kg/m³; positive and finite. It converts volume flow to mass flow and enters
    /// the Reynolds number.
    pub density_kg_m3: f64,
    /// Dynamic viscosity in Pa·s; positive and finite.
    pub dynamic_viscosity_pa_s: f64,
    /// Citation and usage terms of the property values; neither may be blank.
    pub source: MaterialSource,
}

/// Wall condition assigned to one endpoint region of the transverse mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoSlipWall {
    /// Name of the wall: the `start_region` or `end_region` of the channel's transverse mesh. The
    /// two wall entries must name different endpoints.
    pub region: String,
    /// Velocity condition applied on that wall.
    pub condition: WallCondition,
}

/// Velocity condition on a channel wall, spelled in snake_case in JSON.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WallCondition {
    /// Zero fluid velocity at the stationary wall; `no_slip` in JSON.
    NoSlip,
}

/// Driving condition of the channel flow, selected in JSON by the `kind` key in snake_case.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FlowDrive {
    /// Prescribes the pressure at both ports; `kind` is `pressure`. The inlet pressure must exceed
    /// the outlet pressure by a positive, finite difference, which is reported as the pressure
    /// loss.
    Pressure {
        /// Pressure at the inlet; its `region` must equal `geometry.inlet_region`.
        inlet: PressureAtPort,
        /// Pressure at the outlet; its `region` must equal `geometry.outlet_region`.
        outlet: PressureAtPort,
    },
    /// Prescribes the volume flow rate and the outlet pressure; `kind` is `volume_flow`. The
    /// pressure loss and the inlet pressure are computed.
    VolumeFlow {
        /// Name of the inlet; must equal `geometry.inlet_region`.
        inlet_region: String,
        /// Volume flow rate through the channel in m³/s, from inlet to outlet; positive and finite.
        rate_m3_s: f64,
        /// Pressure at the outlet; its `region` must equal `geometry.outlet_region`.
        outlet: PressureAtPort,
    },
}

/// Pressure prescribed on a named inlet or outlet region.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PressureAtPort {
    /// Name of the port; must equal the geometry's inlet or outlet region name, whichever this
    /// entry is given for.
    pub region: String,
    /// Absolute pressure, or a gauge reading with its absolute reference pressure, in Pa. The
    /// reference of a gauge reading and the resulting absolute pressure must be positive and
    /// finite.
    pub pressure: Pressure,
}

pub use caekit_types::Pressure;

impl PressureAtPort {
    fn validate(&self, path: &str, region: &str) -> Result<(), Error> {
        if self.region != region {
            return Err(Error::invalid(
                format!("{path}.region"),
                format!("expected {region:?}"),
            ));
        }
        self.pressure.validate(&format!("{path}.pressure"))
    }
}

/// Converged hydraulic state of a channel, returned by [`LaminarChannel::compute`].
///
/// Holds the cell velocities and centres, volume and mass flow rates, pressure loss, absolute port
/// pressures and wall drag that [`LaminarChannel::add_outputs`] turns into report quantities.
pub struct FlowState {
    velocity_m_s: Vec<f64>,
    centers_m: Vec<f64>,
    volume_flow_m3_s: f64,
    mass_flow_kg_s: f64,
    pressure_loss_pa: f64,
    inlet_absolute_pa: f64,
    outlet_absolute_pa: f64,
    wall_drag_n: [f64; 2],
}

impl FlowState {
    /// Returns the mass flow rate through the channel in kg/s: density times volume flow rate. It
    /// is positive and finite.
    pub fn mass_flow_kg_s(&self) -> f64 {
        self.mass_flow_kg_s
    }
}

impl LaminarChannel {
    /// Output quantities that a model of this case may request.
    pub const OUTPUTS: &[Output] = &[
        Output::AxialVelocity,
        Output::VolumeFlowRate,
        Output::MassFlowRate,
        Output::PressureLoss,
        Output::AbsolutePressure,
        Output::ChannelReynolds,
        Output::WallDrag,
    ];
    const METHOD: &str = "steady_fully_developed_laminar_channel_cell_centered_fvm";

    /// Checks geometry, transverse mesh, fluid, walls and drive, and returns the mesh metrics.
    ///
    /// `path` is the model path of this case, `physics` for a standalone channel; it prefixes error
    /// paths, and the returned [`MeshMetrics::name`] is `{path}.mesh`. Returns
    /// [`Error::InvalidModel`] when:
    ///
    /// - a geometry identity field is blank, a frame vector is not a unit vector, the two axes are
    ///   not perpendicular, a dimension is not positive and finite, or the port names are blank or
    ///   equal;
    /// - the mesh does not run from 0 to `gap_m` in strictly increasing steps, or its wall names
    ///   are blank or equal;
    /// - the fluid `id` or its source is blank, or density or viscosity is not positive and finite;
    /// - the two `walls` entries do not name the two distinct endpoint regions of the mesh;
    /// - a wall region has the same name as the inlet or outlet;
    /// - a drive region does not match the geometry, a pressure is invalid, the inlet pressure does
    ///   not exceed the outlet pressure, or the volume flow rate is not positive and finite.
    pub fn validate(&self, path: &str) -> Result<MeshMetrics, Error> {
        self.geometry.validate(&format!("{path}.geometry"))?;
        let metrics = self
            .mesh
            .validate(&format!("{path}.mesh"), self.geometry.gap_m)?;
        nonempty(&format!("{path}.material.id"), &self.material.id)?;
        positive(
            &format!("{path}.material.density_kg_m3"),
            self.material.density_kg_m3,
        )?;
        positive(
            &format!("{path}.material.dynamic_viscosity_pa_s"),
            self.material.dynamic_viscosity_pa_s,
        )?;
        self.material
            .source
            .validate(&format!("{path}.material.source"))?;
        let [first, second] = &self.walls;
        if first.region == second.region
            || self.mesh.endpoint(&first.region).is_none()
            || self.mesh.endpoint(&second.region).is_none()
        {
            return Err(Error::invalid(
                format!("{path}.walls"),
                "assign a no-slip wall to each transverse mesh endpoint",
            ));
        }
        let inlet_region = &self.geometry.inlet_region;
        let outlet_region = &self.geometry.outlet_region;
        if [&self.mesh.start_region, &self.mesh.end_region]
            .iter()
            .any(|&region| region == inlet_region || region == outlet_region)
        {
            return Err(Error::invalid(
                path,
                "wall and port region names must differ",
            ));
        }
        match &self.drive {
            FlowDrive::Pressure { inlet, outlet } => {
                inlet.validate(&format!("{path}.drive.inlet"), inlet_region)?;
                outlet.validate(&format!("{path}.drive.outlet"), outlet_region)?;
                positive(
                    &format!("{path}.drive.pressure_drop_pa"),
                    inlet.pressure.difference_pa(outlet.pressure),
                )?;
            }
            FlowDrive::VolumeFlow {
                inlet_region: region,
                rate_m3_s,
                outlet,
            } => {
                if region != inlet_region {
                    return Err(Error::invalid(
                        format!("{path}.drive.inlet_region"),
                        format!("expected {inlet_region:?}"),
                    ));
                }
                positive(&format!("{path}.drive.rate_m3_s"), *rate_m3_s)?;
                outlet.validate(&format!("{path}.drive.outlet"), outlet_region)?;
            }
        }
        Ok(metrics)
    }

    /// Solve a validated channel model and construct its standalone outputs.
    ///
    /// Runs [`LaminarChannel::compute`] with `settings` and returns a [`Solution`] with the method
    /// name `steady_fully_developed_laminar_channel_cell_centered_fvm` and the convergence trace of
    /// the momentum stage. If that stage converged, the balances and quantities of
    /// [`LaminarChannel::add_outputs`] are included; otherwise both are empty. The model is not
    /// validated again here; call [`LaminarChannel::validate`] first. Errors are those of `compute`
    /// and `add_outputs`.
    pub fn solve(&self, settings: LinearSettings) -> Result<Solution<Convergence>, Error> {
        let (convergence, state) = self.compute(settings)?;
        let mut result = Solution::new(Self::METHOD, vec![convergence]);
        if let Some(state) = state {
            self.add_outputs(state, &mut result)?;
        }
        Ok(result)
    }

    /// Solve the hydraulic stage of a validated channel model.
    ///
    /// Discretises `−μ u'' = Δp / L` across the gap with cell-centred finite volumes and a no-slip
    /// wall at both ends, and solves the system by Jacobi-preconditioned conjugate gradient under
    /// `settings`. The residual is reported in Pa. With a pressure drive the stage is named
    /// `axial_momentum`. With a volume-flow drive the stage `unit_gradient_axial_momentum` solves
    /// for a pressure gradient of 1 Pa/m; velocity and pressure loss are then scaled so that the
    /// discrete flow integral equals the prescribed rate, and the trace describes the unit-gradient
    /// system.
    ///
    /// Returns the convergence trace and, only when the stage converged, the [`FlowState`]. Returns
    /// [`Error::InvalidModel`] when the unit-gradient flow response, the computed volume flow or
    /// the pressure loss is not positive and finite, or the inlet absolute pressure is not finite,
    /// and [`Error::NonFiniteResult`] when the mass flow is not positive and finite.
    pub fn compute(
        &self,
        settings: LinearSettings,
    ) -> Result<(Convergence, Option<FlowState>), Error> {
        let vertices = &self.mesh.vertices_m;
        let viscosity = self.material.dynamic_viscosity_pa_s;
        let length = self.geometry.length_m;
        let width = self.geometry.width_m;
        let centers: Vec<_> = vertices
            .windows(2)
            .map(|edge| edge[0] + 0.5 * (edge[1] - edge[0]))
            .collect();
        let n = centers.len();
        let (gradient, stage) = match &self.drive {
            FlowDrive::Pressure { inlet, outlet } => (
                inlet.pressure.difference_pa(outlet.pressure) / length,
                "axial_momentum",
            ),
            FlowDrive::VolumeFlow { .. } => (1.0, "unit_gradient_axial_momentum"),
        };

        let mut system = SparseAssembly::new(n);
        let lower_wall_conductance = viscosity / centers[0];
        let upper_wall_conductance = viscosity / (self.geometry.gap_m - centers[n - 1]);
        system.add(0, 0, lower_wall_conductance);
        system.add(n - 1, n - 1, upper_wall_conductance);
        for (i, pair) in centers.windows(2).enumerate() {
            let conductance = viscosity / (pair[1] - pair[0]);
            system.add(i, i, conductance);
            system.add(i, i + 1, -conductance);
            system.add(i + 1, i, -conductance);
            system.add(i + 1, i + 1, conductance);
        }
        let rhs: Vec<_> = vertices
            .windows(2)
            .map(|edge| gradient * (edge[1] - edge[0]))
            .collect();
        let (mut velocity, convergence) = system.solve(&rhs, settings, stage, "Pa");
        if convergence.stop_reason != StopReason::Converged {
            return Ok((convergence, None));
        }
        let integrated_flow = |velocity: &[f64]| -> f64 {
            width
                * vertices
                    .windows(2)
                    .zip(velocity)
                    .map(|(edge, u)| (edge[1] - edge[0]) * u)
                    .sum::<f64>()
        };

        let (pressure_loss, inlet_absolute, outlet_absolute) = match &self.drive {
            FlowDrive::Pressure { inlet, outlet } => (
                inlet.pressure.difference_pa(outlet.pressure),
                inlet.pressure.absolute_pa(),
                outlet.pressure.absolute_pa(),
            ),
            FlowDrive::VolumeFlow {
                rate_m3_s, outlet, ..
            } => {
                let response = integrated_flow(&velocity);
                if !response.is_finite() || response <= 0.0 {
                    return Err(Error::invalid(
                        "physics.drive",
                        "unit pressure gradient did not produce a positive flow response; tighten the linear solver tolerance or check the physical scales",
                    ));
                }
                let scale = rate_m3_s / response;
                velocity.iter_mut().for_each(|u| *u *= scale);
                let pressure_loss = scale * length;
                let outlet_absolute = outlet.pressure.absolute_pa();
                (
                    pressure_loss,
                    outlet_absolute + pressure_loss,
                    outlet_absolute,
                )
            }
        };
        let volume_flow = integrated_flow(&velocity);
        if !volume_flow.is_finite()
            || volume_flow <= 0.0
            || !pressure_loss.is_finite()
            || pressure_loss <= 0.0
            || !inlet_absolute.is_finite()
        {
            return Err(Error::invalid(
                "physics.drive",
                "computed flow and pressure loss must be positive and finite; check tolerances and physical scales",
            ));
        }
        let mass_flow = self.material.density_kg_m3 * volume_flow;
        if !mass_flow.is_finite() || mass_flow <= 0.0 {
            return Err(Error::NonFiniteResult {
                quantity: "mass_flow_rate".to_owned(),
            });
        }
        let wall_area = width * length;
        let state = FlowState {
            volume_flow_m3_s: volume_flow,
            mass_flow_kg_s: mass_flow,
            pressure_loss_pa: pressure_loss,
            inlet_absolute_pa: inlet_absolute,
            outlet_absolute_pa: outlet_absolute,
            wall_drag_n: [
                -lower_wall_conductance * velocity[0] * wall_area,
                -upper_wall_conductance * velocity[n - 1] * wall_area,
            ],
            velocity_m_s: velocity,
            centers_m: centers,
        };
        Ok((convergence, Some(state)))
    }

    /// Adds the channel balances and every channel quantity of a converged flow state to `result`.
    ///
    /// Two balances are appended, each with the signed sum of its terms as imbalance:
    ///
    /// - `channel_mass` in kg/s, with `inflow` positive and `outflow` negative;
    /// - `channel_axial_momentum` in N, with `pressure_force` (the pressure loss times width times
    ///   gap), `lower_wall_drag` for the wall at the first mesh vertex and `upper_wall_drag` for
    ///   the wall at the last.
    ///
    /// All entries of [`LaminarChannel::OUTPUTS`] are inserted:
    ///
    /// - `axial_velocity` in m/s at the cell centres in mesh order, positioned at mid-length in
    ///   global coordinates;
    /// - `volume_flow_rate` in m^3/s, `mass_flow_rate` in kg/s and `pressure_loss` in Pa as
    ///   scalars;
    /// - `channel_reynolds` with unit `1`: mean velocity times the hydraulic diameter `2 * gap_m`,
    ///   divided by the kinematic viscosity;
    /// - `absolute_pressure` in Pa for the inlet and outlet regions;
    /// - `wall_drag` in N for the two wall regions: the axial force of each wall on the fluid,
    ///   negative for flow in the positive axial direction.
    ///
    /// Returns [`Error::NonFiniteResult`] if the sum of a balance is not finite.
    pub fn add_outputs(
        &self,
        state: FlowState,
        result: &mut Solution<Convergence>,
    ) -> Result<(), Error> {
        let width = self.geometry.width_m;
        let gap = self.geometry.gap_m;
        let inv_kinematic_viscosity =
            self.material.density_kg_m3 / self.material.dynamic_viscosity_pa_s;
        // Hydraulic diameter = 2 * gap, mean velocity = volume flow / (width * gap).
        let reynolds = 2.0 * (state.volume_flow_m3_s / width) * inv_kinematic_viscosity;
        result.balances.extend([
            Balance::new(
                "channel_mass",
                "kg/s",
                [
                    ("inflow", state.mass_flow_kg_s),
                    ("outflow", -state.mass_flow_kg_s),
                ],
            )?,
            Balance::new(
                "channel_axial_momentum",
                "N",
                [
                    ("pressure_force", state.pressure_loss_pa * (width * gap)),
                    ("lower_wall_drag", state.wall_drag_n[0]),
                    ("upper_wall_drag", state.wall_drag_n[1]),
                ],
            )?,
        ]);
        result.quantities.extend([
            (
                Output::AxialVelocity,
                Quantity::field(
                    "m/s",
                    SampleLocation::Cell,
                    state
                        .centers_m
                        .iter()
                        .map(|&y| self.geometry.position(0.5 * self.geometry.length_m, y))
                        .collect(),
                    state.velocity_m_s,
                ),
            ),
            (
                Output::VolumeFlowRate,
                Quantity::scalar("m^3/s", state.volume_flow_m3_s),
            ),
            (
                Output::MassFlowRate,
                Quantity::scalar("kg/s", state.mass_flow_kg_s),
            ),
            (
                Output::PressureLoss,
                Quantity::scalar("Pa", state.pressure_loss_pa),
            ),
            (Output::ChannelReynolds, Quantity::scalar("1", reynolds)),
            (
                Output::AbsolutePressure,
                Quantity::by_region(
                    "Pa",
                    [
                        (self.geometry.inlet_region.clone(), state.inlet_absolute_pa),
                        (
                            self.geometry.outlet_region.clone(),
                            state.outlet_absolute_pa,
                        ),
                    ],
                ),
            ),
            (
                Output::WallDrag,
                Quantity::by_region(
                    "N",
                    [
                        (self.mesh.start_region.clone(), state.wall_drag_n[0]),
                        (self.mesh.end_region.clone(), state.wall_drag_n[1]),
                    ],
                ),
            ),
        ]);
        Ok(())
    }
}
