//! Criterion B: restricted geographic distribution.

use crate::assessment::{overall, Assessment, CriterionResult, Note, Provenance};
use crate::subcondition::{evaluate, Gate};
use crate::{
    Category, CategoryRange, CriterionId, Estimate, NoThresholdTable, SubconditionAssessment,
    ThresholdTable,
};

/// Assess Criterion B from its spatial metrics and sub-conditions.
///
/// `eoo_km2` is the extent of occurrence in square kilometres (B1) and
/// `aoo_cells` the number of occupied 10x10 km cells after the 1% small-patch
/// exclusion (B2). Either may be `None` when unavailable, in which case that
/// sub-criterion is reported as `NE` without affecting the other.
///
/// The spatial thresholds alone do not produce a listing. Criterion B also
/// requires at least one of sub-conditions (a), (b), or (c), so the outcome
/// depends on what `subconditions` records:
///
/// * any sub-condition **met** — the threshold category stands;
/// * all three assessed and **none met** — the criterion is not triggered, `LC`;
/// * none met but some **unassessed** — the honest answer is a range from `LC`
///   to the threshold category, with the best estimate held at the threshold
///   category on precautionary grounds.
///
/// That third case is the common one in practice, and it is why this returns a
/// [`CategoryRange`] rather than a bare [`Category`].
///
/// # Errors
///
/// Returns [`NoThresholdTable`] if `table` has no thresholds for B1 or B2.
///
/// ```
/// use iucn_rle_core::{criterion_b, Basis, Estimate, ThresholdTable};
///
/// // 15 000 km2 EOO meets the EN threshold, but nobody has checked (a)/(b)/(c).
/// let eoo = Estimate::point(15_000.0, Basis::Estimated);
/// let assessment = criterion_b(Some(eoo), None, &[], ThresholdTable::v2_2024())?;
/// assert_eq!(assessment.category().to_string(), "EN (LC-EN)");
/// # Ok::<(), iucn_rle_core::NoThresholdTable>(())
/// ```
pub fn criterion_b(
    eoo_km2: Option<Estimate>,
    aoo_cells: Option<Estimate>,
    subconditions: &[SubconditionAssessment],
    table: &ThresholdTable,
) -> Result<Assessment, NoThresholdTable> {
    let (gate, pending) = evaluate(subconditions);

    let results = vec![
        sub_criterion(
            CriterionId::B1,
            eoo_km2,
            subconditions,
            table,
            gate,
            &pending,
        )?,
        sub_criterion(
            CriterionId::B2,
            aoo_cells,
            subconditions,
            table,
            gate,
            &pending,
        )?,
    ];

    let category = overall(&results);
    let provenance = Provenance::new(table.guidelines_version(), table.sha256());

    Ok(Assessment::new(results, category, provenance))
}

fn sub_criterion(
    criterion: CriterionId,
    metric: Option<Estimate>,
    subconditions: &[SubconditionAssessment],
    table: &ThresholdTable,
    gate: Gate,
    pending: &[crate::Subcondition],
) -> Result<CriterionResult, NoThresholdTable> {
    let Some(metric) = metric else {
        return Ok(CriterionResult::new(
            criterion,
            None,
            None,
            subconditions.to_vec(),
            CategoryRange::point(Category::Ne),
            vec![Note::MetricMissing],
        ));
    };

    let threshold_category = table.classify_estimate(criterion, &metric)?;
    let mut notes = Vec::new();

    // A metric above every threshold does not trigger the criterion, so the
    // sub-conditions are irrelevant and no range is warranted.
    if threshold_category.plausible_most() == Category::Lc {
        notes.push(Note::MetricAboveAllThresholds);
        return Ok(CriterionResult::new(
            criterion,
            Some(metric),
            Some(threshold_category),
            subconditions.to_vec(),
            threshold_category,
            notes,
        ));
    }

    let category = match gate {
        Gate::Satisfied => threshold_category,
        Gate::Refuted => {
            notes.push(Note::SubconditionsRefuted);
            CategoryRange::point(Category::Lc)
        }
        Gate::Unknown => {
            notes.push(Note::SubconditionsNotAssessed {
                pending: pending.to_vec(),
            });
            // Best estimate stays at the threshold outcome (precautionary), but
            // LC remains plausible because a met sub-condition is not in evidence.
            CategoryRange::span(
                threshold_category.best(),
                Category::Lc,
                threshold_category.plausible_most(),
            )
        }
    };

    Ok(CriterionResult::new(
        criterion,
        Some(metric),
        Some(threshold_category),
        subconditions.to_vec(),
        category,
        notes,
    ))
}
