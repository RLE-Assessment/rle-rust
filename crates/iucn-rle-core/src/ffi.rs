//! The entry point every language binding calls.
//!
//! Python, R, JavaScript, Julia, and the CLI all need to turn loosely-typed
//! input — optional floats and pairs of strings — into the strongly-typed core
//! API, and to flatten the result back out. Doing that once here rather than
//! five times in five languages is what keeps the bindings thin enough to trust,
//! and it means an input-validation bug can only exist in one place.
//!
//! Errors are returned as plain `String` deliberately: a rich error type buys
//! nothing across an FFI boundary that can only carry a message anyway.

use serde::{Deserialize, Serialize};

use crate::distribution::DistributionAccumulator;
use crate::grid::{AOO_CELL_SIZE_M, AOO_CRS};
use crate::{
    criterion_b, Basis, ConditionStatus, DeclineAspect, Estimate, Subconditions, Summary,
    ThreatLocations, ThresholdTable,
};

/// A metric value with optional plausible bounds, as supplied by a caller.
#[derive(Copy, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct MetricInput {
    /// The best estimate.
    pub best: f64,
    /// An optional plausible lower bound.
    pub lower: Option<f64>,
    /// An optional plausible upper bound.
    pub upper: Option<f64>,
}

impl MetricInput {
    /// A metric with no stated uncertainty.
    #[must_use]
    pub const fn point(best: f64) -> Self {
        Self {
            best,
            lower: None,
            upper: None,
        }
    }

    /// A metric with plausible bounds.
    #[must_use]
    pub const fn bounded(best: f64, lower: f64, upper: f64) -> Self {
        Self {
            best,
            lower: Some(lower),
            upper: Some(upper),
        }
    }

    fn into_estimate(self) -> Estimate {
        // Bindings do not carry an evidence basis yet; `Estimated` is the
        // neutral default and is recorded so it can be overridden later without
        // changing the shape of this type.
        match (self.lower, self.upper) {
            (Some(lower), Some(upper)) => {
                Estimate::bounded(self.best, lower, upper, Basis::Estimated)
            }
            _ => Estimate::point(self.best, Basis::Estimated),
        }
    }
}

/// A clause and its status, as supplied by a caller.
///
/// `sub` accepts `"a"` or `"b"`, the decline aspects `"a.i"` / `"a.ii"` / `"a.iii"`,
/// or `"b3_rapid_collapse"` for B3's second limb. Clause (c) is **not** given here — it
/// is a count of threat-defined locations, supplied via
/// [`SubconditionsInput::locations`].
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SubconditionInput {
    /// Which clause this concerns.
    pub sub: String,
    /// `"met"`, `"not_met"`, or `"not_assessed"`.
    pub status: String,
}

/// All Criterion B evidence, as supplied by a caller.
#[derive(Clone, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SubconditionsInput {
    /// Clause (a) aspects, clause (b), and B3's rapid-collapse limb.
    pub clauses: Vec<SubconditionInput>,
    /// Clause (c): the number of threat-defined locations, when counted.
    pub locations: Option<u32>,
    /// No plausible threats exist, so clause (c) and B3 are **not met** — a finding,
    /// distinct from an absent count, which means nobody looked.
    pub no_plausible_threats: bool,
    /// Threats exist but their extent cannot be assessed: Data Deficient.
    pub locations_insufficient_information: bool,
}

impl SubconditionsInput {
    fn into_subconditions(self) -> Result<Subconditions, String> {
        let mut subs = Subconditions::new();

        for clause in &self.clauses {
            let status: ConditionStatus = clause.status.parse().map_err(|e| format!("{e}"))?;
            let key = clause.sub.trim().to_ascii_lowercase();
            subs = match key.as_str() {
                // A bare "a" sets every aspect, for callers that do not distinguish them.
                "a" | "continuing_decline" => DeclineAspect::ALL
                    .into_iter()
                    .fold(subs, |acc, aspect| acc.with_decline(aspect, status)),
                "b" | "threatening_processes" => subs.with_threatening_processes(status),
                "b3_rapid_collapse" | "capable_of_rapid_collapse" => {
                    subs.with_capable_of_rapid_collapse(status)
                }
                "c" | "few_locations" => {
                    return Err("clause (c) is a count of threat-defined locations, not a \
                                status; supply it as `locations` instead"
                        .to_owned())
                }
                other => {
                    let aspect: DeclineAspect = other
                        .strip_prefix("a.")
                        .unwrap_or(other)
                        .parse()
                        .map_err(|_| {
                        format!(
                            "unrecognised clause: {:?}; expected \
                                     a|b|a.i|a.ii|a.iii|b3_rapid_collapse",
                            clause.sub
                        )
                    })?;
                    subs.with_decline(aspect, status)
                }
            };
        }

        let locations = match (
            self.locations,
            self.no_plausible_threats,
            self.locations_insufficient_information,
        ) {
            (Some(_), true, _) | (Some(_), _, true) | (None, true, true) => {
                return Err(
                    "threat-defined locations are over-specified: give at most one of \
                            a count, no_plausible_threats, or \
                            locations_insufficient_information"
                        .to_owned(),
                )
            }
            (Some(n), _, _) => ThreatLocations::Count(n),
            (None, true, false) => ThreatLocations::NoPlausibleThreats,
            (None, false, true) => ThreatLocations::InsufficientInformation,
            (None, false, false) => ThreatLocations::NotAssessed,
        };

        Ok(subs.with_locations(locations))
    }
}

