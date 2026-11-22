//! Solution-verification functions for refinement studies.
//!
//! The functions measure how a discretization error or a computed quantity changes over a
//! family of refined meshes: the observed order between two levels and fitted by least squares
//! over three or more levels, the order band that a refinement study must meet
//! ([`band_check`]), Richardson extrapolation, the grid convergence index (GCI), the
//! classification of a sequence as monotone, oscillatory or divergent, and the budget for the
//! algebraic (iterative solver) error of a study (Roache 1994; NASA NPARC, *Examining Spatial
//! (Grid) Convergence*; Celik et al. 2008; Eça and Hoekstra 2014).
//!
//! Spacings are mesh sizes `h` in any one length unit, because only their ratios enter; errors
//! are nonnegative norms in the unit of the measured quantity. Every logarithm and power comes
//! from `libm`, so a result is the same on every platform and in WebAssembly. No input causes a
//! panic: an input that the formulas do not accept returns a [`VerifyError`].
//!
//! A Taylor test of a Jacobian fits its remainders `‖F(x + εv) − F(x) − ε J v‖` with
//! [`fitted_order`], using the step sizes `ε` as spacings; an exact Jacobian gives order 2.

use std::fmt;

/// Largest shift `Δp` of an observed order that the algebraic error of a study may cause.
///
/// Dimensionless. [`algebraic_error_budget`] turns it into a bound on the relative algebraic
/// error of each measured error norm.
///
/// Reason: it is the algebraic share of the order band `δ` of [`band_check`] and
/// `docs/design/blueprint.md` section 11.2. Specification SPEC-VER-003 keeps it below one third of the narrowest order margin
/// of the existing studies: the pressure-loss study of `laminar_channel` accepts error ratios
/// from 3.8 to 4.1 at a refinement ratio of 2, observed orders 1.926 to 2.036, an upper margin
/// of 0.036 above the expected order 2.
pub const ORDER_SHIFT_BUDGET: f64 = 0.01;

/// Factor by which a study divides the relative solver tolerance for its check solve.
///
/// Each level is solved again with `relative_residual_tolerance / TOLERANCE_DIVISOR`, and
/// [`algebraic_check`] compares the error norms of the two solves.
///
/// Reason: the tighter solve is the reference of the relative change, so its own algebraic error
/// should be small against the one it measures. With the error amplification of
/// `docs/design/blueprint.md` section 10.2 proportional to the tolerance, dividing it by 100
/// leaves the reference with a hundredth of the algebraic error of the study solve, so the
/// measured change is that error to within about one percent.
pub const TOLERANCE_DIVISOR: f64 = 100.0;

/// Safety factor `F_s` of the grid convergence index when the order is observed on three grids.
///
/// Roache's value as given by the NASA NPARC page; two grids with an assumed order would use
/// 3.0, which this module does not offer.
pub const GCI_SAFETY_FACTOR: f64 = 1.25;

/// Fewest levels that [`fitted_order`] and [`classify`] accept.
///
/// A least-squares order needs more points than unknowns, and a classification needs two
/// successive changes. [`band_check`] fits this many levels, the finest ones.
pub const MIN_FIT_LEVELS: usize = 3;

/// Fewest levels of a refinement study, which [`band_check`] accepts.
///
/// Specification SPEC-VER-003 asks for four: the finest three are fitted, and the coarsest
/// adds a third pairwise order that shows how the distances to the formal order develop.
pub const MIN_STUDY_LEVELS: usize = 4;

/// Factor on the spread `s` in the band half-width `δ = Δp + 1.5 s` of [`band_check`].
///
/// Reason: derived in [`band_check`]; the fit over the finest three levels lies at most
/// `1.5 s` from the formal order when the distances to it shrink by [`MIN_CONTRACTION`] or more
/// per level.
pub const SPREAD_FACTOR: f64 = 1.5;

/// Least shrink factor `ρ` of the distances to the formal order that [`SPREAD_FACTOR`] covers.
///
/// Reason: a leading correction of order `h^q` with `q ≥ 1` shrinks the distances by `r^q`
/// per level, at least 2 at a refinement ratio `r ≥ 2`; [`band_check`] reports whether a study
/// meets it.
pub const MIN_CONTRACTION: f64 = 2.0;

/// Relative difference up to which [`band_check`] treats two refinement ratios as equal.
///
/// Reason: spacings computed as `L/n` round by half an ulp each, so the ratios of a family with
/// one ratio agree to a few epsilon; 1e-12 is 4,500 epsilon, far above that. A ratio that
/// differs by a relative `ε` moves the three-level fit away from the mean of its two pairwise
/// orders by about `ε s/ln r`, below 1e-12 for every spread `s < 1` at `r ≥ 2`, so the
/// derivation of the band still holds.
const RATIO_TOLERANCE: f64 = 1e-12;

