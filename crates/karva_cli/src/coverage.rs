use camino::Utf8PathBuf;
use clap::{Args, Parser};
use karva_metadata::{CovFailUnder, CoverageOptions, CoveragePrecision, Options};
use karva_static::EnvVars;

use crate::test::parse_cov_fail_under;

#[derive(Debug, Parser)]
/// Read and report native Karva coverage data.
pub struct CoverageCommand {
    /// Coverage operation to execute.
    #[command(subcommand)]
    pub action: CoverageAction,

    /// Native coverage artifact path, relative to the project root.
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help_heading = "Coverage options"
    )]
    pub data_file: Option<Utf8PathBuf>,

    /// Include only report paths matching this glob.
    #[arg(long, global = true, value_name = "GLOB", action = clap::ArgAction::Append, help_heading = "Coverage options")]
    pub include: Vec<String>,

    /// Exclude report paths matching this glob after inclusion.
    #[arg(long, global = true, value_name = "GLOB", action = clap::ArgAction::Append, help_heading = "Coverage options")]
    pub omit: Vec<String>,

    /// Include execution attributed to a matching context regular expression.
    #[arg(long = "contexts", global = true, value_name = "REGEX", action = clap::ArgAction::Append, help_heading = "Coverage options")]
    pub contexts: Vec<String>,

    /// Decimal places shown in coverage percentages.
    #[arg(
        long,
        global = true,
        value_name = "N",
        help_heading = "Coverage options"
    )]
    pub precision: Option<CoveragePrecision>,

    /// Fail when total coverage is below this percentage.
    #[arg(
        long,
        global = true,
        value_name = "PERCENT",
        value_parser = parse_cov_fail_under,
        help_heading = "Coverage options"
    )]
    pub fail_under: Option<f64>,

    /// The path to a `karva.toml` file to use for configuration.
    #[arg(
        long,
        global = true,
        env = EnvVars::KARVA_CONFIG_FILE,
        value_name = "PATH",
        help_heading = "Config options"
    )]
    pub config_file: Option<Utf8PathBuf>,

    /// Configuration profile to resolve.
    #[arg(
        short = 'P',
        long,
        global = true,
        env = EnvVars::KARVA_PROFILE,
        value_name = "NAME",
        help_heading = "Config options"
    )]
    pub profile: Option<String>,
}

impl CoverageCommand {
    /// Converts command-line values into highest-precedence coverage options.
    pub fn options(&self) -> Options {
        Options {
            coverage: Some(CoverageOptions {
                data_file: self.data_file.as_ref().map(ToString::to_string),
                path_aliases: None,
                include: (!self.include.is_empty()).then(|| self.include.clone()),
                omit: (!self.omit.is_empty()).then(|| self.omit.clone()),
                context: None,
                contexts: (!self.contexts.is_empty()).then(|| self.contexts.clone()),
                precision: self.precision,
                append: None,
                fail_under: self.fail_under.map(CovFailUnder),
                ..CoverageOptions::default()
            }),
            ..Options::default()
        }
    }
}

#[derive(Debug, clap::Subcommand)]
/// Native coverage operations.
pub enum CoverageAction {
    /// Print the compact terminal coverage report.
    Report(CoverageReportCommand),

    /// Generate a navigable annotated HTML coverage report.
    Html(CoverageHtmlCommand),

    /// Generate a Cobertura-compatible XML coverage report.
    Xml(CoverageXmlCommand),

    /// Export documented JSON coverage data.
    Json(CoverageJsonCommand),

    /// Generate an LCOV tracefile.
    Lcov(CoverageLcovCommand),

    /// Combine native coverage artifacts.
    Combine(CoverageCombineCommand),

    /// Delete native combined and shard coverage data.
    Erase,
}

#[derive(Debug, Args)]
/// Options specific to combining native coverage artifacts.
pub struct CoverageCombineCommand {
    /// Native coverage files or directories containing them.
    #[arg(value_name = "PATH")]
    pub inputs: Vec<Utf8PathBuf>,

    /// Include an existing combined artifact in the result.
    #[arg(long)]
    pub append: bool,

    /// Keep input artifacts after a successful combination.
    #[arg(long)]
    pub keep: bool,
}

#[derive(Debug, Args)]
/// Options specific to the LCOV tracefile.
pub struct CoverageLcovCommand {
    /// Path receiving the LCOV tracefile.
    #[arg(long, value_name = "PATH", default_value = "coverage.lcov")]
    pub output: Utf8PathBuf,
}

