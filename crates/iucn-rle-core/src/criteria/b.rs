//! Criterion B: restricted geographic distribution.
//!
//! IUCN (2024) Guidelines v2.0, Section 6 (pp. 65-76) and Appendix 1 (Criteria v2.1,
//! pp. 154-155).
//!
//! The evaluation runs **per category level**, not threshold-then-gate. Clause (c) is
//! category dependent — 1 threat-defined location for CR, <= 5 for EN, <= 10 for VU —
//! so a sub-criterion is met at level *L* only when the spatial threshold and the gate
//! both hold *at L*. Computing a threshold category first and then applying a boolean
//! gate overstates the threat whenever (c) is the only qualifying clause.

use crate::assessment::{overall, Assessment, CriterionResult, Note, Provenance};
use crate::{
    Category, CategoryRange, ConditionStatus, CriterionId, Estimate, NoThresholdTable,
    Subconditions, ThreatLocations, ThresholdTable,
};

/// Whether the sub-condition gate is satisfied at a particular category level.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Gate {
    /// A clause is established at this level.
    Met,
    /// Every clause has been assessed and none holds at this level.
    NotMet,
    /// No clause is established, but something is still unassessed.
    Unknown,
}

/// Evaluate clauses (a), (b) and (c) at one category level.
fn gate_at(
    subs: &Subconditions,
    table: &ThresholdTable,
    criterion: CriterionId,
    level: Category,
) -> Gate {
    let mut any_unknown = false;

    for status in [subs.continuing_decline(), subs.threatening_processes()] {
        match status {
            ConditionStatus::Met => return Gate::Met,
            ConditionStatus::NotAssessed => any_unknown = true,
            ConditionStatus::NotMet => {}
        }
    }

    // Clause (c): a count compared against this level's bound.
    match subs.locations() {
        ThreatLocations::Count(n) => {
            if table
                .max_locations(criterion, level)
                .is_some_and(|max| n <= max)
            {
                return Gate::Met;
            }
        }
        // A finding that no threats exist: (c) is not met, and cannot become met.
        ThreatLocations::NoPlausibleThreats => {}
        ThreatLocations::InsufficientInformation | ThreatLocations::NotAssessed => {
            any_unknown = true;
        }
    }

    if any_unknown {
        Gate::Unknown
    } else {
        Gate::NotMet
    }
}

/// The most threatened level a sub-criterion reaches, under one reading of the unknowns.
///
/// `optimistic` treats unassessed clauses as not met (the least threatened reading);
/// otherwise they are treated as met (the precautionary reading). Propagating both is
/// what Section 6.3.2 p.70 asks for.
fn level_reached(
    criterion: CriterionId,
    metric: f64,
    subs: &Subconditions,
    table: &ThresholdTable,
    optimistic: bool,
) -> Category {
    table
        .levels(criterion)
        .into_iter()
        .find(|&level| {
            if !table.metric_meets(criterion, level, metric) {
                return false;
            }
            match gate_at(subs, table, criterion, level) {
                Gate::Met => true,
                Gate::Unknown => !optimistic,
                Gate::NotMet => false,
            }
        })
        .unwrap_or(Category::Lc)
}

/// Assess Criterion B from its spatial metrics and sub-conditions.
///
/// `eoo_km2` is the extent of occurrence in square kilometres (B1) and `aoo_cells` the
/// number of occupied 10 x 10 km cells after the 1% small-patch exclusion (B2). Either
/// may be `None`, in which case that sub-criterion is reported as `NE` without
/// affecting the other. B3 needs no spatial metric at all.
///
/// The spatial thresholds alone do not produce a listing: Criterion B also requires at
/// least one of clauses (a), (b) or (c). Where a clause is unassessed the outcome is a
/// range rather than a firm category, which is what the Guidelines require:
///
/// > "In cases where continuing declines are equally likely to be occurring or not
/// > occurring, upper and lower bounds of the status under criterion B should be
/// > estimated by propagating both scenarios through the criteria." (Section 6.3.2, p. 70)
///
/// # Errors
///
/// Returns [`NoThresholdTable`] if `table` has no rules for B1, B2 or B3.
///
/// ```
/// use iucn_rle_core::{criterion_b, Basis, Estimate, Subconditions, ThresholdTable};
///
/// // 15 000 km2 EOO meets the EN threshold, but nobody has checked (a)/(b)/(c).
/// let eoo = Estimate::point(15_000.0, Basis::Estimated);
/// let assessment = criterion_b(Some(eoo), None, &Subconditions::new(), ThresholdTable::v2_2024())?;
/// assert_eq!(assessment.category().to_string(), "EN (LC-EN)");
/// # Ok::<(), iucn_rle_core::NoThresholdTable>(())
/// ```
pub fn criterion_b(
    eoo_km2: Option<Estimate>,
    aoo_cells: Option<Estimate>,
    subconditions: &Subconditions,
    table: &ThresholdTable,
) -> Result<Assessment, NoThresholdTable> {
    let results = vec![
        spatial_sub_criterion(CriterionId::B1, eoo_km2, subconditions, table)?,
        spatial_sub_criterion(CriterionId::B2, aoo_cells, subconditions, table)?,
        b3(subconditions, table)?,
    ];

    let category = overall(&results);
    let provenance = Provenance::new(table.guidelines_version(), table.sha256());

    Ok(Assessment::new(results, category, provenance))
}