/// An input that a verification formula does not accept.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum VerifyError {
    /// Fewer levels than the function needs.
    TooFewLevels {
        /// Number of levels the function needs at least.
        needed: usize,
        /// Number of levels given.
        given: usize,
    },
    /// The spacing and error slices differ in length.
    LengthMismatch {
        /// Number of spacings given.
        spacings: usize,
        /// Number of errors given.
        errors: usize,
    },
    /// A spacing or an error norm is zero, negative or not finite.
    NotPositive {
        /// `"spacing"` or `"error"`.
        quantity: &'static str,
        /// Index of the value in its slice; 0 for a scalar argument, 0 and 1 for the coarse and
        /// fine argument of a two-level function.
        index: usize,
    },
    /// A solution value is not finite.
    NotFinite {
        /// Index of the value, counted from the finest level.
        index: usize,
    },
    /// The refinement ratio is not finite or not greater than one. [`band_check`] returns it
    /// also for levels listed from the finest to the coarsest.
    InvalidRatio,
    /// The ratio of a pair of successive spacings differs from the ratio of the first pair by
    /// more than a relative 1e-12.
    NonConstantRatio {
        /// Index of the coarser level of the pair.
        index: usize,
    },
    /// The order given to [`richardson_extrapolation`] or the formal order given to
    /// [`band_check`] is not finite or not positive.
    InvalidOrder,
    /// All spacings of a fit are equal, or two successive spacings of a pairwise order are.
    CoincidentSpacings,
    /// Two successive solution values are equal, so the ratio of their changes is undefined.
    UnchangedSolution {
        /// Index of the finer of the two values, counted from the finest level.
        index: usize,
    },
    /// A solution value that normalizes a GCI is zero.
    ZeroSolution {
        /// Index of the value, counted from the finest level.
        index: usize,
    },
    /// The sequence is not monotone, so no GCI is reported.
    NotMonotone(Behaviour),
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewLevels { needed, given } => {
                write!(f, "needs at least {needed} levels, got {given}")
            }
            Self::LengthMismatch { spacings, errors } => {
                write!(f, "{spacings} spacings but {errors} errors")
            }
            Self::NotPositive { quantity, index } => {
                write!(f, "{quantity} {index} must be positive and finite")
            }
            Self::NotFinite { index } => write!(f, "solution value {index} must be finite"),
            Self::InvalidRatio => {
                write!(f, "refinement ratio must be finite and greater than one")
            }
            Self::NonConstantRatio { index } => write!(
                f,
                "refinement ratio of levels {index} and {} differs from the first",
                index + 1
            ),
            Self::InvalidOrder => write!(f, "order must be positive and finite"),
            Self::CoincidentSpacings => write!(f, "spacings must differ"),
            Self::UnchangedSolution { index } => {
                write!(f, "solution values {index} and {} are equal", index + 1)
            }
            Self::ZeroSolution { index } => write!(f, "solution value {index} is zero"),
            Self::NotMonotone(behaviour) => {
                write!(f, "no GCI for a {behaviour} sequence")
            }
        }
    }
}

impl std::error::Error for VerifyError {}

/// How a sequence of solution values behaves under refinement.
///
/// For three successive values `f₁, f₂, f₃` from fine to coarse, the convergence ratio is
/// `R = (f₂ − f₁)/(f₃ − f₂)`: `0 < R < 1` is monotone convergence, `−1 < R < 0` oscillatory
/// convergence and `|R| ≥ 1` divergence (Celik et al. 2008; Stern et al. 2001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behaviour {
    /// Every triplet has `0 < R < 1`.
    Monotone,
    /// Every triplet has `|R| < 1` and at least one has `R < 0`.
    Oscillatory,
    /// At least one triplet has `|R| ≥ 1`.
    Divergent,
}

impl fmt::Display for Behaviour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Monotone => "monotone",
            Self::Oscillatory => "oscillatory",
            Self::Divergent => "divergent",
        })
    }
}

fn check_ratio(ratio: f64) -> Result<(), VerifyError> {
    if ratio.is_finite() && ratio > 1.0 {
        Ok(())
    } else {
        Err(VerifyError::InvalidRatio)
    }
}

fn check_positive(value: f64, quantity: &'static str, index: usize) -> Result<(), VerifyError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(VerifyError::NotPositive { quantity, index })
    }
}

fn check_levels(spacings: &[f64], errors: &[f64], needed: usize) -> Result<(), VerifyError> {
    if spacings.len() != errors.len() {
        return Err(VerifyError::LengthMismatch {
            spacings: spacings.len(),
            errors: errors.len(),
        });
    }
    if spacings.len() < needed {
        return Err(VerifyError::TooFewLevels {
            needed,
            given: spacings.len(),
        });
    }
    for (index, &spacing) in spacings.iter().enumerate() {
        check_positive(spacing, "spacing", index)?;
    }
    for (index, &error) in errors.iter().enumerate() {
        check_positive(error, "error", index)?;
    }
    Ok(())
}

/// Observed order `p̂ = ln(e_coarse/e_fine)/ln r` between two levels with refinement ratio `r`.
///
/// `coarse_error` and `fine_error` are error norms against an exact solution, in one unit, and
/// must be positive and finite (`NotPositive` with index 0 or 1). `ratio` is
/// `h_coarse/h_fine` and must be finite and greater than one. A positive result means the error
/// falls with refinement.
pub fn observed_order(coarse_error: f64, fine_error: f64, ratio: f64) -> Result<f64, VerifyError> {
    check_positive(coarse_error, "error", 0)?;
    check_positive(fine_error, "error", 1)?;
    check_ratio(ratio)?;
    Ok(libm::log(coarse_error / fine_error) / libm::log(ratio))
}

/// Observed orders of each pair of successive levels, `ln(eᵢ/eᵢ₊₁)/ln(hᵢ/hᵢ₊₁)`.
///
/// `spacings` and `errors` hold one value per level in the same order, any order of
/// refinement; every value must be positive and finite, and at least two levels are needed.
/// Two successive equal spacings return `CoincidentSpacings`. Entry `i` of the result belongs
/// to levels `i` and `i + 1`. The band rule of a study, [`band_check`], uses the spread of the
/// pairwise orders of its finest three levels.
pub fn pairwise_orders(spacings: &[f64], errors: &[f64]) -> Result<Vec<f64>, VerifyError> {
    check_levels(spacings, errors, 2)?;
    spacings
        .windows(2)
        .zip(errors.windows(2))
        .map(|(h, e)| {
            let log_ratio = libm::log(h[0] / h[1]);
            if log_ratio == 0.0 {
                return Err(VerifyError::CoincidentSpacings);
            }
            Ok(libm::log(e[0] / e[1]) / log_ratio)
        })
        .collect()
}

