//! Describe a remote `GeoParquet` file without downloading it.
//!
//! Reads only the footer over HTTP range requests, then prints what an assessment would
//! need to know: how big the file is, how it is divided, which columns it has, and
//! whether its geometry is in a coordinate system the metrics accept.
//!
//! ```text
//! cargo run -p iucn-rle-engine --example inspect -- <url>
//! ```
//!
//! Useful in its own right, and it is how the numbers in the acceptance test were
//! established rather than guessed.

use std::env;

use iucn_rle_format::geoparquet::{
    footer_range, parse_footer, Footer, GeoParquet, Severity, DEFAULT_FOOTER_PREFETCH,
};
use iucn_rle_io::{ByteSource, HttpSource};

fn main() {
    let Some(url) = env::args().nth(1) else {
        eprintln!("usage: inspect <url>");
        std::process::exit(2);
    };

    // reqwest's native transport needs a reactor; the browser's Fetch does not. This is
    // the seam where a caller supplies one, which is why the engine itself never does.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread runtime");

    runtime.block_on(async {
        let source = HttpSource::new(&url).expect("a usable URL");
        let size = source.size().await.expect("the object's size");
        println!("url    {url}");
        println!("size   {size} bytes ({:.2} GB)", size as f64 / 1e9);

        let mut range = footer_range(size, DEFAULT_FOOTER_PREFETCH);
        let mut fetched = 0u64;
        let file = loop {
            let tail = source.read_range(range.clone()).await.expect("the footer");
            fetched += tail.len() as u64;
            match parse_footer(&tail, size).expect("a parquet footer") {
                Footer::Complete(file) => break file,
                Footer::NeedMore(wider) => range = wider,
            }
        };

        println!(
            "footer {fetched} bytes fetched, {:.6}% of the file",
            100.0 * fetched as f64 / size as f64
        );
        println!("rows   {}", file.num_rows());
        println!("groups {}", file.row_groups().len());

        if let Some(largest) = file
            .row_groups()
            .iter()
            .max_by_key(|group| group.compressed_size)
        {
            println!(
                "largest row group: {} rows, {:.1} MB across all columns",
                largest.num_rows,
                largest.compressed_size as f64 / 1e6
            );
        }

        // What actually bounds peak memory is the *projected* size — an assessment
        // reads the geometry and one label column, not all of them. Reporting the whole
        // row group here would overstate the requirement several times over on a wide
        // dataset, which is exactly the sort of number that gets quoted later.
        let geometry = &file.geo().primary_column;
        let widest = (0..file.row_groups().len())
            .filter_map(|index| {
                let ranges = file.ranges_for_row_group(index, geometry).ok()?;
                Some(ranges.iter().map(|r| r.end - r.start).sum::<u64>())
            })
            .max()
            .unwrap_or(0);
        println!(
            "largest row group, geometry column only: {:.1} MB — this is what bounds \
             peak memory",
            widest as f64 / 1e6
        );

        describe(&file);
    });
}

fn describe(file: &GeoParquet) {
    let geo = file.geo();
    println!("geoparquet: {} (as declared by the writer)", geo.version);
    println!("geometry column: {}", geo.primary_column);
    println!("encoding: {}", geo.primary_encoding());
    println!(
        "crs: {} ({})",
        geo.primary_crs_code()
            .unwrap_or_else(|| "none stated".into()),
        if geo.primary_is_geographic() {
            "geographic — usable"
        } else {
            "projected — must be reprojected first"
        }
    );
    // The declared version is reported, and deliberately not trusted. geopandas
    // 1.1.4 writes "1.0.0" while also writing the `covering` key introduced in
    // 1.1.0, so what the file *says* and what it *contains* can disagree — which is
    // worth surfacing here, since a reader that gated on the version would lose all
    // spatial pruning on files from the most widely used writer there is.
    match geo.primary_covering() {
        Some(_) if geo.version.starts_with("1.0") => println!(
            "bbox covering: present — spatial pruning available, even though the \
             file declares {} and covering is a 1.1 feature",
            geo.version
        ),
        Some(_) => println!("bbox covering: present — spatial pruning available"),
        None => println!("bbox covering: absent — every row group must be read"),
    }

    let columns: Vec<_> = file
        .metadata()
        .file_metadata()
        .schema_descr()
        .columns()
        .iter()
        .map(|column| column.path().string())
        .collect();
    println!("columns: {} total", columns.len());
    println!("  {}", columns.join(", "));

    // Two halves of the same question. Schema validation says the metadata is
    // well-formed per the specification; the structural checks say it describes
    // the file it is actually in. A file can pass either and fail the other.
    let mut findings = Vec::new();
    #[cfg(feature = "schema-validation")]
    {
        let schema_findings = file.validate_against_schema();
        println!(
            "\nschema validation: {}",
            if schema_findings.is_empty() {
                format!(
                    "conforms to the published GeoParquet {} schema",
                    file.geo().version
                )
            } else {
                format!("{} finding(s)", schema_findings.len())
            }
        );
        findings.extend(schema_findings);
    }
    #[cfg(not(feature = "schema-validation"))]
    println!("\nschema validation: not built in (enable the `schema-validation` feature)");
    findings.extend(file.check_structure());
    println!("\nstructural checks: {} finding(s)", findings.len());
    for finding in &findings {
        let label = match finding.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        };
        println!("  {label}: {}", finding.message);
    }
    if findings.iter().all(|f| f.severity != Severity::Error) {
        println!("  (nothing that would stop an assessment)");
    }
}
