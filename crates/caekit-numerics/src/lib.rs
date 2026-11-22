//! Sparse linear methods and convergence reporting.

pub mod fmath;
pub mod verify;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Stopping settings of the iterative linear solver, read from a model's `linear_solver` object.
///
/// A solve converges when the 2-norm of its residual is at most
/// `max(absolute_residual_tolerance, relative_residual_tolerance * ‖b‖₂)`, where `b` is the
/// right-hand side of the solved system. In JSON all three keys are required when the object is
/// given and unknown keys are rejected. The [`Default`] values are `1e-10`, `0` and `20000`.
///
/// Reasons of the defaults:
///
/// - Relative tolerance `1e-10`. The relative test bounds the algebraic error by `C` times the
///   tolerance (`docs/design/blueprint.md` section 10.2), so `1e-10` stays under the 3.5e-3
///   algebraic budget of a refinement study (section 11.2) for every amplification `C` up to
///   3.5e7. It lies above the rounding
///   floor `ε‖A‖‖x‖/‖b‖` of the reference models: every model of `examples/` converges with it
///   (`cargo run -p caekit-cli -- run examples/<kind>.json`; the iteration counts are exact and
///   at most 16 per stage).
/// - Absolute tolerance `0`. An absolute residual in W or N has no mesh-independent meaning
///   (the residual scale follows the mesh, `blueprint.md` section 10.2), so the default leaves
///   the relative test in control.
/// - Iteration limit `20000`. It is a guard against a run that does not terminate, not a
///   convergence criterion. Conjugate gradient finishes in at most `n` iterations in exact
///   arithmetic for `n` unknowns, and the reference models need at most 16 (same command as
///   above), so the limit sits three orders of magnitude above what they use.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinearSettings {
    /// Dimensionless factor that multiplies the 2-norm of the right-hand side to form the residual
    /// threshold. Must be greater than zero and less than one.
    pub relative_residual_tolerance: f64,
    /// Lower bound of the residual threshold, in the residual unit of the solved equations (for
    /// example W for heat conduction, N for elasticity). Must be nonnegative and finite; zero
    /// leaves the relative tolerance in control.
    pub absolute_residual_tolerance: f64,
    /// Maximum number of solver iterations. Must be positive.
    pub max_iterations: usize,
}

impl Default for LinearSettings {
    fn default() -> Self {
        Self {
            relative_residual_tolerance: 1.0e-10,
            absolute_residual_tolerance: 0.0,
            max_iterations: 20_000,
        }
    }
}

/// The first [`LinearSettings`] field rejected by [`LinearSettings::validate`].
///
/// Displayed as `field: message`.
#[derive(Debug)]
pub struct InvalidLinearSetting {
    /// Name of the rejected field, identical to its JSON key.
    pub field: &'static str,
    /// The requirement the value violates, for example `must be positive`.
    pub message: &'static str,
}

impl std::fmt::Display for InvalidLinearSetting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

impl std::error::Error for InvalidLinearSetting {}

impl LinearSettings {
    /// Checks the three settings and returns the first violation.
    ///
    /// In order: `relative_residual_tolerance` must lie strictly between zero and one,
    /// `absolute_residual_tolerance` must be nonnegative and finite, and `max_iterations` must be
    /// positive. NaN is rejected for both tolerances.
    pub fn validate(self) -> Result<(), InvalidLinearSetting> {
        if !(0.0 < self.relative_residual_tolerance && self.relative_residual_tolerance < 1.0) {
            return Err(InvalidLinearSetting {
                field: "relative_residual_tolerance",
                message: "must be greater than zero and less than one",
            });
        }
        if !self.absolute_residual_tolerance.is_finite() || self.absolute_residual_tolerance < 0.0 {
            return Err(InvalidLinearSetting {
                field: "absolute_residual_tolerance",
                message: "must be nonnegative and finite",
            });
        }
        if self.max_iterations == 0 {
            return Err(InvalidLinearSetting {
                field: "max_iterations",
                message: "must be positive",
            });
        }
        Ok(())
    }
}