/// Observed order fitted by least squares to `ln e = ln C + p ln h` over all levels.
///
/// Returns the slope `p̂ = Σ(xᵢ − x̄)(yᵢ − ȳ)/Σ(xᵢ − x̄)²` with `xᵢ = ln hᵢ` and `yᵢ = ln eᵢ`,
/// summed in slice order (Eça and Hoekstra 2014 for the least-squares form). `spacings` and
/// `errors` hold one value per level in the same order; the levels need not have a constant
/// ratio. Every value must be positive and finite, at least [`MIN_FIT_LEVELS`] levels are
/// needed, and equal spacings everywhere return `CoincidentSpacings`. For errors `C hᵖ` the
/// result is `p` to rounding.
pub fn fitted_order(spacings: &[f64], errors: &[f64]) -> Result<f64, VerifyError> {
    check_levels(spacings, errors, MIN_FIT_LEVELS)?;
    let count = spacings.len() as f64;
    let x: Vec<f64> = spacings.iter().map(|&h| libm::log(h)).collect();
    let y: Vec<f64> = errors.iter().map(|&e| libm::log(e)).collect();
    let x_mean = x.iter().sum::<f64>() / count;
    let y_mean = y.iter().sum::<f64>() / count;
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    for (&xi, &yi) in x.iter().zip(&y) {
        let dx = xi - x_mean;
        sxy += dx * (yi - y_mean);
        sxx += dx * dx;
    }
    if sxx == 0.0 {
        return Err(VerifyError::CoincidentSpacings);
    }
    Ok(sxy / sxx)
}

/// Outcome of the band rule of a refinement study. Built only by [`band_check`].
///
/// Orders are dimensionless. A distance `dᵢ = p_f − p̂ᵢ` is positive when the pairwise order
/// `p̂ᵢ` lies below the formal order `p_f`.
#[derive(Debug, Clone, PartialEq)]
pub struct BandCheck {
    /// Formal order `p_f` of the scheme in the measured norm.
    pub formal_order: f64,
    /// Least-squares order of the finest three levels ([`fitted_order`]); the value that the
    /// band accepts or rejects.
    pub fitted_order: f64,
    /// Orders of successive level pairs ([`pairwise_orders`]), coarsest pair first.
    pub pairwise_orders: Vec<f64>,
    /// Spread `s = |p̂₂ − p̂₃|` of the two finest pairwise orders, `p̂₃` the finest.
    pub spread: f64,
    /// Half-width `δ = Δp + 1.5 s` of the band, with `Δp` = [`ORDER_SHIFT_BUDGET`] and 1.5 =
    /// [`SPREAD_FACTOR`].
    pub half_width: f64,
    /// The band `[p_f − δ, p_f + δ]`, lower edge first.
    pub band: [f64; 2],
    /// Shrink factor `ρ = d₂/d₃` of the distances of the two finest pairwise orders. Negative
    /// when they lie on both sides of `p_f`; infinite or NaN when `d₃` is zero.
    pub contraction: f64,
    /// Signed distance `d₃ = p_f − p̂₃` of the finest pairwise order.
    pub finest_distance: f64,
    /// Whether `|p̂ − p_f| ≤ δ` for the fitted order `p̂`.
    pub in_band: bool,
    /// Whether the band derivation applies: `ρ ≥ 2` ([`MIN_CONTRACTION`]), or `|d₃| ≤ Δp`, for
    /// which the derivation needs no shrink rate.
    pub contraction_holds: bool,
}

impl BandCheck {
    /// Whether the study meets the band rule: the fit lies in the band and the derivation of
    /// the band applies.
    pub fn passes(&self) -> bool {
        self.in_band && self.contraction_holds
    }

    /// The first failed condition as a message, `None` when the study passes. A fit outside
    /// the band is reported first, with a message that starts with `"fitted order"`.
    pub fn failure(&self) -> Option<String> {
        if !self.in_band {
            Some(format!(
                "fitted order {} outside {} +- {}",
                self.fitted_order, self.formal_order, self.half_width
            ))
        } else if !self.contraction_holds {
            Some(format!(
                "distances to the formal order shrink by {} per level, below {MIN_CONTRACTION}",
                self.contraction
            ))
        } else {
            None
        }
    }
}

/// Applies the band rule of `docs/design/blueprint.md` section 11.2 to a refinement study.
///
/// `spacings` and `errors` hold one value per level, ordered from the coarsest to the finest
/// level. Every value must be positive and finite (`NotPositive`); each slice is in any one
/// unit, because only ratios enter. At least [`MIN_STUDY_LEVELS`] levels are needed
/// (`TooFewLevels`, `LengthMismatch` for slices of different lengths), with one refinement
/// ratio `r = hᵢ/hᵢ₊₁ > 1`: `InvalidRatio` for `r ≤ 1`, so also for levels listed from fine to
/// coarse, and `NonConstantRatio` when a ratio differs from the first by more than a relative
/// 1e-12. `formal_order` is the expected order `p_f`, positive and finite (`InvalidOrder`). No
/// input causes a panic.
///
/// The rule fits the order `p̂` by least squares over the finest three levels and requires
/// `|p̂ − p_f| ≤ δ` with `δ = Δp + 1.5 s`, where `Δp` = [`ORDER_SHIFT_BUDGET`] is the share of
/// the algebraic error and `s` the spread of the two finest pairwise orders. The band is valid
/// when the distances to `p_f` shrink by `ρ ≥ 2` per level, which the result reports
/// ([`BandCheck::contraction_holds`]). A super-optimal fit lies outside the band and fails
/// like a low one.
///
/// Derivation of the factor 1.5: at a constant ratio the three `ln h` are equally spaced, so
/// the least-squares slope is the mean of the two finest pairwise orders, `p̂ = (p̂₂ + p̂₃)/2`.
/// For an error `C hᵖ (1 + D h^q)` a pairwise order differs from `p` by about
/// `D h^q (1 − r^(−q))/ln r`, with `h` the coarser spacing of the pair, so the distances
/// shrink by `ρ = d₂/d₃ = r^q` per level. With `d₂` and `d₃` of one sign, `s = |d₃| (ρ − 1)`
/// and `|p̂ − p_f| = |d₃| (ρ + 1)/2 = s (ρ + 1)/(2 (ρ − 1))`, which falls with `ρ` and equals
/// `1.5 s` at `ρ = 2`. When `|d₃| ≤ Δp` no shrink rate is needed: `|d₂| ≤ |d₃| + s`, so
/// `|p̂ − p_f| ≤ |d₃| + s/2 ≤ Δp + 1.5 s`.
///
/// The values are computed with these operations, so a study that computed them itself gets
/// the same bits: the pairwise orders by [`pairwise_orders`], the fit by [`fitted_order`] over
/// the finest three levels, `s = |p̂₂ − p̂₃|`, `ρ = (p_f − p̂₂)/(p_f − p̂₃)`, `d₃ = p_f − p̂₃` and
/// `δ = Δp + 1.5 s`.
pub fn band_check(
    spacings: &[f64],
    errors: &[f64],
    formal_order: f64,
) -> Result<BandCheck, VerifyError> {
    check_levels(spacings, errors, MIN_STUDY_LEVELS)?;
    let first_ratio = spacings[0] / spacings[1];
    check_ratio(first_ratio)?;
    for (index, pair) in spacings.windows(2).enumerate() {
        if (pair[0] / pair[1] - first_ratio).abs() > RATIO_TOLERANCE * first_ratio {
            return Err(VerifyError::NonConstantRatio { index });
        }
    }
    if !(formal_order.is_finite() && formal_order > 0.0) {
        return Err(VerifyError::InvalidOrder);
    }
    let pairwise_orders = pairwise_orders(spacings, errors)?;
    let [.., older, newest] = pairwise_orders[..] else {
        unreachable!("check_levels established at least four levels, so three pairwise orders")
    };
    let first = spacings.len() - MIN_FIT_LEVELS;
    let fitted_order = fitted_order(&spacings[first..], &errors[first..])?;
    let spread = (older - newest).abs();
    let contraction = (formal_order - older) / (formal_order - newest);
    let finest_distance = formal_order - newest;
    let half_width = ORDER_SHIFT_BUDGET + SPREAD_FACTOR * spread;
    Ok(BandCheck {
        formal_order,
        fitted_order,
        pairwise_orders,
        spread,
        half_width,
        band: [formal_order - half_width, formal_order + half_width],
        contraction,
        finest_distance,
        in_band: (fitted_order - formal_order).abs() <= half_width,
        contraction_holds: finest_distance.abs() <= ORDER_SHIFT_BUDGET
            || contraction >= MIN_CONTRACTION,
    })
}

