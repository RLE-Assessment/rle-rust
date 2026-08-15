#' Assess IUCN RLE Criterion B
#'
#' Criterion B covers restricted geographic distribution. The spatial thresholds
#' alone do not produce a listing: at least one of clause (a) continuing decline,
#' (b) threatening processes, or (c) few threat-defined locations must also hold.
#'
#' Clauses you do not name count as *not assessed*, which is deliberately
#' different from *not met*. An ecosystem whose metrics say EN but whose clauses
#' nobody has examined is reported as `"EN (LC-EN)"`, not a bare `"EN"` that
#' would overstate confidence.
#'
#' Clause (c) is supplied as `locations`, a **count**, because it is category
#' dependent: 1 threat-defined location for CR, 5 or fewer for EN, 10 or fewer
#' for VU (IUCN 2024 Guidelines v2.0, Appendix 1, pp. 154-155).
#'
#' @param eoo_km2 Extent of occurrence in square kilometres (sub-criterion B1).
#' @param aoo_cells Occupied 10x10 km cells after the 1% exclusion (B2).
#' @param clauses Named character vector, e.g. `c(a = "met")`. Names may be
#'   `"a"` (or its aspects `"a.i"`, `"a.ii"`, `"a.iii"`), `"b"`, or
#'   `"b3_rapid_collapse"`. Values must be `"met"`, `"not_met"` or
#'   `"not_assessed"`.
#' @param locations Clause (c): the number of threat-defined locations.
#' @param no_plausible_threats No plausible threats exist, so clause (c) and B3
#'   are *not met*. A finding, distinct from omitting `locations`.
#' @param locations_insufficient_information Threats exist but their extent
#'   cannot be assessed, yielding Data Deficient.
#' @param eoo_bounds Optional length-2 numeric of plausible EOO bounds.
#' @param aoo_bounds Optional length-2 numeric of plausible AOO bounds.
#'
#' @return A list with the overall category, each sub-criterion, any caveats,
#'   and the provenance needed to reproduce the result.
#'
#' @examples
#' criterion_b(eoo_km2 = 15000)$overall
#' criterion_b(eoo_km2 = 15000, clauses = c(a = "met"))$overall
#'
#' # Clause (c) is category dependent: an EOO of 1500 km2 is in the CR band, but
#' # CR requires exactly one threat-defined location, so three qualifies at EN.
#' criterion_b(
#'   eoo_km2 = 1500,
#'   clauses = c(a = "not_met", b = "not_met"),
#'   locations = 3
#' )$criteria[[1]]$category
#'
#' @export
criterion_b <- function(eoo_km2 = NULL,
                        aoo_cells = NULL,
                        clauses = character(0),
                        locations = NULL,
                        no_plausible_threats = FALSE,
                        locations_insufficient_information = FALSE,
                        eoo_bounds = NULL,
                        aoo_bounds = NULL) {
  as_scalar <- function(x) if (is.null(x) || length(x) == 0) NULL else as.numeric(x[[1]])
  bound <- function(b, i) if (is.null(b)) NULL else as.numeric(b[[i]])

  if (length(clauses) > 0 && is.null(names(clauses))) {
    stop("`clauses` must be a named vector, e.g. c(a = \"met\")", call. = FALSE)
  }

  json <- rle_criterion_b_json(
    eoo_km2 = as_scalar(eoo_km2),
    aoo_cells = as_scalar(aoo_cells),
    clause_names = as.character(names(clauses)),
    clause_statuses = as.character(unname(clauses)),
    locations = as_scalar(locations),
    no_plausible_threats = isTRUE(no_plausible_threats),
    locations_insufficient_information = isTRUE(locations_insufficient_information),
    eoo_lower_km2 = bound(eoo_bounds, 1),
    eoo_upper_km2 = bound(eoo_bounds, 2),
    aoo_lower_cells = bound(aoo_bounds, 1),
    aoo_upper_cells = bound(aoo_bounds, 2)
  )

  jsonlite::fromJSON(json, simplifyDataFrame = FALSE)
}

#' The IUCN threshold table as TOML
#'
#' Returns the auditable source of every numeric breakpoint the engine applies,
#' so it can be diffed against the published Guidelines.
#'
#' @return A single string of TOML.
#' @export
thresholds_toml <- function() {
  rle_thresholds_toml()
}

#' SHA-256 of the IUCN threshold table
#'
#' @return A 64-character hex digest.
#' @export
thresholds_sha256 <- function() {
  rle_thresholds_sha256()
}

#' Version of the underlying calculation engine
#'
#' @return A version string.
#' @export
engine_version <- function() {
  rle_version()
}
