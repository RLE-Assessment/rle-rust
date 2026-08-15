# The R runner for rle-python's golden null-island dataset.
#
# Runs the identical fixture as the Rust, Python and JavaScript suites, so agreement
# with the implementation assessments currently use is enforced on every surface.

golden_path <- function() {
  root <- Sys.getenv("IUCN_RLE_REPO_ROOT", unset = NA)
  from_test_path <- tryCatch(
    testthat::test_path("..", "..", "..", "..", "..", "fixtures", "cases", "null_island_aoo.json"),
    error = function(e) NULL
  )
  candidates <- c(
    if (!is.na(root)) file.path(root, "fixtures", "cases", "null_island_aoo.json"),
    file.path("..", "..", "..", "..", "..", "fixtures", "cases", "null_island_aoo.json"),
    from_test_path
  )
  for (path in candidates) {
    if (file.exists(path)) {
      return(normalizePath(path))
    }
  }
  # An error, not a skip: skipping would pass this suite vacuously.
  stop(
    "golden fixture not found. Set IUCN_RLE_REPO_ROOT, or run from the repository ",
    "checkout. Tried:\n  ", paste(candidates, collapse = "\n  "),
    call. = FALSE
  )
}

golden <- jsonlite::fromJSON(golden_path(), simplifyVector = FALSE)

golden_metrics <- function() {
  polygons <- lapply(golden[["features"]], function(f) {
    list(ecosystem = f[["code"]], rings = f[["rings"]])
  })
  result <- distribution_metrics(polygons)
  stats <- result[["ecosystems"]]
  names(stats) <- vapply(stats, function(e) e[["ecosystem"]], character(1))
  stats
}

test_that("fixture is present", {
  expect_equal(length(golden[["features"]]), 3L)
})

test_that("occupied cell counts match rle-python", {
  stats <- golden_metrics()
  for (feature in golden[["features"]]) {
    code <- feature[["code"]]
    expected <- sum(vapply(
      golden[["expected_cells"]],
      function(cell) {
        value <- cell[["fractions"]][[code]]
        !is.null(value) && value > 0
      },
      logical(1)
    ))
    expect_equal(stats[[code]][["occupied_cell_count"]], expected, info = code)
  }
})

test_that("published metrics match", {
  # From the committed workshop notebook: "EOO is 73.2 km2", "AOO is 4 cells".
  published <- golden[["published_metrics"]]
  stats <- golden_metrics()

  for (code in names(published[["ecosystems"]])) {
    expected <- published[["ecosystems"]][[code]]
    actual <- stats[[code]]
    expect_lt(
      abs(actual[["eoo_km2"]] - expected[["eoo_km2"]]),
      published[["eoo_tolerance_km2"]]
    )
    expect_equal(actual[["aoo_cells"]], expected[["aoo_cells"]], info = code)
  }
})

test_that("out-of-range coordinates are rejected", {
  # Rejected rather than clamped: swapped or already-projected coordinates would
  # otherwise yield a plausible-looking but wrong AOO.
  expect_error(
    distribution_metrics(list(
      list(ecosystem = "forest", rings = list(list(c(10, 200), c(11, 201))))
    )),
    "latitude"
  )
})
