//! Solid heat conduction and conservative bulk coolant energy transport.

pub mod heat_conduction;
mod interface;

use serde::{Deserialize, Serialize};

use caekit_mesh::{GeometryRevision, LineMesh, MeshMetrics};
use caekit_numerics::{Convergence, StopReason, norm};
use caekit_types::error::{nonempty, positive};
use caekit_types::{Balance, Error, MaterialSource};
use interface::overlap;

/// Conductive wall of the `cooling_channel` case, read from `physics.wall` in the JSON model.
///
/// The wall is a row of axial panels, one per cell of `axial_mesh`. Each panel conducts heat in one
/// dimension from a hot face held at a uniform temperature to the wetted face, with the condensed
/// conductance `k * panel area / thickness_m`. There is no axial conduction between panels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConductiveWall {
    /// Identity of the wall geometry that the meshes and regions belong to. It is not checked by
    /// [`ConductiveWall::check_meshes`]; the cooling-channel case validates it.
    pub identity: GeometryRevision,
    /// Wall thickness from the hot face to the wetted face, in m. Must be positive and finite.
    pub thickness_m: f64,
    /// Wall material. Its constant conductivity applies to the whole wall.
    pub material: ConductiveMaterial,
    /// Partition of the wall into panels along the channel axis, in m. The vertices must increase
    /// strictly from exactly 0 to exactly the channel length. The partition may differ from the
    /// coolant's axial mesh.
    pub axial_mesh: LineMesh,
    /// Through-thickness coordinates in m, increasing strictly from exactly 0 at the hot face
    /// (`start_region`) to exactly `thickness_m` at the wetted face (`end_region`). The energy
    /// solve does not use the interior vertices; the cooling-channel case samples the reconstructed
    /// wall temperature at them.
    pub thickness_mesh: LineMesh,
    /// Uniform temperature prescribed on the hot face. Its `region` must equal
    /// `thickness_mesh.start_region`.
    pub hot_boundary: TemperatureBoundary,
}

/// Isotropic solid with a constant thermal conductivity, as written in the JSON model.
///
/// Used for the cooling-channel wall (`physics.wall.material`), for homogeneous conduction
/// (`physics.thermal.material`) and as an entry of `physics.thermal.materials` in the multimaterial
/// case.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConductiveMaterial {
    /// Material identifier. Must contain a non-whitespace character. In a multimaterial model it
    /// must be unique in the material list and is the key that `volume_regions[].material_id`
    /// refers to.
    pub id: String,
    /// Thermal conductivity `k` in W/(m·K), independent of temperature and direction. Must be
    /// positive and finite.
    #[serde(deserialize_with = "deserialize_conductivity")]
    pub thermal_conductivity_w_m_k: f64,
    /// Citation and usage terms of the conductivity value. Both must be nonempty.
    pub source: MaterialSource,
}

fn deserialize_conductivity<'de, D: serde::Deserializer<'de>>(reader: D) -> Result<f64, D::Error> {
    f64::deserialize(reader)
        .map_err(|error| serde::de::Error::custom(format!("thermal_conductivity_w_m_k: {error}")))
}

/// Coolant stream of the `cooling_channel` case, read from `physics.coolant` in the JSON model.
///
/// The stream carries heat by axial advection of bulk enthalpy with a constant specific heat. Each
/// cell of `axial_mesh` is a finite volume with a first-order upwind energy flux. The mass flow is
/// supplied by the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coolant {
    /// Specific heat capacity of the coolant in J/(kg·K), constant along the channel. Must be
    /// positive and finite.
    pub specific_heat_j_kg_k: f64,
    /// Citation and usage terms of the specific heat value. Both must be nonempty.
    pub source: MaterialSource,
    /// Finite-volume cells of the stream along the channel axis, in m, ordered from the inlet
    /// (`start_region`) to the outlet (`end_region`). It is not checked by
    /// [`Coolant::validate_properties`]; the cooling-channel case requires it to span 0 to the
    /// channel length and to use the hydraulic inlet and outlet region names.
    pub axial_mesh: LineMesh,
    /// Bulk temperature of the coolant entering the first cell. Its `region` must equal the
    /// hydraulic inlet region.
    pub inlet: TemperatureBoundary,
}

/// Absolute temperature prescribed on one named boundary region.
///
/// Used for the wall's hot face (`physics.wall.hot_boundary`) and the coolant inlet
/// (`physics.coolant.inlet`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemperatureBoundary {
    /// Name of the boundary region that carries the temperature. It must equal the region the
    /// owning object expects: the start region of the wall thickness mesh, or the hydraulic inlet
    /// region.
    pub region: String,
    /// Absolute temperature in K. Must be positive and finite.
    pub temperature_k: f64,
}