/// Stop reasons for a numerical stage. A failed stage retains its convergence evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// The residual norm evaluated from the returned iterate is at or below the threshold. Written
    /// `converged` in JSON.
    Converged,
    /// The iteration limit was reached with the residual still above the threshold. Written
    /// `iteration_limit` in JSON.
    IterationLimit,
    /// The stage could not proceed or its result could not be accepted: invalid input to the solve,
    /// a non-finite or nonpositive scalar inside the iteration, or a true residual that stagnates
    /// above the threshold at the binary64 rounding scale. Written `numerical_breakdown` in JSON.
    NumericalBreakdown,
}

/// Convergence record of one numerical stage, an entry of the `convergence` list of a run report.
///
/// All residual norms are 2-norms in `residual_unit`.
#[derive(Debug, Serialize, Deserialize)]
pub struct Convergence {
    /// Name of the stage, chosen by the calling solver, for example `solid_heat_conduction`.
    pub stage: String,
    /// Identifier of the iterative method, for example `jacobi_preconditioned_conjugate_gradient`.
    pub algorithm: String,
    /// Why the stage stopped.
    pub stop_reason: StopReason,
    /// Number of iterations carried out; zero when the initial residual already met the threshold
    /// or the stage broke down before iterating.
    pub iterations: usize,
    /// Unit of the residual norms and of the threshold, as text, for example `W`, `N` or `Pa`.
    pub residual_unit: String,
    /// Residual norm at or below which the stage counts as converged: the larger of the absolute
    /// tolerance and the relative tolerance times the initial residual norm. `None` (JSON `null`)
    /// when the stage broke down before the threshold was formed.
    pub residual_threshold: Option<f64>,
    /// Residual norm before the first iteration, followed by the norm after each iteration that
    /// produced a finite one. In the conjugate gradient solve the later entries are norms of the
    /// recursively updated residual, which can drift from `b − A x`.
    pub residual_l2_history: Vec<f64>,
    /// Residual norm evaluated directly from the returned iterate, `‖b − A x‖₂` for a linear solve.
    /// `None` (JSON `null`) when it was not evaluated or is not finite.
    pub final_true_residual_l2: Option<f64>,
    /// Explanation of a failure when the solver has one to give. The key is omitted from JSON when
    /// absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}

/// Sparse triplet accumulation with deterministic row and column ordering.
pub struct SparseAssembly {
    rows: Vec<BTreeMap<usize, f64>>,
    invalid: bool,
}

impl SparseAssembly {
    /// Creates a square matrix of `size` rows and columns with no entries.
    pub fn new(size: usize) -> Self {
        Self {
            rows: vec![BTreeMap::new(); size],
            invalid: false,
        }
    }

    /// Adds `entry` to the coefficient at zero-based `row` and `column`; repeated calls for the
    /// same position accumulate.
    ///
    /// An index outside the matrix does not panic. It marks the assembly invalid, and
    /// [`SparseAssembly::solve`] then reports a numerical breakdown.
    pub fn add(&mut self, row: usize, column: usize, entry: f64) {
        if row >= self.rows.len() || column >= self.rows.len() {
            self.invalid = true;
            return;
        }
        *self.rows[row].entry(column).or_default() += entry;
    }

