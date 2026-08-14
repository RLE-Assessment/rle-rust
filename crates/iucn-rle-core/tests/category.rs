//! Category ranking and selection.
//!
//! Ranking is load-bearing: the overall category of an assessment is the
//! *most threatened* across criteria, which rle-python expresses as
//! `min(categories, key=_CATEGORY_RANK.get)`.

use iucn_rle_core::Category;

#[test]
fn ranks_run_from_collapsed_to_not_evaluated() {
    // rle.py:100 — _CATEGORY_RANK
    assert_eq!(Category::Co.rank(), 0);
    assert_eq!(Category::Cr.rank(), 1);
    assert_eq!(Category::En.rank(), 2);
    assert_eq!(Category::Vu.rank(), 3);
    assert_eq!(Category::Nt.rank(), 4);
    assert_eq!(Category::Lc.rank(), 5);
    assert_eq!(Category::Dd.rank(), 6);
    assert_eq!(Category::Ne.rank(), 7);
}

#[test]
fn ordering_puts_most_threatened_first() {
    // Ord is by rank, so `min` IS "most threatened" and cannot drift from it.
    assert!(Category::Cr < Category::En);
    assert!(Category::En < Category::Vu);
    assert!(Category::Vu < Category::Lc);
}

#[test]
fn most_threatened_of_several_is_the_minimum() {
    let found = [Category::Lc, Category::En, Category::Vu]
        .into_iter()
        .min()
        .unwrap();
    assert_eq!(found, Category::En);
}

#[test]
fn threatened_categories_are_co_cr_en_vu() {
    assert!(Category::Co.is_threatened());
    assert!(Category::Cr.is_threatened());
    assert!(Category::En.is_threatened());
    assert!(Category::Vu.is_threatened());

    assert!(!Category::Nt.is_threatened());
    assert!(!Category::Lc.is_threatened());
    // Data Deficient and Not Evaluated are the absence of an assessment, not a
    // finding of safety — but they are not *threatened* findings either.
    assert!(!Category::Dd.is_threatened());
    assert!(!Category::Ne.is_threatened());
}

#[test]
fn codes_round_trip_through_parsing() {
    for category in Category::ALL {
        assert_eq!(category.code().parse::<Category>().unwrap(), category);
    }
}

#[test]
fn parsing_an_unknown_code_fails() {
    assert!("XX".parse::<Category>().is_err());
}