/// Stopping settings for the partitioned wall-coolant energy iteration, read from the optional
/// `physics.coupling_solver` object in the JSON model.
///
/// The iteration converges when the L2 norm of the wall-panel and coolant-cell energy residuals, in
/// W, is at most `max(absolute_residual_tolerance_w, relative_residual_tolerance * initial_norm)`.
/// The initial norm is evaluated with every wetted-face wall temperature and coolant temperature
/// equal to the inlet temperature. When the object is omitted the [`Default`] values apply: 1e-10,
/// 1e-10 W and 1000 iterations. When it is present all three fields are required.
///
/// Reasons of the defaults:
///
/// - Relative tolerance `1e-10`. The same figure as the default of the linear solver (see
///   `caekit_numerics::LinearSettings`, which gives its derivation), so the coupling error is not
///   looser than the algebraic error of the stages it couples.
/// - Absolute tolerance `1e-10` W. A floor for a start so close to the converged state that the
///   relative threshold falls below what the residual can resolve: each entry of the residual is
///   a heat rate of at most the initial norm of 8.4e3 W in the reference model, which rounds at
///   about 2.2e-16 * 8.4e3 = 1.9e-12 W, so the floor is about 50 times that and stays
///   attainable.
///   The reference model's threshold is `1e-10 * 8351.6 W = 8.35e-7 W`, far above the floor.
/// - Sweep limit `1000`. The reference model contracts the residual by a factor of 0.036 per
///   sweep (geometric mean of the nine entries of `residual_l2_history` of the
///   `coupled_wall_coolant_energy` stage in the output of
///   `cargo run -p caekit-cli -- run examples/cooling_channel.json`) and converges in 8 sweeps. A
///   linearly convergent iteration with contraction factor `ρ` reaches `1e-10` within 1000
///   sweeps whenever `ρ <= 10^(-10/1000) = 0.977`, so the limit only stops an iteration that
///   is close to not contracting at all.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CouplingSettings {
    /// Factor applied to the initial residual norm to form the threshold. Must be greater than 0
    /// and less than 1.
    pub relative_residual_tolerance: f64,
    /// Lower bound of the threshold, in W. Must be finite and nonnegative.
    pub absolute_residual_tolerance_w: f64,
    /// Maximum number of sweeps; one sweep updates every wall panel and then every coolant cell.
    /// Must be at least 1. Reaching it without convergence stops with the `iteration_limit` reason.
    pub max_iterations: usize,
}

impl Default for CouplingSettings {
    fn default() -> Self {
        Self {
            relative_residual_tolerance: 1.0e-10,
            absolute_residual_tolerance_w: 1.0e-10,
            max_iterations: 1000,
        }
    }
}

impl ConductiveWall {
    /// Validates the wall's thickness, material, meshes and hot boundary for a channel of length
    /// `length` in m.
    ///
    /// Checks that `thickness_m` and the conductivity are positive and finite, that the material
    /// ID, citation and usage terms are nonempty, that `axial_mesh` spans 0 to `length` and
    /// `thickness_mesh` spans 0 to `thickness_m` with at least two strictly increasing vertices and
    /// distinct nonempty endpoint region names, and that `hot_boundary` names
    /// `thickness_mesh.start_region` with a positive finite temperature. `identity` is not checked.
    ///
    /// Returns the metrics of the axial mesh and of the thickness mesh, in that order, or the first
    /// [`Error::InvalidModel`] found, with a path under `physics.wall`.
    pub fn check_meshes(&self, length: f64) -> Result<[MeshMetrics; 2], Error> {
        positive("physics.wall.thickness_m", self.thickness_m)?;
        nonempty("physics.wall.material.id", &self.material.id)?;
        positive(
            "physics.wall.material.thermal_conductivity_w_m_k",
            self.material.thermal_conductivity_w_m_k,
        )?;
        self.material
            .source
            .validate("physics.wall.material.source")?;
        let wall = self
            .axial_mesh
            .validate("physics.wall.axial_mesh", length)?;
        let thickness = self
            .thickness_mesh
            .validate("physics.wall.thickness_mesh", self.thickness_m)?;
        self.hot_boundary.validate(
            "physics.wall.hot_boundary",
            &self.thickness_mesh.start_region,
        )?;
        Ok([wall, thickness])
    }
}