#[derive(Debug, Args)]
/// Options specific to exported JSON coverage data.
pub struct CoverageJsonCommand {
    /// Path receiving the JSON report.
    #[arg(long, value_name = "PATH", default_value = "coverage.json")]
    pub output: Utf8PathBuf,

    /// Format output with indentation and line breaks.
    #[arg(long)]
    pub pretty_print: bool,

    /// Include per-line execution contexts.
    #[arg(long)]
    pub show_contexts: bool,
}

#[derive(Debug, Args)]
/// Options specific to the Cobertura XML coverage report.
pub struct CoverageXmlCommand {
    /// Path receiving the XML report.
    #[arg(long, value_name = "PATH", default_value = "coverage.xml")]
    pub output: Utf8PathBuf,
}

#[derive(Debug, Args)]
/// Options specific to the annotated HTML coverage report.
pub struct CoverageHtmlCommand {
    /// Directory receiving the report files.
    #[arg(long, value_name = "PATH", default_value = "htmlcov")]
    pub directory: Utf8PathBuf,

    /// Report title shown in the browser.
    #[arg(long, default_value = "Coverage report")]
    pub title: String,

    /// Show execution contexts beside annotated source lines.
    #[arg(long)]
    pub show_contexts: bool,

    /// Omit fully covered source pages from the index.
    #[arg(long)]
    pub skip_covered: bool,

    /// Omit sources with no statements or branches from the index.
    #[arg(long)]
    pub skip_empty: bool,
}

#[derive(Debug, Args)]
/// Options specific to the terminal coverage report.
pub struct CoverageReportCommand {
    /// File paths, directories, or dotted module names to include.
    #[arg(value_name = "SELECTOR")]
    pub selectors: Vec<String>,

    /// Show missing line ranges and branch arcs.
    #[arg(long)]
    pub show_missing: bool,

    /// Hide files with complete coverage without changing totals.
    #[arg(long)]
    pub skip_covered: bool,

    /// Hide files with no statements or branches without changing totals.
    #[arg(long)]
    pub skip_empty: bool,

    /// Column used to order displayed files.
    #[arg(long, value_enum, default_value_t)]
    pub sort: CoverageSort,

    /// Report representation.
    #[arg(long, value_enum, default_value_t)]
    pub format: CoverageFormat,

    /// Write the report to this path instead of stdout.
    #[arg(long, value_name = "PATH")]
    pub output: Option<Utf8PathBuf>,

    /// Append to the output file instead of replacing it.
    #[arg(long, requires = "output", default_missing_value = "true", require_equals = true, num_args = 0..=1)]
    pub append: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, clap::ValueEnum)]
/// Terminal coverage report representation.
pub enum CoverageFormat {
    /// Human-readable aligned table.
    #[default]
    Text,
    /// GitHub-flavored Markdown table.
    Markdown,
    /// Numeric total percentage only.
    Total,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, clap::ValueEnum)]
/// Column used to order displayed coverage files.
pub enum CoverageSort {
    /// Source path.
    #[default]
    Name,
    /// Statement count.
    Statements,
    /// Missing statement count.
    Misses,
    /// Branch count.
    Branches,
    /// Partial branch count.
    PartialBranches,
    /// Coverage percentage.
    Coverage,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_options_parse_after_report() {
        let command = CoverageCommand::try_parse_from([
            "coverage",
            "report",
            "--data-file",
            "build/data.json",
            "--include",
            "src/**",
            "--contexts",
            "test_checkout",
            "--precision",
            "2",
            "--fail-under",
            "90.5",
        ])
        .expect("parse coverage command");
        let coverage = command.options().coverage.expect("coverage options");

        assert_eq!(coverage.data_file.as_deref(), Some("build/data.json"));
        assert_eq!(coverage.include, Some(vec!["src/**".to_owned()]));
        assert_eq!(coverage.contexts, Some(vec!["test_checkout".to_owned()]));
        assert_eq!(coverage.precision, Some(CoveragePrecision(2)));
        assert_eq!(coverage.fail_under, Some(CovFailUnder(90.5)));
    }

    #[test]
    fn precision_rejects_digits_f64_cannot_represent() {
        let requested = CoveragePrecision::MAX + 1;
        let error = CoverageCommand::try_parse_from([
            "coverage",
            "report",
            "--precision",
            &requested.to_string(),
        ])
        .expect_err("reject excessive precision");
        let message = error.to_string();

        assert!(message.contains(&requested.to_string()));
        assert!(message.contains(&CoveragePrecision::MAX.to_string()));
    }
}