    /// Invalid indices, dimensions or settings produce a numerical breakdown at iteration zero.
    pub fn solve(
        self,
        rhs: &[f64],
        settings: LinearSettings,
        stage: &str,
        residual_unit: &str,
    ) -> (Vec<f64>, Convergence) {
        let stats = Convergence {
            stage: stage.to_owned(),
            algorithm: "jacobi_preconditioned_conjugate_gradient".to_owned(),
            stop_reason: StopReason::NumericalBreakdown,
            iterations: 0,
            residual_unit: residual_unit.to_owned(),
            residual_threshold: None,
            residual_l2_history: Vec::new(),
            final_true_residual_l2: None,
            diagnostic: None,
        };
        if self.invalid || rhs.len() != self.rows.len() || settings.validate().is_err() {
            return (vec![0.0; self.rows.len()], stats);
        }
        pcg(&CsrMatrix::from_rows(self.rows), rhs, settings, stats)
    }
}

struct CsrMatrix {
    offsets: Vec<usize>,
    columns: Vec<usize>,
    entries: Vec<f64>,
    diagonal: Vec<f64>,
}

impl CsrMatrix {
    fn from_rows(rows: Vec<BTreeMap<usize, f64>>) -> Self {
        let mut matrix = Self {
            offsets: Vec::with_capacity(rows.len() + 1),
            columns: Vec::new(),
            entries: Vec::new(),
            diagonal: vec![0.0; rows.len()],
        };
        matrix.offsets.push(0);
        for (i, row) in rows.into_iter().enumerate() {
            for (j, entry) in row {
                if i == j {
                    matrix.diagonal[i] = entry;
                }
                if entry != 0.0 {
                    matrix.columns.push(j);
                    matrix.entries.push(entry);
                }
            }
            matrix.offsets.push(matrix.entries.len());
        }
        matrix
    }

    fn multiply(&self, x: &[f64], output: &mut [f64]) {
        for (row, value) in output.iter_mut().enumerate() {
            *value = (self.offsets[row]..self.offsets[row + 1])
                .map(|i| self.entries[i] * x[self.columns[i]])
                .sum();
        }
    }