impl Coolant {
    /// Validates the coolant's inlet condition and properties. `axial_mesh` is not checked.
    ///
    /// Requires `inlet.region` to equal `inlet_region` (the hydraulic inlet region of the channel),
    /// the inlet temperature and the specific heat to be positive and finite, and the citation and
    /// usage terms to be nonempty. Returns the first [`Error::InvalidModel`] found, with a path
    /// under `physics.coolant`.
    pub fn validate_properties(&self, inlet_region: &str) -> Result<(), Error> {
        self.inlet.validate("physics.coolant.inlet", inlet_region)?;
        positive(
            "physics.coolant.specific_heat_j_kg_k",
            self.specific_heat_j_kg_k,
        )?;
        self.source.validate("physics.coolant.source")
    }
}

impl TemperatureBoundary {
    fn validate(&self, path: &str, region: &str) -> Result<(), Error> {
        if self.region != region {
            return Err(Error::invalid(
                format!("{path}.region"),
                format!("expected {region:?}"),
            ));
        }
        positive(&format!("{path}.temperature_k"), self.temperature_k)
    }
}

impl CouplingSettings {
    /// Checks each setting against its allowed range.
    ///
    /// Returns an [`Error::InvalidModel`] with a path under `physics.coupling_solver` for the first
    /// violation: a relative tolerance outside the open interval (0, 1), an absolute tolerance that
    /// is negative or not finite, or a zero iteration limit.
    pub fn validate(self) -> Result<(), Error> {
        if !(0.0 < self.relative_residual_tolerance && self.relative_residual_tolerance < 1.0) {
            return Err(Error::invalid(
                "physics.coupling_solver.relative_residual_tolerance",
                "must be greater than zero and less than one",
            ));
        }
        if !(self.absolute_residual_tolerance_w.is_finite()
            && self.absolute_residual_tolerance_w >= 0.0)
        {
            return Err(Error::invalid(
                "physics.coupling_solver.absolute_residual_tolerance_w",
                "must be nonnegative and finite",
            ));
        }
        if self.max_iterations == 0 {
            return Err(Error::invalid(
                "physics.coupling_solver.max_iterations",
                "must be positive",
            ));
        }
        Ok(())
    }
}

/// Thermal state in mesh order, with temperatures expressed relative to the coolant inlet.
pub struct ChannelEnergy {
    /// Wall temperature at the wetted face minus the coolant inlet temperature, in K, one value per
    /// cell of the wall's axial mesh.
    pub wall_temperature_offset_k: Vec<f64>,
    /// Coolant bulk temperature minus the inlet temperature, in K, one value per cell of the
    /// coolant's axial mesh from inlet to outlet. The last value is the outlet temperature rise.
    pub fluid_temperature_offset_k: Vec<f64>,
    /// Heat flux leaving the wetted face of each wall panel, in W/m²: the panel's heat rate to the
    /// coolant divided by its area (channel width times cell length). Positive toward the coolant.
    pub wall_heat_flux_w_m2: Vec<f64>,
    /// Total heat rate entering the coolant through the film, in W, summed over all wall-coolant
    /// overlaps.
    pub heat_to_coolant_w: f64,
    /// Global energy balances in W, in the order `wall_energy`, `interface_energy`,
    /// `coolant_energy` and `system_energy`. Each holds signed terms and their sum as the
    /// imbalance.
    pub balances: Vec<Balance>,
}

/// Solve a validated wall and coolant with a supplied mass flow and film coefficient.
/// Their axial meshes must cover the same interval.
///
/// `width_m` is the channel width in m, `mass_flow_kg_s` the coolant mass flow in kg/s and
/// `film_coefficient_w_m2_k` the fluid-side film coefficient `h` in W/(m²·K). Each overlap of a
/// wall panel and a coolant cell transfers `h * width_m * overlap_length * (T_wall - T_fluid)`, the
/// same heat rate on both sides. Partitioned Gauss-Seidel alternates the wall-panel update with a
/// downstream sweep of the coolant cells, starting with every wetted-face wall temperature and
/// coolant temperature at the inlet temperature.
///
/// The returned [`Convergence`] is the `coupled_wall_coolant_energy` stage with residuals in W; see
/// [`CouplingSettings`] for its threshold. The [`ChannelEnergy`] is `Some` only when the stage
/// converged. It is `None` at the iteration limit, or with the `numerical_breakdown` reason when a
/// residual is not finite.
///
/// Returns an error if the heat capacity flow rate `mass_flow_kg_s * specific_heat_j_kg_k`, a
/// wall-panel conductance or the film conductance of any overlap is not positive and finite, or if
/// a sum of conductances or an energy balance is not finite. `settings` is used as given; validate
/// it with [`CouplingSettings::validate`] first.
pub fn solve_channel_energy(
    wall: &ConductiveWall,
    coolant: &Coolant,
    width_m: f64,
    mass_flow_kg_s: f64,
    film_coefficient_w_m2_k: f64,
    settings: CouplingSettings,
) -> Result<(Convergence, Option<ChannelEnergy>), Error> {
    let system = EnergySystem::new(
        wall,
        coolant,
        width_m,
        mass_flow_kg_s,
        film_coefficient_w_m2_k,
    )?;
    let (convergence, temperatures) = system.solve(settings);
    let energy = temperatures
        .map(|state| system.summary(state))
        .transpose()?;
    Ok((convergence, energy))
}