/// Note when the Box 13 step 6(iv) Near Threatened pathway might apply.
///
/// That rule depends on judgements the engine cannot make — whether information is
/// "insufficient" and whether less than 30% of the distribution is unthreatened — so it
/// is surfaced for the assessor rather than computed.
fn near_threatened_note(criterion: CriterionId, subs: &Subconditions) -> Option<Note> {
    let ThreatLocations::Count(n) = subs.locations() else {
        return None;
    };
    let trigger = match criterion {
        CriterionId::B1 | CriterionId::B2 => 9,
        CriterionId::B3 => 4,
        _ => return None,
    };
    (n <= trigger).then_some(Note::NearThreatenedMayApply {
        locations: n,
        max_locations: trigger,
    })
}

fn spatial_sub_criterion(
    criterion: CriterionId,
    metric: Option<Estimate>,
    subs: &Subconditions,
    table: &ThresholdTable,
) -> Result<CriterionResult, NoThresholdTable> {
    let Some(metric) = metric else {
        return Ok(CriterionResult::new(
            criterion,
            None,
            None,
            subs.clone(),
            CategoryRange::point(Category::Ne),
            vec![Note::MetricMissing],
        ));
    };

    // Thresholds alone, before the gate — retained so a reviewer can see what the
    // spatial data said independently of the sub-conditions.
    let threshold_category = table.classify_estimate(criterion, &metric)?;

    // Insufficient information about locations yields Data Deficient, but only when no
    // other clause has settled the question (Box 13 step 5).
    let undecidable = subs.locations() == ThreatLocations::InsufficientInformation
        && subs.continuing_decline() != ConditionStatus::Met
        && subs.threatening_processes() != ConditionStatus::Met;

    let mut notes = Vec::new();

    if threshold_category.plausible_most() == Category::Lc {
        // Above every spatial threshold: the criterion is not triggered and the
        // sub-conditions are irrelevant.
        notes.push(Note::MetricAboveAllThresholds);
        return Ok(CriterionResult::new(
            criterion,
            Some(metric),
            Some(threshold_category),
            subs.clone(),
            threshold_category,
            notes,
        ));
    }

    if undecidable {
        notes.push(Note::LocationsInsufficientInformation);
        return Ok(CriterionResult::new(
            criterion,
            Some(metric),
            Some(threshold_category),
            subs.clone(),
            CategoryRange::point(Category::Dd),
            notes,
        ));
    }

    let (best, lower_metric, upper_metric) = metric_bounds(&metric);
    let precautionary = level_reached(criterion, lower_metric, subs, table, false);
    let optimistic = level_reached(criterion, upper_metric, subs, table, true);
    let best_level = level_reached(criterion, best, subs, table, false);

    let pending = subs.pending();
    if !pending.is_empty() {
        notes.push(Note::SubconditionsNotAssessed {
            pending: pending.iter().map(|s| (*s).to_owned()).collect(),
        });
    } else if precautionary == Category::Lc {
        notes.push(Note::SubconditionsRefuted);
    }
    notes.extend(near_threatened_note(criterion, subs));

    let category = CategoryRange::span(best_level, optimistic, precautionary);

    Ok(CriterionResult::new(
        criterion,
        Some(metric),
        Some(threshold_category),
        subs.clone(),
        category,
        notes,
    ))
}

/// The metric's best estimate and its bounds, defaulting to the best estimate.
fn metric_bounds(metric: &Estimate) -> (f64, f64, f64) {
    let best = metric.best();
    (
        best,
        metric.lower().unwrap_or(best),
        metric.upper().unwrap_or(best),
    )
}

/// B3: a very small number of threat-defined locations, and capable of collapse or
/// becoming CR within a very short time period. Can only ever yield VU (Section 6.3.3).
fn b3(subs: &Subconditions, table: &ThresholdTable) -> Result<CriterionResult, NoThresholdTable> {
    let max = table
        .max_locations(CriterionId::B3, Category::Vu)
        .ok_or(NoThresholdTable(
            CriterionId::B3,
            table.guidelines_version(),
        ))?;

    let mut notes = Vec::new();

    // No evidence at all on either limb means nobody attempted B3. That is Not
    // Evaluated, not a range — a range would imply an assessment we never made.
    if subs.locations() == ThreatLocations::NotAssessed
        && subs.capable_of_rapid_collapse() == ConditionStatus::NotAssessed
    {
        return Ok(CriterionResult::new(
            CriterionId::B3,
            None,
            None,
            subs.clone(),
            CategoryRange::point(Category::Ne),
            vec![Note::B3NotAssessed],
        ));
    }

    let few_locations = match subs.locations() {
        ThreatLocations::Count(n) => {
            if n <= max {
                ConditionStatus::Met
            } else {
                ConditionStatus::NotMet
            }
        }
        ThreatLocations::NoPlausibleThreats => ConditionStatus::NotMet,
        ThreatLocations::InsufficientInformation => {
            notes.push(Note::LocationsInsufficientInformation);
            return Ok(CriterionResult::new(
                CriterionId::B3,
                None,
                None,
                subs.clone(),
                CategoryRange::point(Category::Dd),
                notes,
            ));
        }
        ThreatLocations::NotAssessed => ConditionStatus::NotAssessed,
    };

    // Both limbs must hold. Either being ruled out settles B3 as not triggered.
    let limbs = [few_locations, subs.capable_of_rapid_collapse()];
    let category = if limbs.contains(&ConditionStatus::NotMet) {
        CategoryRange::point(Category::Lc)
    } else if limbs.iter().all(|s| *s == ConditionStatus::Met) {
        CategoryRange::point(Category::Vu)
    } else {
        notes.push(Note::B3NotAssessed);
        CategoryRange::span(Category::Vu, Category::Lc, Category::Vu)
    };

    notes.extend(near_threatened_note(CriterionId::B3, subs));

    Ok(CriterionResult::new(
        CriterionId::B3,
        None,
        None,
        subs.clone(),
        category,
        notes,
    ))
}