/// Assess Criterion B from loosely-typed input.
///
/// Anything not supplied counts as **not assessed**, which produces a provisional range
/// rather than a definite category.
///
/// # Errors
///
/// Returns a human-readable message if a clause name or status is not recognised, or if
/// threat-defined locations are over-specified. Absent metrics are not an error; they
/// yield `NE`.
pub fn criterion_b_from_parts(
    eoo_km2: Option<MetricInput>,
    aoo_cells: Option<MetricInput>,
    subconditions: SubconditionsInput,
) -> Result<Summary, String> {
    let subs = subconditions.into_subconditions()?;

    let assessment = criterion_b(
        eoo_km2.map(MetricInput::into_estimate),
        aoo_cells.map(MetricInput::into_estimate),
        &subs,
        ThresholdTable::v2_2024(),
    )
    .map_err(|e| format!("{e}"))?;

    Ok(assessment.summary())
}

/// One polygon from a distribution map, in longitude/latitude degrees.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PolygonInput {
    /// The ecosystem this feature belongs to, usually a Global Ecosystem Typology code.
    pub ecosystem: String,
    /// Exterior ring first, then any holes. Ring *position* decides the role, not
    /// winding — see [`crate::aoo::AooAccumulator::add_polygon`].
    pub rings: Vec<Vec<[f64; 2]>>,
}

/// Criterion B spatial metrics for one ecosystem.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct EcosystemMetrics {
    /// The ecosystem these metrics describe.
    pub ecosystem: String,
    /// Extent of occurrence in km² — the B1 metric.
    pub eoo_km2: f64,
    /// Occupied 10 x 10 km cells after the 1% exclusion — the B2 metric.
    pub aoo_cells: u32,
    /// Cells containing any of the ecosystem, before the exclusion. A different
    /// number from [`Self::aoo_cells`], and not the one B2 uses.
    pub occupied_cell_count: u32,
    /// Whether a cell sat close enough to the 1% cutoff that the count is not robust
    /// to implementation differences, and a human should look.
    pub aoo_near_boundary: bool,
}

/// Spatial metrics for a whole distribution map.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct DistributionSummary {
    /// One entry per ecosystem, sorted by code.
    pub ecosystems: Vec<EcosystemMetrics>,
    /// Cells that received more extent than they can hold, indicating overlapping
    /// features of one ecosystem in the source map.
    pub overfull_cells: u32,
    /// The CRS the grid is defined in, recorded so a cell count can be traced.
    pub grid_crs: String,
    /// Grid cell size in metres.
    pub cell_size_m: f64,
}

/// Validate a coordinate, naming the problem rather than silently coping with it.
fn check_coordinate(ecosystem: &str, [lon, lat]: [f64; 2]) -> Result<(), String> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(format!(
            "{ecosystem}: coordinate ({lon}, {lat}) is not a finite number"
        ));
    }
    // Out-of-range values are not rounding artefacts. They almost always mean the
    // coordinates are swapped, or already projected, and clamping them would produce
    // a plausible-looking but wrong AOO instead of an error.
    if !(-90.0..=90.0).contains(&lat) {
        return Err(format!(
            "{ecosystem}: latitude {lat} is outside -90..90 — are the coordinates \
             swapped, or already projected?"
        ));
    }
    if !(-180.0..=180.0).contains(&lon) {
        return Err(format!(
            "{ecosystem}: longitude {lon} is outside -180..180 — are the coordinates \
             swapped, or already projected?"
        ));
    }
    Ok(())
}

/// Compute Criterion B spatial metrics from a distribution map.
///
/// Polygons are in longitude/latitude degrees on WGS84. They are projected to
/// ESRI:54034 internally, because both metrics require an equal-area CRS.
///
/// # Errors
///
/// Returns a human-readable message if a feature has no exterior ring, or if a
/// coordinate is non-finite or outside valid longitude/latitude range.
///
/// ```
/// use iucn_rle_core::ffi::{distribution_metrics, PolygonInput};
///
/// let summary = distribution_metrics(&[PolygonInput {
///     ecosystem: "T1.1.1".to_owned(),
///     rings: vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]],
/// }])?;
///
/// assert_eq!(summary.ecosystems.len(), 1);
/// assert!(summary.ecosystems[0].eoo_km2 > 12_000.0);
/// # Ok::<(), String>(())
/// ```
pub fn distribution_metrics(polygons: &[PolygonInput]) -> Result<DistributionSummary, String> {
    let mut accumulator = DistributionAccumulator::new();

    for polygon in polygons {
        if polygon.rings.is_empty() || polygon.rings[0].is_empty() {
            return Err(format!(
                "{}: feature has no exterior ring",
                polygon.ecosystem
            ));
        }
        for ring in &polygon.rings {
            for &coordinate in ring {
                check_coordinate(&polygon.ecosystem, coordinate)?;
            }
        }
        accumulator.add_polygon(&polygon.ecosystem, &polygon.rings);
    }

    let distribution = accumulator.finish();

    let ecosystems = distribution
        .ecosystems()
        .iter()
        .map(|code| {
            let aoo = distribution.aoo(code);
            EcosystemMetrics {
                ecosystem: code.clone(),
                eoo_km2: distribution.eoo_km2(code),
                aoo_cells: aoo.aoo_cells,
                occupied_cell_count: aoo.occupied_cell_count,
                aoo_near_boundary: aoo.near_boundary,
            }
        })
        .collect();

    Ok(DistributionSummary {
        ecosystems,
        overfull_cells: distribution.grid().overfull_cells(),
        grid_crs: AOO_CRS.to_owned(),
        cell_size_m: AOO_CELL_SIZE_M,
    })
}