struct ThermalFace {
    wall: usize,
    fluid: usize,
    coefficient_w_k: f64,
}

struct EnergySystem {
    faces: Vec<ThermalFace>,
    capacity: f64,
    hot_delta: f64,
    wall_area: Vec<f64>,
    conductance: Vec<f64>,
    wall_film: Vec<f64>,
    fluid_film: Vec<f64>,
}

struct Temperatures {
    wall: Vec<f64>,
    fluid: Vec<f64>,
}

struct InterfaceFlux {
    from_wall: Vec<f64>,
    into_fluid: Vec<f64>,
}

impl EnergySystem {
    fn new(
        solid: &ConductiveWall,
        coolant: &Coolant,
        width: f64,
        mass_flow_kg_s: f64,
        h: f64,
    ) -> Result<Self, Error> {
        let wall = &solid.axial_mesh.vertices_m;
        let fluid = &coolant.axial_mesh.vertices_m;
        let capacity = mass_flow_kg_s * coolant.specific_heat_j_kg_k;
        positive("coolant heat capacity flow rate", capacity)?;

        let wall_area: Vec<_> = wall
            .windows(2)
            .map(|edge| width * (edge[1] - edge[0]))
            .collect();
        let coefficient = solid.material.thermal_conductivity_w_m_k / solid.thickness_m;
        // Condensed P1 conduction column. Uniform k and zero volumetric heat source
        // give a linear profile regardless of the normal element spacing.
        let conductance: Vec<_> = wall_area.iter().map(|area| coefficient * area).collect();
        let mut wall_film = vec![0.0; wall.len() - 1];
        let mut fluid_film = vec![0.0; fluid.len() - 1];
        let mut faces = Vec::new();
        for face in overlap(wall, fluid) {
            let coefficient = h * (width * face.length_m);
            positive(
                "interface face heat transfer coefficient times area",
                coefficient,
            )?;
            wall_film[face.left] += coefficient;
            fluid_film[face.right] += coefficient;
            faces.push(ThermalFace {
                wall: face.left,
                fluid: face.right,
                coefficient_w_k: coefficient,
            });
        }

        if !conductance
            .iter()
            .zip(&wall_film)
            .all(|(&a, &b)| a > 0.0 && b > 0.0 && (a + b).is_finite())
            || !fluid_film
                .iter()
                .all(|&b| b > 0.0 && (capacity + b).is_finite())
        {
            return Err(Error::NonFiniteResult {
                quantity: "thermal conductances".to_owned(),
            });
        }
        Ok(Self {
            faces,
            capacity,
            hot_delta: solid.hot_boundary.temperature_k - coolant.inlet.temperature_k,
            wall_area,
            conductance,
            wall_film,
            fluid_film,
        })
    }

    fn fluxes(&self, state: &Temperatures) -> InterfaceFlux {
        let mut from_wall = vec![0.0; state.wall.len()];
        let mut into_fluid = vec![0.0; state.fluid.len()];
        for face in &self.faces {
            let heat = face.coefficient_w_k * (state.wall[face.wall] - state.fluid[face.fluid]);
            from_wall[face.wall] += heat;
            into_fluid[face.fluid] += heat;
        }
        InterfaceFlux {
            from_wall,
            into_fluid,
        }
    }

    fn residual(&self, state: &Temperatures, imbalance: &mut Vec<f64>) -> f64 {
        let flux = self.fluxes(state);
        imbalance.clear();
        for (j, heat) in flux.from_wall.into_iter().enumerate() {
            imbalance.push(self.conductance[j] * (self.hot_delta - state.wall[j]) - heat);
        }
        let mut upstream = 0.0;
        for (temperature, heat) in state.fluid.iter().zip(flux.into_fluid) {
            imbalance.push(heat - self.capacity * (temperature - upstream));
            upstream = *temperature;
        }
        norm(imbalance)
    }

