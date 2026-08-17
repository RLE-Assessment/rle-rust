//! Compute EOO and AOO for every ecosystem in a remote `GeoParquet` file.
//!
//! ```text
//! cargo run --release -p iucn-rle-engine --example assess -- <url> <ecosystem-column> [limit]
//! ```
//!
//! Prints the read report — row groups touched, bytes fetched, peak resident memory —
//! alongside the metrics, because on a national dataset *how* it was read is as much
//! the point as what came out.

use std::env;

use iucn_rle_core::distribution::DistributionAccumulator;
use iucn_rle_engine::accumulate_geoparquet;
use iucn_rle_format::geoparquet::Query;
use iucn_rle_io::HttpSource;

fn main() {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let [url, column, rest @ ..] = arguments.as_slice() else {
        eprintln!("usage: assess <url> <ecosystem-column> [limit]");
        std::process::exit(2);
    };
    let limit: usize = rest.first().and_then(|n| n.parse().ok()).unwrap_or(15);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread runtime");

    runtime.block_on(async {
        let source = HttpSource::new(url).expect("a usable URL");
        let mut accumulator = DistributionAccumulator::new();

        let started = std::time::Instant::now();
        let report = accumulate_geoparquet(&source, &Query::default(), column, &mut accumulator)
            .await
            .unwrap_or_else(|error| {
                eprintln!("could not read {url}: {error}");
                std::process::exit(1);
            });
        let elapsed = started.elapsed();

        println!("read report");
        println!("  row groups read     {}", report.row_groups_read);
        println!("  row groups skipped  {}", report.row_groups_skipped);
        println!("  features            {}", report.features);
        println!(
            "  bytes fetched       {} ({:.1} MB)",
            report.bytes_fetched,
            report.bytes_fetched as f64 / 1e6
        );
        println!(
            "  crs                 {}",
            report.crs.as_deref().unwrap_or("none stated")
        );
        println!("  elapsed             {:.1}s", elapsed.as_secs_f64());
        println!("  peak memory         {:.1} MB", peak_resident_mb());

        let distribution = accumulator.finish();
        let ecosystems = distribution.ecosystems();
        println!("\n{} ecosystems", ecosystems.len());
        println!("     EOO km2       AOO     cells   ecosystem");
        for ecosystem in ecosystems.iter().take(limit) {
            let aoo = distribution.aoo(ecosystem);
            println!(
                "{:>12.1}  {:>8}  {:>8}   {}",
                distribution.eoo_km2(ecosystem),
                aoo.aoo_cells,
                aoo.occupied_cell_count,
                truncate(ecosystem, 70)
            );
        }
        if ecosystems.len() > limit {
            println!("... and {} more", ecosystems.len() - limit);
        }
    });
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    text.chars().take(width - 1).collect::<String>() + "…"
}

/// Peak resident set size in megabytes.
///
/// The number the memory claim rests on, so it is measured rather than reasoned about.
#[cfg(target_os = "macos")]
fn peak_resident_mb() -> f64 {
    // ru_maxrss is bytes on macOS and kilobytes on Linux — a difference that quietly
    // reports a thousandfold error if assumed either way.
    resident_bytes() / 1e6
}

#[cfg(target_os = "linux")]
fn peak_resident_mb() -> f64 {
    resident_bytes() * 1024.0 / 1e6
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn peak_resident_mb() -> f64 {
    f64::NAN
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn resident_bytes() -> f64 {
    // Read from /proc or getrusage without unsafe: the CLI's own process status is the
    // simplest portable-enough source, and `unsafe_code` is denied workspace-wide.
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status
                    .lines()
                    .find(|line| line.starts_with("VmHWM:"))?
                    .split_whitespace()
                    .nth(1)?
                    .parse::<f64>()
                    .ok()
            })
            .unwrap_or(f64::NAN)
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .and_then(|text| text.trim().parse::<f64>().ok())
            .map_or(f64::NAN, |kilobytes| kilobytes * 1024.0)
    }
}