    /// Conservative binary64 rounding scale for computing b - A*x row by row.
    ///
    /// A row with `k` stored entries does `k` products, `k` additions of the running sum and
    /// one subtraction from `b`, so `2k + 1` operations each add a relative error of at most
    /// the unit roundoff `ε/2`. The bound `(2k + 1) ε (|b| + |A||x|)` therefore overstates the
    /// first-order bound `γ_{2k+1}(|b| + |A||x|)` of Higham (2002) by the factor two that
    /// replaces `ε/2` with `ε`.
    fn residual_roundoff_l2(&self, x: &[f64], rhs: &[f64]) -> f64 {
        fmath::norm2(rhs.iter().enumerate().map(|(row, b)| {
            let indices = self.offsets[row]..self.offsets[row + 1];
            let operations = 2.0 * indices.len() as f64 + 1.0;
            let scale = indices.fold(b.abs(), |sum, i| {
                sum + (self.entries[i] * x[self.columns[i]]).abs()
            });
            operations * f64::EPSILON * scale
        }))
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

/// Returns the Euclidean norm of `values`, which is zero for an empty slice.
///
/// See [`fmath::norm2`] for scaling and non-finite inputs.
pub fn norm(values: &[f64]) -> f64 {
    fmath::norm2(values.iter().copied())
}

/// A verified correction counts as progress when it lowers the true residual below this fraction
/// of the best true residual verified so far.
///
/// A heuristic with no derivation: its only job is to tell a stalled residual (ratio near one)
/// from a converging one. It cannot cause a false verdict, because the stagnation stop also needs
/// the true residual to lie below the rounding bound of
/// [`CsrMatrix::residual_roundoff_l2`]. It only moves the iteration at which a hopeless solve
/// stops; the unattainable single-unknown system of `tests/sparse_system.rs` stops after 10
/// iterations (measured by solving it with a relative tolerance of 1e-20).
const STAGNATION_PROGRESS_FACTOR: f64 = 0.75;

/// Number of consecutive verified corrections without progress (see
/// [`STAGNATION_PROGRESS_FACTOR`]) after which the rounding bound is consulted.
///
/// A heuristic with no derivation; it has the same role and the same safety argument as
/// [`STAGNATION_PROGRESS_FACTOR`]: the rounding bound, not the count, decides the verdict.
const STAGNATION_RESTART_LIMIT: u32 = 8;

fn pcg(
    matrix: &CsrMatrix,
    rhs: &[f64],
    settings: LinearSettings,
    mut stats: Convergence,
) -> (Vec<f64>, Convergence) {
    let mut x = vec![0.0; rhs.len()];
    if !rhs.iter().all(|x| x.is_finite())
        || !matrix.entries.iter().all(|x| x.is_finite())
        || !matrix.diagonal.iter().all(|&d| d > 0.0)
    {
        return (x, stats);
    }

    let initial_norm = norm(rhs);
    if !initial_norm.is_finite() {
        return (x, stats);
    }
    let threshold = settings
        .absolute_residual_tolerance
        .abs()
        .max(settings.relative_residual_tolerance.abs() * initial_norm);
    stats.residual_threshold = Some(threshold);
    stats.residual_l2_history.push(initial_norm);
    if initial_norm <= threshold {
        stats.stop_reason = StopReason::Converged;
        stats.final_true_residual_l2 = Some(initial_norm);
        return (x, stats);
    }

    let mut residual = rhs.to_vec();
    let inverse_diagonal: Vec<_> = matrix.diagonal.iter().map(|d| d.recip()).collect();
    let mut preconditioned: Vec<_> = residual
        .iter()
        .zip(&inverse_diagonal)
        .map(|(r, d)| r * d)
        .collect();
    let mut direction = preconditioned.clone();
    let mut residual_dot = dot(&residual, &preconditioned);
    let mut product = vec![0.0; rhs.len()];
    let mut restart_baseline = f64::INFINITY;
    let mut unproductive_restarts = 0_u32;

    for iteration in 1..=settings.max_iterations {
        matrix.multiply(&direction, &mut product);
        let denominator = dot(&direction, &product);
        if !residual_dot.is_finite()
            || residual_dot <= 0.0
            || !denominator.is_finite()
            || denominator <= 0.0
        {
            break;
        }
        let step = residual_dot / denominator;
        if !step.is_finite() {
            break;
        }
        for i in 0..x.len() {
            x[i] += step * direction[i];
            residual[i] -= step * product[i];
        }
        stats.iterations = iteration;
        let estimate = norm(&residual);
        if !estimate.is_finite() {
            break;
        }
        stats.residual_l2_history.push(estimate);

        // Recursive residuals drift from b - A*x. Verify convergence against the latter.
        let restart = estimate <= threshold;
        if restart {
            matrix.multiply(&x, &mut product);
            for i in 0..rhs.len() {
                residual[i] = rhs[i] - product[i];
            }
            let actual = norm(&residual);
            if actual.is_finite() && actual <= threshold {
                stats.stop_reason = StopReason::Converged;
                stats.final_true_residual_l2 = Some(actual);
                return (x, stats);
            }
            if !actual.is_finite() {
                break;
            }
            // Count complete correction attempts only after verifying the true residual.
            if actual < STAGNATION_PROGRESS_FACTOR * restart_baseline {
                restart_baseline = actual;
                unproductive_restarts = 0;
            } else {
                unproductive_restarts += 1;
                if unproductive_restarts >= STAGNATION_RESTART_LIMIT {
                    let roundoff = matrix.residual_roundoff_l2(&x, rhs);
                    if roundoff.is_finite() && actual <= roundoff {
                        stats.final_true_residual_l2 = Some(actual);
                        stats.diagnostic = Some(format!(
                            "True residual stagnated at {actual:.6e} {} above the requested threshold {threshold:.6e} {} after repeated corrections at binary64 precision.",
                            stats.residual_unit, stats.residual_unit
                        ));
                        return (x, stats);
                    }
                }
            }
        }
        for i in 0..rhs.len() {
            preconditioned[i] = residual[i] * inverse_diagonal[i];
        }
        let next_dot = dot(&residual, &preconditioned);
        let beta = if restart {
            0.0
        } else {
            next_dot / residual_dot
        };
        for i in 0..rhs.len() {
            direction[i] = preconditioned[i] + beta * direction[i];
        }
        residual_dot = next_dot;
    }

    matrix.multiply(&x, &mut product);
    for i in 0..rhs.len() {
        residual[i] = rhs[i] - product[i];
    }
    let actual = norm(&residual);
    if actual.is_finite() {
        stats.final_true_residual_l2 = Some(actual);
        if actual <= threshold {
            stats.stop_reason = StopReason::Converged;
        } else if stats.iterations == settings.max_iterations {
            stats.stop_reason = StopReason::IterationLimit;
        }
    }
    (x, stats)
}

/// Symmetric elimination of prescribed degrees of freedom.
pub struct ConstrainedSystem {
    reduced: SparseAssembly,
    rhs: Vec<f64>,
    prescribed: Vec<Option<f64>>,
    reduced_index: Vec<Option<usize>>,
}

impl ConstrainedSystem {
    /// Starts a system with one degree of freedom per entry of `prescribed`.
    ///
    /// `Some(value)` fixes that degree of freedom to `value` and `None` leaves it free. `loads` is
    /// the right-hand side for all degrees of freedom in the same zero-based order; its entries at
    /// prescribed degrees of freedom do not enter the reduced equations. If `loads` and
    /// `prescribed` differ in length, or any load or prescribed value is not finite, the system is
    /// marked invalid and [`ConstrainedSystem::solve`] reports a numerical breakdown.
    pub fn new(prescribed: Vec<Option<f64>>, loads: &[f64]) -> Self {
        let mut rhs = Vec::new();
        let reduced_index = prescribed
            .iter()
            .enumerate()
            .map(|(i, value)| {
                if value.is_some() {
                    None
                } else {
                    let index = rhs.len();
                    rhs.push(loads.get(i).copied().unwrap_or(0.0));
                    Some(index)
                }
            })
            .collect();
        let mut reduced = SparseAssembly::new(rhs.len());
        reduced.invalid = loads.len() != prescribed.len()
            || !prescribed
                .iter()
                .flatten()
                .chain(loads)
                .all(|v| v.is_finite());
        Self {
            reduced,
            rhs,
            prescribed,
            reduced_index,
        }
    }

    /// Adds `entry` to the coefficient of the full system at zero-based `row` and `column`;
    /// repeated calls accumulate.
    ///
    /// An entry with a free row and a free column goes to the reduced matrix. With a free row and a
    /// prescribed column, `entry` times the prescribed value is subtracted from the right-hand
    /// side. Entries in a prescribed row are dropped because that equation is eliminated. An
    /// out-of-range index or a non-finite `entry` does not panic; it marks the system invalid.
    pub fn add(&mut self, row: usize, column: usize, entry: f64) {
        if row >= self.reduced_index.len()
            || column >= self.reduced_index.len()
            || !entry.is_finite()
        {
            self.reduced.invalid = true;
            return;
        }
        if let Some(i) = self.reduced_index[row] {
            match self.reduced_index[column] {
                Some(j) => self.reduced.add(i, j, entry),
                None => self.rhs[i] -= entry * self.prescribed[column].unwrap(),
            }
        }
    }

    /// Solves the reduced equations and returns the values of all degrees of freedom.
    ///
    /// The free equations are passed to [`SparseAssembly::solve`] with `settings`; `stage` and
    /// `residual_unit` label the returned [`Convergence`], whose residuals refer to the free
    /// equations only. The returned vector follows the original ordering: prescribed entries hold
    /// their prescribed values and free entries hold the solver's iterate, which is a solution only
    /// if the stop reason is [`StopReason::Converged`]. An invalid system or invalid settings give
    /// a numerical breakdown at iteration zero with the free entries left at zero. A valid system
    /// without free degrees of freedom converges in zero iterations.
    pub fn solve(
        self,
        settings: LinearSettings,
        stage: &str,
        residual_unit: &str,
    ) -> (Vec<f64>, Convergence) {
        let (free, convergence) = self
            .reduced
            .solve(&self.rhs, settings, stage, residual_unit);
        let full = self
            .prescribed
            .into_iter()
            .zip(self.reduced_index)
            .map(|(value, index)| value.unwrap_or_else(|| free[index.unwrap()]))
            .collect();
        (full, convergence)
    }
}
