mod compare;
mod matrix;
mod metric;
mod report;
mod runner;

use std::path::PathBuf;

use anyhow::Result;
use camino::Utf8PathBuf;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(about = "Run Karva benchmark comparisons")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, clap::Subcommand)]
enum Commands {
    Compare(compare::CompareArgs),
    ListProjects,
    MergeReports(report::MergeReportsArgs),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Compare(args) => compare::compare(args),
        Commands::ListProjects => matrix::list_projects(),
        Commands::MergeReports(args) => report::merge_reports(args),
    }
}

fn utf8_path(path: PathBuf) -> Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("Path is not valid UTF-8: {}", path.display()))
}
