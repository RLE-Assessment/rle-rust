# The R runner for the cross-language conformance corpus.
#
# Runs the identical JSON file as the Rust, Python, C ABI and JavaScript suites. If
# all five pass, the bindings provably agree.

corpus_path <- function() {
  # Works both from the package source tree and from an installed package.
  candidates <- c(
    file.path("..", "..", "..", "..", "..", "fixtures", "cases", "criterion_b.json"),
    testthat::test_path("..", "..", "..", "..", "..", "fixtures", "cases", "criterion_b.json")
  )
  for (path in candidates) {
    if (file.exists(path)) {
      return(normalizePath(path))
    }
  }
  skip("conformance corpus not found; run from the repository checkout")
}

cases <- jsonlite::fromJSON(corpus_path(), simplifyDataFrame = FALSE)$cases

test_that("corpus is not empty", {
  # Guards against a renamed fixture making the suite vacuously pass.
  expect_gte(length(cases), 28)
})

# `$` on a list does PARTIAL matching, so `case$locations` silently resolves to
# `locations_insufficient_information` when no exact `locations` key exists — which
# turns TRUE into a location count of 1 and corrupts the case. `[[` matches exactly.
# Every field access below must use `[[`.
field <- function(case, name) case[[name]]

for (case in cases) {
  local({
    this <- case
    test_that(field(this, "id"), {
      clause_list <- field(this, "clauses")
      clauses <- character(0)
      if (length(clause_list) > 0) {
        clauses <- vapply(clause_list, function(x) x[["status"]], character(1))
        names(clauses) <- vapply(clause_list, function(x) x[["sub"]], character(1))
      }

      eoo_bounds <- NULL
      if (!is.null(field(this, "eoo_lower_km2"))) {
        eoo_bounds <- c(field(this, "eoo_lower_km2"), field(this, "eoo_upper_km2"))
      }

      result <- criterion_b(
        eoo_km2 = field(this, "eoo_km2"),
        aoo_cells = field(this, "aoo_cells"),
        clauses = clauses,
        locations = field(this, "locations"),
        no_plausible_threats = isTRUE(field(this, "no_plausible_threats")),
        locations_insufficient_information =
          isTRUE(field(this, "locations_insufficient_information")),
        eoo_bounds = eoo_bounds
      )

      criteria <- result[["criteria"]]
      by_criterion <- vapply(criteria, function(c) c[["category"]], character(1))
      names(by_criterion) <- vapply(criteria, function(c) c[["criterion"]], character(1))

      expected <- field(this, "expect")
      for (key in names(expected)) {
        actual <- if (key == "overall") {
          result[["overall"]]
        } else {
          unname(by_criterion[[toupper(key)]])
        }
        expect_equal(actual, expected[[key]], info = key)
      }

      expected_thresholds <- field(this, "expect_threshold")
      if (!is.null(expected_thresholds)) {
        thresholds <- vapply(
          criteria,
          function(c) {
            v <- c[["threshold_category"]]
            if (is.null(v)) NA_character_ else v
          },
          character(1)
        )
        names(thresholds) <- names(by_criterion)
        for (key in names(expected_thresholds)) {
          expect_equal(
            unname(thresholds[[toupper(key)]]),
            expected_thresholds[[key]],
            info = paste(key, "threshold")
          )
        }
      }
    })
  })
}

test_that("thresholds shipped with the package match the engine", {
  expect_match(thresholds_toml(), 'guidelines_version = "2.0"', fixed = TRUE)
  expect_match(thresholds_toml(), 'criteria_version = "2.1"', fixed = TRUE)
  expect_equal(nchar(thresholds_sha256()), 64L)
  expect_equal(criterion_b(eoo_km2 = 15000)$thresholds_sha256, thresholds_sha256())
})

test_that("clause (c) as a status is rejected", {
  # (c) is a count, not a status. Accepting a boolean here is the bug M1.5 fixed.
  expect_error(
    criterion_b(eoo_km2 = 15000, clauses = c(c = "met")),
    "locations"
  )
})

test_that("an unknown clause raises a useful error", {
  expect_error(
    criterion_b(eoo_km2 = 15000, clauses = c(z = "met")),
    "a\\|b"
  )
})
