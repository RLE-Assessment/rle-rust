//! Command-line surface for IUCN Red List of Ecosystems calculations.
//!
//! M1: `criterion-b` and `thresholds`. The AOO/EOO subcommands that read remote
//! data arrive in M3.

// clap turns doc comments into --help text, where backticks would be printed
// literally. Rustdoc conventions do not apply to strings a user reads in a
// terminal.
#![allow(clippy::doc_markdown)]

use clap::{Parser, Subcommand, ValueEnum};
use iucn_rle_core::ffi::{
    criterion_b_from_parts, distribution_metrics, MetricInput, PolygonInput, SubconditionInput,
    SubconditionsInput,
};
use iucn_rle_core::Summary;

#[derive(Parser)]
#[command(
    name = "iucn-rle",
    about = "IUCN Red List of Ecosystems assessment calculations",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Format {
    /// Human-readable summary.
    Text,
    /// Machine-readable JSON.
    Json,
}

#[derive(Subcommand)]
enum Command {
    /// Print the version of the underlying calculation engine.
    Version,

    /// Print the IUCN threshold table this build applies, as TOML.
    Thresholds {
        /// Print only the SHA-256 digest of the table.
        #[arg(long)]
        sha256: bool,
    },

    /// Compute EOO and AOO from a GeoJSON distribution map.
    ///
    /// Reads a FeatureCollection of Polygons in longitude/latitude degrees and
    /// reports the Criterion B spatial metrics per ecosystem.
    Metrics {
        /// Path to a GeoJSON file, or - to read standard input.
        #[arg(value_name = "GEOJSON")]
        path: String,

        /// Feature property holding the ecosystem code.
        #[arg(long, default_value = "ECO_CODE")]
        ecosystem_property: String,

        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },

    /// Assess Criterion B (restricted geographic distribution).
    ///
    /// Sub-conditions you do not pass count as NOT ASSESSED, which is different
    /// from not met: the result becomes a range rather than a firm category.
    CriterionB {
        /// Extent of occurrence in km2 (sub-criterion B1).
        #[arg(long)]
        eoo_km2: Option<f64>,

        /// Occupied 10x10 km cells after the 1% exclusion (B2).
        #[arg(long)]
        aoo_cells: Option<f64>,

        /// Plausible bounds on the EOO, as LOWER UPPER.
        #[arg(long, num_args = 2, value_names = ["LOWER", "UPPER"])]
        eoo_bounds: Option<Vec<f64>>,

        /// Plausible bounds on the AOO, as LOWER UPPER.
        #[arg(long, num_args = 2, value_names = ["LOWER", "UPPER"])]
        aoo_bounds: Option<Vec<f64>>,

        /// Clause status, repeatable. CLAUSE is a, b, a.i, a.ii, a.iii or
        /// b3_rapid_collapse; STATUS is met, not_met or not_assessed.
        #[arg(long = "clause", value_name = "CLAUSE=STATUS")]
        clauses: Vec<String>,

        /// Clause (c): the number of threat-defined locations. A count, not a
        /// status, because clause (c) is category dependent: 1 location for CR,
        /// 5 or fewer for EN, 10 or fewer for VU.
        #[arg(long)]
        locations: Option<u32>,

        /// No plausible threats exist, so clause (c) and B3 are not met. A
        /// finding, distinct from simply omitting --locations.
        #[arg(long)]
        no_plausible_threats: bool,

        /// Threats exist but their extent cannot be assessed: Data Deficient.
        #[arg(long)]
        locations_insufficient_information: bool,

        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
}

fn metric(best: Option<f64>, bounds: Option<&Vec<f64>>) -> Option<MetricInput> {
    best.map(|best| match bounds {
        Some(b) if b.len() == 2 => MetricInput::bounded(best, b[0], b[1]),
        _ => MetricInput::point(best),
    })
}

fn parse_clauses(clauses: &[String]) -> Result<Vec<SubconditionInput>, String> {
    clauses
        .iter()
        .map(|entry| {
            let (sub, status) = entry.split_once('=').ok_or_else(|| {
                format!("expected CLAUSE=STATUS, got {entry:?} (for example: --clause a=met)")
            })?;
            Ok(SubconditionInput {
                sub: sub.to_owned(),
                status: status.to_owned(),
            })
        })
        .collect()
}

fn print_text(summary: &Summary) {
    println!("Criterion B: {}", summary.overall);
    println!();
    for criterion in &summary.criteria {
        let threshold = criterion.threshold_category.as_deref().unwrap_or("-");
        println!(
            "  {:<4} {:<14} (thresholds alone: {})",
            criterion.criterion, criterion.category, threshold
        );
    }
    // The same caveat usually applies to every sub-criterion, so the summary
    // carries it once per criterion. Printing each one would just be noise.
    let mut seen: Vec<&str> = Vec::new();
    for note in &summary.notes {
        if !seen.contains(&note.as_str()) {
            seen.push(note);
        }
    }
    if !seen.is_empty() {
        println!();
        for note in seen {
            println!("  note: {note}");
        }
    }
    println!();
    println!(
        "  IUCN RLE Guidelines v{}, thresholds {}",
        summary.guidelines_version,
        &summary.thresholds_sha256[..12]
    );
}

/// Read a GeoJSON FeatureCollection into polygons.
///
/// Deliberately minimal: only what a distribution map needs. Polygon and MultiPolygon
/// features, one ecosystem code per feature.
fn read_geojson(path: &str, ecosystem_property: &str) -> Result<Vec<PolygonInput>, String> {
    let text = if path == "-" {
        std::io::read_to_string(std::io::stdin()).map_err(|e| format!("reading stdin: {e}"))?
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("reading {path}: {e}"))?
    };

    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{path} is not valid JSON: {e}"))?;