    fn solve(&self, settings: CouplingSettings) -> (Convergence, Option<Temperatures>) {
        let mut state = Temperatures {
            wall: vec![0.0; self.wall_film.len()],
            fluid: vec![0.0; self.fluid_film.len()],
        };
        let mut residual = Vec::new();
        let mut trace = Convergence {
            stage: "coupled_wall_coolant_energy".to_owned(),
            algorithm: "partitioned_gauss_seidel".to_owned(),
            stop_reason: StopReason::NumericalBreakdown,
            iterations: 0,
            residual_unit: "W".to_owned(),
            residual_threshold: None,
            residual_l2_history: Vec::new(),
            final_true_residual_l2: None,
            diagnostic: None,
        };
        let initial = self.residual(&state, &mut residual);
        if !initial.is_finite() {
            return (trace, None);
        }
        let threshold = settings
            .absolute_residual_tolerance_w
            .abs()
            .max(settings.relative_residual_tolerance.abs() * initial);
        trace.residual_threshold = Some(threshold);
        trace.residual_l2_history.push(initial);
        trace.final_true_residual_l2 = Some(initial);
        if initial == 0.0 {
            trace.stop_reason = StopReason::Converged;
            return (trace, Some(state));
        }

        let mut fluid_forcing = vec![0.0; self.fluid_film.len()];
        let mut wall_forcing = vec![0.0; self.wall_film.len()];
        for iteration in 1..=settings.max_iterations {
            wall_forcing.fill(0.0);
            for face in &self.faces {
                wall_forcing[face.wall] += face.coefficient_w_k * state.fluid[face.fluid];
            }
            for (j, &mapped_fluid) in wall_forcing.iter().enumerate() {
                state.wall[j] = (self.conductance[j] * self.hot_delta + mapped_fluid)
                    / (self.conductance[j] + self.wall_film[j]);
            }

            fluid_forcing.fill(0.0);
            for face in &self.faces {
                fluid_forcing[face.fluid] += face.coefficient_w_k * state.wall[face.wall];
            }
            let mut upstream = 0.0;
            for (i, &mapped_wall) in fluid_forcing.iter().enumerate() {
                let theta =
                    (self.capacity * upstream + mapped_wall) / (self.capacity + self.fluid_film[i]);
                state.fluid[i] = theta;
                upstream = theta;
            }

            trace.iterations = iteration;
            let actual = self.residual(&state, &mut residual);
            if !actual.is_finite() {
                trace.final_true_residual_l2 = None;
                return (trace, None);
            }
            trace.final_true_residual_l2 = Some(actual);
            trace.residual_l2_history.push(actual);
            if actual <= threshold {
                trace.stop_reason = StopReason::Converged;
                return (trace, Some(state));
            }
        }
        trace.stop_reason = StopReason::IterationLimit;
        (trace, None)
    }

    fn summary(&self, temperatures: Temperatures) -> Result<ChannelEnergy, Error> {
        let flux = self.fluxes(&temperatures);
        let hot_heat: f64 = self
            .conductance
            .iter()
            .zip(&temperatures.wall)
            .map(|(conductance, wall)| conductance * (self.hot_delta - wall))
            .sum();
        let outlet_delta = temperatures.fluid[temperatures.fluid.len() - 1];
        let carried_heat = self.capacity * outlet_delta;
        let film_loss: f64 = flux.from_wall.iter().sum();
        let film_gain: f64 = flux.into_fluid.iter().sum();
        let balances = vec![
            Balance::new(
                "wall_energy",
                "W",
                [
                    ("hot_face_input", hot_heat),
                    ("heat_to_interface", -film_loss),
                ],
            )?,
            Balance::new(
                "interface_energy",
                "W",
                [("from_wall", film_loss), ("to_fluid", -film_gain)],
            )?,
            Balance::new(
                "coolant_energy",
                "W",
                [
                    ("heat_from_interface", film_gain),
                    ("outlet_enthalpy_rise", -carried_heat),
                ],
            )?,
            Balance::new(
                "system_energy",
                "W",
                [
                    ("hot_face_input", hot_heat),
                    ("outlet_enthalpy_rise", -carried_heat),
                ],
            )?,
        ];

        let wall_heat_flux_w_m2 = flux
            .from_wall
            .into_iter()
            .zip(&self.wall_area)
            .map(|(heat, area)| heat / area)
            .collect();
        Ok(ChannelEnergy {
            wall_temperature_offset_k: temperatures.wall,
            fluid_temperature_offset_k: temperatures.fluid,
            wall_heat_flux_w_m2,
            heat_to_coolant_w: film_gain,
            balances,
        })
    }
}