/// Classifies solution values ordered from the finest to the coarsest level.
///
/// Each triplet of successive values gives `R = (f₂ − f₁)/(f₃ − f₂)` (see [`Behaviour`]). The
/// sequence is `Divergent` if any triplet has `|R| ≥ 1`, otherwise `Oscillatory` if any has
/// `R < 0`, otherwise `Monotone`. Needs at least [`MIN_FIT_LEVELS`] finite values; two equal
/// successive values return `UnchangedSolution`, because `R` is then zero or undefined.
pub fn classify(values: &[f64]) -> Result<Behaviour, VerifyError> {
    if values.len() < MIN_FIT_LEVELS {
        return Err(VerifyError::TooFewLevels {
            needed: MIN_FIT_LEVELS,
            given: values.len(),
        });
    }
    if let Some(index) = values.iter().position(|value| !value.is_finite()) {
        return Err(VerifyError::NotFinite { index });
    }
    if let Some(index) = values.windows(2).position(|pair| pair[0] == pair[1]) {
        return Err(VerifyError::UnchangedSolution { index });
    }
    let mut behaviour = Behaviour::Monotone;
    for triplet in values.windows(3) {
        let ratio = (triplet[1] - triplet[0]) / (triplet[2] - triplet[1]);
        if ratio.abs() >= 1.0 {
            return Ok(Behaviour::Divergent);
        }
        if ratio < 0.0 {
            behaviour = Behaviour::Oscillatory;
        }
    }
    Ok(behaviour)
}

/// Richardson extrapolation `f₁ + (f₁ − f₂)/(rᵖ − 1)` of a fine and a coarse solution value.
///
/// `fine` and `coarse` are finite values of one quantity on two levels with ratio
/// `r = h_coarse/h_fine > 1`; `order` is the formal or observed order `p` and must be positive
/// and finite. The result estimates the value at `h = 0` in the unit of the inputs.
pub fn richardson_extrapolation(
    fine: f64,
    coarse: f64,
    ratio: f64,
    order: f64,
) -> Result<f64, VerifyError> {
    if !fine.is_finite() {
        return Err(VerifyError::NotFinite { index: 0 });
    }
    if !coarse.is_finite() {
        return Err(VerifyError::NotFinite { index: 1 });
    }
    check_ratio(ratio)?;
    if !(order.is_finite() && order > 0.0) {
        return Err(VerifyError::InvalidOrder);
    }
    Ok(fine + (fine - coarse) / (libm::pow(ratio, order) - 1.0))
}

/// Three-grid study of one solution value without an exact solution.
///
/// Built only by [`grid_convergence`], which refuses a sequence that is not monotone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridConvergence {
    /// Observed order `p̂ = ln[(f₃ − f₂)/(f₂ − f₁)]/ln r`, dimensionless and positive.
    pub observed_order: f64,
    /// Richardson estimate `f₁ + (f₁ − f₂)/(r^p̂ − 1)` of the value at `h = 0`, in the unit of
    /// the inputs.
    pub extrapolated: f64,
    /// `GCI₁₂ = F_s |(f₁ − f₂)/f₁|/(r^p̂ − 1)` of the fine level, as a fraction (multiply by 100
    /// for percent), with `F_s` = [`GCI_SAFETY_FACTOR`].
    pub gci_fine: f64,
    /// `GCI₂₃ = F_s |(f₂ − f₃)/f₂|/(r^p̂ − 1)` of the medium level, as a fraction.
    pub gci_coarse: f64,
    /// `GCI₂₃/(r^p̂ GCI₁₂)`; close to one when the levels are in the asymptotic range.
    pub asymptotic_ratio: f64,
}

