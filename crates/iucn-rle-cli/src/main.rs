//! Command-line surface for IUCN Red List of Ecosystems calculations.
//!
//! M0 skeleton: `version` only. Assessment subcommands land in M1 (`criterion-b`)
//! and M3 (`aoo`, `eoo`, `optimize`).

use clap::{Parser, Subcommand};

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

#[derive(Subcommand)]
enum Command {
    /// Print the version of the underlying calculation engine.
    Version,
}

fn main() {
    match Cli::parse().command {
        Command::Version => println!("{}", iucn_rle_core::version()),
    }
}
