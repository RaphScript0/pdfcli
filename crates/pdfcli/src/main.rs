use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "pdfcli", version, about = "CLI wrapper around PDF utilities")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Validate that the input file exists (bootstrap command).
    Validate {
        /// Input PDF path
        input: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Validate { input } => {
            pdfcore::validate_input_file(&input)
                .with_context(|| format!("validating input: {}", input.display()))?;
            println!("OK: {}", input.display());
        }
    }

    Ok(())
}