    let features = value
        .get("features")
        .and_then(|f| f.as_array())
        .ok_or_else(|| format!("{path} is not a GeoJSON FeatureCollection"))?;

    let mut polygons = Vec::new();

    for (index, feature) in features.iter().enumerate() {
        let ecosystem = feature
            .get("properties")
            .and_then(|p| p.get(ecosystem_property))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                format!(
                    "feature {index} has no string property {ecosystem_property:?}; \
                     use --ecosystem-property to name the right one"
                )
            })?
            .to_owned();

        let geometry = feature
            .get("geometry")
            .ok_or_else(|| format!("feature {index} has no geometry"))?;
        let kind = geometry.get("type").and_then(|t| t.as_str()).unwrap_or("");

        // A MultiPolygon is a list of polygons; a Polygon is a list of rings. Treating
        // each part of a MultiPolygon as its own feature is correct for both metrics.
        let parts: Vec<&serde_json::Value> = match kind {
            "Polygon" => vec![geometry
                .get("coordinates")
                .unwrap_or(&serde_json::Value::Null)],
            "MultiPolygon" => geometry
                .get("coordinates")
                .and_then(|c| c.as_array())
                .map(|a| a.iter().collect())
                .unwrap_or_default(),
            other => {
                return Err(format!(
                    "feature {index} has geometry type {other:?}; only Polygon and \
                     MultiPolygon describe a distribution"
                ))
            }
        };

        for part in parts {
            let rings: Vec<Vec<[f64; 2]>> = serde_json::from_value(part.clone())
                .map_err(|e| format!("feature {index} has malformed coordinates: {e}"))?;
            polygons.push(PolygonInput {
                ecosystem: ecosystem.clone(),
                rings,
            });
        }
    }

    Ok(polygons)
}

fn run() -> Result<(), String> {
    match Cli::parse().command {
        Command::Version => println!("{}", iucn_rle_core::version()),

        Command::Metrics {
            path,
            ecosystem_property,
            format,
        } => {
            let polygons = read_geojson(&path, &ecosystem_property)?;
            let summary = distribution_metrics(&polygons)?;

            match format {
                Format::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&summary)
                        .map_err(|e| format!("could not serialise metrics: {e}"))?
                ),
                Format::Text => {
                    println!(
                        "{:<12} {:>14} {:>10} {:>10}",
                        "ECOSYSTEM", "EOO (km2)", "AOO", "OCCUPIED"
                    );
                    for e in &summary.ecosystems {
                        println!(
                            "{:<12} {:>14.1} {:>10} {:>10}",
                            e.ecosystem, e.eoo_km2, e.aoo_cells, e.occupied_cell_count
                        );
                    }
                    println!();
                    println!(
                        "  grid {} at {:.0} m",
                        summary.grid_crs, summary.cell_size_m
                    );
                    if summary.overfull_cells > 0 {
                        println!(
                            "  warning: {} cell(s) received more extent than they can \
                             hold, so the source map has overlapping features",
                            summary.overfull_cells
                        );
                    }
                }
            }
        }

        Command::Thresholds { sha256 } => {
            if sha256 {
                println!("{}", iucn_rle_core::thresholds::V2_2024_SHA256);
            } else {
                print!("{}", iucn_rle_core::thresholds::V2_2024_TOML);
            }
        }

        Command::CriterionB {
            eoo_km2,
            aoo_cells,
            eoo_bounds,
            aoo_bounds,
            clauses,
            locations,
            no_plausible_threats,
            locations_insufficient_information,
            format,
        } => {
            let subs = SubconditionsInput {
                clauses: parse_clauses(&clauses)?,
                locations,
                no_plausible_threats,
                locations_insufficient_information,
            };
            let summary = criterion_b_from_parts(
                metric(eoo_km2, eoo_bounds.as_ref()),
                metric(aoo_cells, aoo_bounds.as_ref()),
                subs,
            )?;

            match format {
                Format::Text => print_text(&summary),
                Format::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&summary)
                        .map_err(|e| format!("could not serialise summary: {e}"))?
                ),
            }
        }
    }
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}