/// Observed order, Richardson extrapolation and GCI of three solution values.
///
/// `fine`, `medium` and `coarse` are finite values `f₁, f₂, f₃` of one quantity on levels with
/// the constant ratio `r = h₂/h₁ = h₃/h₂ > 1` (Roache 1994; NASA NPARC). The values are
/// classified first and a sequence that is not monotone returns `NotMonotone` with its
/// behaviour, because a GCI of such a sequence is not defined. A zero `f₁` or `f₂`, which
/// normalizes a GCI, returns `ZeroSolution`.
pub fn grid_convergence(
    fine: f64,
    medium: f64,
    coarse: f64,
    ratio: f64,
) -> Result<GridConvergence, VerifyError> {
    let values = [fine, medium, coarse];
    let behaviour = classify(&values)?;
    check_ratio(ratio)?;
    if behaviour != Behaviour::Monotone {
        return Err(VerifyError::NotMonotone(behaviour));
    }
    for (index, &value) in values[..2].iter().enumerate() {
        if value == 0.0 {
            return Err(VerifyError::ZeroSolution { index });
        }
    }
    let order = libm::log((coarse - medium) / (medium - fine)) / libm::log(ratio);
    let growth = libm::pow(ratio, order);
    let gci_fine = GCI_SAFETY_FACTOR * ((fine - medium) / fine).abs() / (growth - 1.0);
    let gci_coarse = GCI_SAFETY_FACTOR * ((medium - coarse) / medium).abs() / (growth - 1.0);
    Ok(GridConvergence {
        observed_order: order,
        extrapolated: fine + (fine - medium) / (growth - 1.0),
        gci_fine,
        gci_coarse,
        asymptotic_ratio: gci_coarse / (growth * gci_fine),
    })
}

/// Largest relative algebraic error `δ = Δp · ln r / 2` of each error norm of a study.
///
/// `Δp` is [`ORDER_SHIFT_BUDGET`] and `ratio` is the refinement ratio `r`, finite and greater
/// than one; `δ = 3.4657·10⁻³` at `r = 2`.
///
/// Derivation: if the computed errors are `e_c(1 + δ_c)` and `e_f(1 + δ_f)` with
/// `|δ_c|, |δ_f| ≤ δ`, the observed order moves by
/// `[ln(1 + δ_c) − ln(1 + δ_f)]/ln r`, at most `ln((1 + δ)/(1 − δ))/ln r = 2 artanh(δ)/ln r`.
/// To first order this is `2δ/ln r`, and `2δ/ln r ≤ Δp` gives the budget. The exact bound
/// `δ ≤ tanh(Δp ln r / 2)` is smaller by a relative `(Δp ln r)²/12`, `4·10⁻⁶` at `r = 2`, so the
/// budget holds the order shift to `Δp (1 + 4·10⁻⁶)` there.
pub fn algebraic_error_budget(ratio: f64) -> Result<f64, VerifyError> {
    check_ratio(ratio)?;
    Ok(ORDER_SHIFT_BUDGET * libm::log(ratio) / 2.0)
}

/// Relative change of one error norm between a solve and a solve with a tighter tolerance.
///
/// Built only by [`algebraic_check`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AlgebraicCheck {
    /// `|e − e_tight|/e_tight`, dimensionless.
    pub relative_change: f64,
    /// [`algebraic_error_budget`] of the study's refinement ratio.
    pub budget: f64,
}

impl AlgebraicCheck {
    /// Whether the relative change is at or below the budget.
    pub fn within_budget(&self) -> bool {
        self.relative_change <= self.budget
    }

    /// A warning when the relative change exceeds the budget, `None` otherwise.
    ///
    /// The warning says that the iterative error is not below the budget, so the observed
    /// orders of the study may be moved by more than [`ORDER_SHIFT_BUDGET`].
    pub fn warning(&self) -> Option<String> {
        (!self.within_budget()).then(|| {
            format!(
                "the error norm changed by a relative {:.4e} with the solver tolerance divided by \
                 {TOLERANCE_DIVISOR}, above the algebraic error budget {:.4e}",
                self.relative_change, self.budget
            )
        })
    }
}

