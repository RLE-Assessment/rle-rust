# The R runner for the cross-language conformance corpus.
#
# Runs the identical JSON file as the Rust, Python, and JavaScript suites. If all
# four pass, the bindings provably agree.

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
  expect_gte(length(cases), 20)
})

for (case in cases) {
  local({
    this <- case
    test_that(this$id, {
      subs <- character(0)
      if (length(this$subconditions) > 0) {
        subs <- vapply(this$subconditions, function(s) s$status, character(1))
        names(subs) <- vapply(this$subconditions, function(s) s$sub, character(1))
      }

      eoo_bounds <- NULL
      if (!is.null(this$eoo_lower_km2)) {
        eoo_bounds <- c(this$eoo_lower_km2, this$eoo_upper_km2)
      }

      result <- criterion_b(
        eoo_km2 = this$eoo_km2,
        aoo_cells = this$aoo_cells,
        subconditions = subs,
        eoo_bounds = eoo_bounds
      )

      by_criterion <- vapply(result$criteria, function(c) c$category, character(1))
      names(by_criterion) <- vapply(result$criteria, function(c) c$criterion, character(1))

      expect_equal(unname(by_criterion[["B1"]]), this$expect$b1)
      expect_equal(unname(by_criterion[["B2"]]), this$expect$b2)
      expect_equal(result$overall, this$expect$overall)
    })
  })
}

test_that("thresholds shipped with the package match the engine", {
  expect_match(thresholds_toml(), 'guidelines_version = "2.0"', fixed = TRUE)
  expect_equal(nchar(thresholds_sha256()), 64L)
  expect_equal(criterion_b(eoo_km2 = 15000)$thresholds_sha256, thresholds_sha256())
})

test_that("an unknown sub-condition raises a useful error", {
  expect_error(
    criterion_b(eoo_km2 = 15000, subconditions = c(z = "met")),
    "a\\|b\\|c"
  )
})