/// Compares the error norm of a level with the norm from a solve at a tighter tolerance.
///
/// `error_norm` comes from the study's solve and `tighter_error_norm` from the same level
/// solved with the relative tolerance divided by [`TOLERANCE_DIVISOR`]; both are in one unit
/// and must be positive and finite (`NotPositive` with index 0 or 1). The tighter solve is
/// the reference of the relative change. `ratio` is the study's refinement ratio.
pub fn algebraic_check(
    error_norm: f64,
    tighter_error_norm: f64,
    ratio: f64,
) -> Result<AlgebraicCheck, VerifyError> {
    check_positive(error_norm, "error", 0)?;
    check_positive(tighter_error_norm, "error", 1)?;
    let budget = algebraic_error_budget(ratio)?;
    Ok(AlgebraicCheck {
        relative_change: (error_norm - tighter_error_norm).abs() / tighter_error_norm,
        budget,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each libm logarithm is within one ulp, and the order divides two of them: a few
    // epsilon covers the rounding of a synthetic order on exactly representable errors.
    // Near an order of two the two logarithms (at most 1 ulp, so a relative epsilon each), the
    // division (half a ulp) and the rounded ratio of errors inside the first logarithm add up to
    // about 3 epsilon of relative error, 6 epsilon of absolute error; 8 epsilon keeps the bound
    // above that without hiding a second source of error.
    const ROUNDING: f64 = 8.0 * f64::EPSILON;

    #[test]
    fn two_level_order_of_power_law_errors() {
        // e = 3 h² at h = 1/4 and h = 1/8.
        let order = observed_order(3.0 / 16.0, 3.0 / 64.0, 2.0).unwrap();
        assert!((order - 2.0).abs() <= ROUNDING, "{order}");
        // e = h at h = 1 and h = 1/3.
        let order = observed_order(1.0, 1.0 / 3.0, 3.0).unwrap();
        assert!((order - 1.0).abs() <= ROUNDING, "{order}");
        assert_eq!(
            observed_order(0.0, 1.0, 2.0),
            Err(VerifyError::NotPositive {
                quantity: "error",
                index: 0
            })
        );
        assert_eq!(
            observed_order(1.0, 0.5, 1.0),
            Err(VerifyError::InvalidRatio)
        );
        assert_eq!(
            observed_order(1.0, f64::NAN, 2.0),
            Err(VerifyError::NotPositive {
                quantity: "error",
                index: 1
            })
        );
    }

    #[test]
    fn least_squares_recovers_the_order_of_power_law_errors() {
        // Five levels h = 2⁻¹ … 2⁻⁵ with e = 0.7 h², exact in binary64 up to the product.
        let spacings: Vec<f64> = (1..=5).map(|k| libm::ldexp(1.0, -k)).collect();
        let errors: Vec<f64> = spacings.iter().map(|h| 0.7 * h * h).collect();
        let order = fitted_order(&spacings, &errors).unwrap();
        assert!((order - 2.0).abs() <= ROUNDING, "{order}");
        let orders = pairwise_orders(&spacings, &errors).unwrap();
        assert_eq!(orders.len(), 4);
        assert!(
            orders.iter().all(|p| (p - 2.0).abs() <= ROUNDING),
            "{orders:?}"
        );
        // Non-uniform ratios with e = h^1.5, listed coarse to fine and fine to coarse.
        let spacings = [1.0, 0.6, 0.35, 0.2];
        let errors: Vec<f64> = spacings.iter().map(|&h| libm::pow(h, 1.5)).collect();
        let order = fitted_order(&spacings, &errors).unwrap();
        // pow and log add a few ulps to each xᵢ and yᵢ (|yᵢ| < 2.5, ulp 2 epsilon); the slope
        // divides Σ|xᵢ − x̄| ≈ 2.15 times that by Σ(xᵢ − x̄)² ≈ 1.44, which stays well below
        // 64 epsilon.
        assert!((order - 1.5).abs() <= 64.0 * f64::EPSILON, "{order}");
        let reversed_spacings: Vec<f64> = spacings.iter().rev().copied().collect();
        let reversed_errors: Vec<f64> = errors.iter().rev().copied().collect();
        let reversed = fitted_order(&reversed_spacings, &reversed_errors).unwrap();
        assert!((reversed - 1.5).abs() <= 64.0 * f64::EPSILON, "{reversed}");
    }

    #[test]
    fn least_squares_averages_a_perturbed_sequence() {
        // y = 2x + δᵢ with x = −k ln 2 (k = 0..3) and δ = ln 1.1 at k = 1 and 3:
        // Σ(xᵢ − x̄)δᵢ = (0.5 − 1.5) ln 2 · ln 1.1 and Σ(xᵢ − x̄)² = 5 (ln 2)², so
        // p̂ = 2 − ln 1.1/(5 ln 2) = 1.972499...
        let spacings = [1.0, 0.5, 0.25, 0.125];
        let errors = [1.0, 0.25 * 1.1, 0.0625, 0.015625 * 1.1];
        let order = fitted_order(&spacings, &errors).unwrap();
        let expected = 2.0 - libm::log(1.1) / (5.0 * libm::log(2.0));
        // The perturbed errors carry one rounding each; 64 epsilon covers it as above.
        assert!((order - expected).abs() <= 64.0 * f64::EPSILON, "{order}");
        // The derivation prints six decimals; the exact value is 1.9724992 (difference 2e-7), so
        // 1e-6 is the unit of the last printed digit.
        assert!((order - 1.972_499).abs() < 1e-6, "{order}");
    }

    #[test]
    fn fits_reject_inputs_without_an_order() {
        assert_eq!(
            fitted_order(&[1.0, 0.5], &[1.0, 0.25]),
            Err(VerifyError::TooFewLevels {
                needed: 3,
                given: 2
            })
        );
        assert_eq!(
            fitted_order(&[1.0, 0.5, 0.25], &[1.0, 0.25]),
            Err(VerifyError::LengthMismatch {
                spacings: 3,
                errors: 2
            })
        );
        assert_eq!(
            fitted_order(&[1.0, 0.5, 0.25], &[1.0, -0.25, 0.1]),
            Err(VerifyError::NotPositive {
                quantity: "error",
                index: 1
            })
        );
        assert_eq!(
            fitted_order(&[1.0, f64::INFINITY, 0.25], &[1.0, 0.25, 0.1]),
            Err(VerifyError::NotPositive {
                quantity: "spacing",
                index: 1
            })
        );
        assert_eq!(
            fitted_order(&[0.5, 0.5, 0.5], &[1.0, 0.25, 0.1]),
            Err(VerifyError::CoincidentSpacings)
        );
        assert_eq!(
            pairwise_orders(&[1.0, 1.0], &[1.0, 0.5]),
            Err(VerifyError::CoincidentSpacings)
        );
        assert_eq!(
            pairwise_orders(&[1.0], &[1.0]),
            Err(VerifyError::TooFewLevels {
                needed: 2,
                given: 1
            })
        );
    }

    /// Four levels `h = 1, 1/2, 1/4, 1/8` whose pairwise orders are `orders`, coarsest first:
    /// `e₀ = 1` and `eᵢ₊₁ = eᵢ 2^(−pᵢ)`.
    fn study_with_orders(orders: [f64; 3]) -> ([f64; 4], [f64; 4]) {
        let mut errors = [1.0; 4];
        for (i, order) in orders.iter().enumerate() {
            errors[i + 1] = errors[i] * libm::pow(2.0, -order);
        }
        ([1.0, 0.5, 0.25, 0.125], errors)
    }

    // Orders of `study_with_orders`: each error carries at most three pow and three product
    // roundings (4.5 epsilon relative), its logarithm one more ulp of a value below 4 (2
    // epsilon), and an order divides a difference of two such logarithms by ln 2, about 13
    // epsilon; the fit and the spread combine two of them. 64 epsilon stays above that without
    // hiding a second source of error.
    const DERIVED_ORDER: f64 = 64.0 * f64::EPSILON;

    #[test]
    fn band_check_accepts_a_fit_inside_the_derived_band() {
        // Distances 0.32, 0.08, 0.02 to p_f = 2 shrink by ρ = 4: p̂ = (1.92 + 1.98)/2 = 1.95,
        // s = 0.06, δ = 0.01 + 1.5 · 0.06 = 0.10, and |p̂ − p_f| = 0.05 = s (ρ + 1)/(2 (ρ − 1)).
        let (h, e) = study_with_orders([1.68, 1.92, 1.98]);
        let check = band_check(&h, &e, 2.0).unwrap();
        assert!(
            (check.fitted_order - 1.95).abs() <= DERIVED_ORDER,
            "{check:?}"
        );
        assert!((check.spread - 0.06).abs() <= DERIVED_ORDER, "{check:?}");
        assert!(
            (check.half_width - 0.10).abs() <= 2.0 * DERIVED_ORDER,
            "{check:?}"
        );
        assert!(
            (check.band[0] - 1.90).abs() <= 4.0 * DERIVED_ORDER,
            "{check:?}"
        );
        assert!(
            (check.band[1] - 2.10).abs() <= 4.0 * DERIVED_ORDER,
            "{check:?}"
        );
        assert!(
            (check.finest_distance - 0.02).abs() <= DERIVED_ORDER,
            "{check:?}"
        );
        // ρ = d₂/d₃ with d₂ = 0.08 and d₃ = 0.02, each off by DERIVED_ORDER: a relative error
        // of at most 64 ε (1/0.08 + 1/0.02) = 62.5 · 64 ε, times ρ = 4, is 3.6e-12.
        assert!((check.contraction - 4.0).abs() <= 4e-12, "{check:?}");
        assert_eq!(check.pairwise_orders.len(), 3);
        assert!(check.in_band && check.contraction_holds && check.passes());
        assert_eq!(check.failure(), None);
        // The fit and the pairwise orders are those of the separate functions, bit for bit.
        assert_eq!(check.fitted_order, fitted_order(&h[1..], &e[1..]).unwrap());
        assert_eq!(check.pairwise_orders, pairwise_orders(&h, &e).unwrap());
    }

    #[test]
    fn band_check_flags_a_shrink_rate_below_two() {
        // Distances 0.024 and 0.016 shrink by ρ = 1.5: s = 0.008, p̂ = 1.98 and δ = 0.022. The
        // fit lies inside, |p̂ − p_f| = 0.02, but the band is derived only for ρ ≥ 2 and
        // |d₃| = 0.016 > Δp, so the study fails.
        let (h, e) = study_with_orders([1.95, 1.976, 1.984]);
        let check = band_check(&h, &e, 2.0).unwrap();
        assert!(
            (check.fitted_order - 1.98).abs() <= DERIVED_ORDER,
            "{check:?}"
        );
        assert!(
            (check.half_width - 0.022).abs() <= 2.0 * DERIVED_ORDER,
            "{check:?}"
        );
        // d₂ = 0.024, d₃ = 0.016: 64 ε (1/0.024 + 1/0.016) · 1.5 is 2.2e-12.
        assert!((check.contraction - 1.5).abs() <= 3e-12, "{check:?}");
        assert!(check.in_band, "{check:?}");
        assert!(!check.contraction_holds && !check.passes(), "{check:?}");
        assert!(
            check
                .failure()
                .is_some_and(|f| f.starts_with("distances to the formal order")),
            "{check:?}"
        );
        // The same shrink rate with the finest distance inside Δp needs no rate: distances
        // 0.012 and 0.008 give s = 0.004, p̂ = 1.99 and δ = 0.016 ≥ |p̂ − p_f| = 0.01.
        let (h, e) = study_with_orders([1.95, 1.988, 1.992]);
        let check = band_check(&h, &e, 2.0).unwrap();
        // 64 ε (1/0.012 + 1/0.008) · 1.5 is 4.4e-12.
        assert!((check.contraction - 1.5).abs() <= 5e-12, "{check:?}");
        assert!(
            check.in_band && check.contraction_holds && check.passes(),
            "{check:?}"
        );
    }

    #[test]
    fn band_check_rejects_a_super_optimal_order() {
        // e = h³ on h = 2⁰ … 2⁻³, exact in binary64: every pairwise order is the same quotient
        // ln 8/ln 2, so s = 0 exactly and δ = Δp. The fit 3 lies outside 2 ± 0.01.
        let h = [1.0, 0.5, 0.25, 0.125];
        let e = [1.0, 0.125, 0.015625, 0.001953125];
        let check = band_check(&h, &e, 2.0).unwrap();
        assert_eq!(check.spread, 0.0);
        assert_eq!(check.half_width, ORDER_SHIFT_BUDGET);
        assert!((check.fitted_order - 3.0).abs() <= ROUNDING, "{check:?}");
        assert!(!check.in_band && !check.passes(), "{check:?}");
        assert!(
            check
                .failure()
                .is_some_and(|f| f.starts_with("fitted order")),
            "{check:?}"
        );
    }

    #[test]
    fn band_check_rejects_inputs_outside_its_derivation() {
        let h = [1.0, 0.5, 0.25, 0.125];
        let e = [1.0, 0.25, 0.0625, 0.015625];
        assert_eq!(
            band_check(&h[1..], &e[1..], 2.0),
            Err(VerifyError::TooFewLevels {
                needed: 4,
                given: 3
            })
        );
        assert_eq!(
            band_check(&[1.0, 0.5, 0.25, 0.2], &e, 2.0),
            Err(VerifyError::NonConstantRatio { index: 2 })
        );
        let reversed_h: Vec<f64> = h.iter().rev().copied().collect();
        let reversed_e: Vec<f64> = e.iter().rev().copied().collect();
        assert_eq!(
            band_check(&reversed_h, &reversed_e, 2.0),
            Err(VerifyError::InvalidRatio)
        );
        assert_eq!(band_check(&h, &e, 0.0), Err(VerifyError::InvalidOrder));
        assert_eq!(band_check(&h, &e, f64::NAN), Err(VerifyError::InvalidOrder));
        assert_eq!(
            band_check(&h, &[1.0, 0.25, f64::NAN, 0.015625], 2.0),
            Err(VerifyError::NotPositive {
                quantity: "error",
                index: 2
            })
        );
    }

    #[test]
    fn classifies_monotone_oscillatory_and_divergent_sequences() {
        // R = 0.00196/0.00676 for the NPARC values.
        assert_eq!(
            classify(&[0.97050, 0.96854, 0.96178]),
            Ok(Behaviour::Monotone)
        );
        // R = 0.1/(−0.3).
        assert_eq!(classify(&[1.0, 1.1, 0.8]), Ok(Behaviour::Oscillatory));
        // R = 0.5/0.2.
        assert_eq!(classify(&[1.0, 1.5, 1.7]), Ok(Behaviour::Divergent));
        // The first triplet is monotone (R = 1/2), the second divergent (R = 2/1).
        assert_eq!(classify(&[0.0, 1.0, 3.0, 4.0]), Ok(Behaviour::Divergent));
        assert_eq!(
            classify(&[1.0, 1.0, 2.0]),
            Err(VerifyError::UnchangedSolution { index: 0 })
        );
        assert_eq!(
            classify(&[1.0, 2.0, f64::NAN]),
            Err(VerifyError::NotFinite { index: 2 })
        );
        assert_eq!(
            classify(&[1.0, 2.0]),
            Err(VerifyError::TooFewLevels {
                needed: 3,
                given: 2
            })
        );
    }

    #[test]
    fn reproduces_the_nparc_spatial_convergence_example() {
        // NASA NPARC, "Examining Spatial (Grid) Convergence": pressure recovery on grids of
        // spacing 1, 2 and 4. The page prints p = 1.786170, Pr(h=0) = 0.97130,
        // GCI12 = 0.103083 %, GCI23 = 0.356249 % and the asymptotic check 1.002019.
        let study = grid_convergence(0.97050, 0.96854, 0.96178, 2.0).unwrap();
        // Half a unit of the last printed digit: the page rounds, it does not truncate.
        assert!(
            (study.observed_order - 1.786_170).abs() <= 5e-7,
            "{study:?}"
        );
        assert!((study.extrapolated - 0.971_30).abs() <= 5e-6, "{study:?}");
        assert!(
            (100.0 * study.gci_fine - 0.103_083).abs() <= 5e-7,
            "{study:?}"
        );
        assert!(
            (100.0 * study.gci_coarse - 0.356_249).abs() <= 5e-7,
            "{study:?}"
        );
        // The page evaluates the check from its rounded GCIs; their last digits carry a
        // relative 1.4e-6 and 4.9e-6, so the printed ratio is good to about 1e-5.
        assert!(
            (study.asymptotic_ratio - 1.002_019).abs() <= 1e-5,
            "{study:?}"
        );
    }

    #[test]
    fn grid_convergence_refuses_sequences_that_are_not_monotone() {
        assert_eq!(
            grid_convergence(1.0, 1.1, 0.8, 2.0),
            Err(VerifyError::NotMonotone(Behaviour::Oscillatory))
        );
        assert_eq!(
            grid_convergence(1.0, 1.5, 1.7, 2.0),
            Err(VerifyError::NotMonotone(Behaviour::Divergent))
        );
        assert_eq!(
            grid_convergence(0.0, 0.25, 0.75, 2.0),
            Err(VerifyError::ZeroSolution { index: 0 })
        );
        assert_eq!(
            grid_convergence(1.0, 0.5, 0.25, 0.5),
            Err(VerifyError::InvalidRatio)
        );
    }

    #[test]
    fn richardson_extrapolation_removes_the_leading_error_term() {
        // f(h) = 1 + 0.25 h² at h = 1 and h = 2: 1.25 + (1.25 − 2)/(2² − 1) = 1, exactly.
        assert_eq!(richardson_extrapolation(1.25, 2.0, 2.0, 2.0), Ok(1.0));
        assert_eq!(
            richardson_extrapolation(1.25, 2.0, 2.0, 0.0),
            Err(VerifyError::InvalidOrder)
        );
        assert_eq!(
            richardson_extrapolation(f64::NAN, 2.0, 2.0, 2.0),
            Err(VerifyError::NotFinite { index: 0 })
        );
    }

    #[test]
    fn algebraic_budget_bounds_the_order_shift() {
        let budget = algebraic_error_budget(2.0).unwrap();
        // 0.01 ln 2 / 2 = 3.465735902799726…e-3; the spec quotes it rounded down to 3.4e-3.
        // The literal keeps 14 significant digits (error 2.6e-17 against 3.465735902799726e-3);
        // 1e-16 is one unit of its last digit.
        assert!((budget - 3.465_735_902_799_7e-3).abs() <= 1e-16, "{budget}");
        // Errors perturbed by +δ on the coarse and −δ on the fine level move the order by
        // 2 artanh(δ)/ln 2 = 0.01 (1 + 4.0e-6), the bound of the derivation. The factor is
        // 1 + δ²/3 + δ⁴/5 with δ = 3.4657e-3, that is 1 + 4.0037e-6 + 2.9e-11; 4.1e-6 leaves
        // 2.5 percent for that tail and for the rounding of the order.
        let order = observed_order(0.25 * (1.0 + budget), 0.0625 * (1.0 - budget), 2.0).unwrap();
        let shift = order - 2.0;
        assert!(shift > ORDER_SHIFT_BUDGET, "{shift}");
        assert!(shift <= ORDER_SHIFT_BUDGET * (1.0 + 4.1e-6), "{shift}");
        assert_eq!(algebraic_error_budget(1.0), Err(VerifyError::InvalidRatio));
        assert_eq!(
            algebraic_error_budget(f64::INFINITY),
            Err(VerifyError::InvalidRatio)
        );
    }

    #[test]
    fn algebraic_check_warns_above_the_budget() {
        // 1e-3/1.001 is below 3.47e-3.
        let check = algebraic_check(1.002, 1.001, 2.0).unwrap();
        assert!(check.within_budget(), "{check:?}");
        assert_eq!(check.warning(), None);
        // 1e-2/1.0 is above it.
        let check = algebraic_check(1.01, 1.0, 2.0).unwrap();
        assert!(!check.within_budget(), "{check:?}");
        assert!(check.warning().is_some_and(|w| w.contains("budget")));
        // At the budget the check passes ("at or below").
        let budget = algebraic_error_budget(2.0).unwrap();
        let at_budget = AlgebraicCheck {
            relative_change: budget,
            budget,
        };
        assert!(at_budget.within_budget());
        assert_eq!(
            algebraic_check(1.0, 0.0, 2.0),
            Err(VerifyError::NotPositive {
                quantity: "error",
                index: 1
            })
        );
    }
}
