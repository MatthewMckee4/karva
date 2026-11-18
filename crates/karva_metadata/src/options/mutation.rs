//! Profile-layered mutation options; validation occurs before discovery or execution.

use karva_macros::{Combine, OptionsMetadata};
use serde::{Deserialize, Serialize};

use crate::TestTimeoutSecs;

/// Source, filter, and execution policy for Python mutation runs.
#[derive(
    Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, OptionsMetadata, Combine,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MutationOptions {
    /// Source files or directories relative to project root. Defaults to `src`
    /// when present, otherwise the project root with test files excluded.
    #[option(
        default = "[]",
        value_type = "list[str]",
        example = r#"sources = ["src"]"#
    )]
    pub sources: Option<Vec<String>>,

    /// Project-relative path globs excluded from mutation generation.
    #[option(
        default = "[]",
        value_type = "list[str]",
        example = r#"exclude = ["**/generated/**"]"#
    )]
    pub exclude: Option<Vec<String>>,

    /// Mutation families to generate. Empty means all supported operators.
    #[option(
        default = "[]",
        value_type = "list[str]",
        example = r#"operators = ["comparison", "return"]"#
    )]
    pub operators: Option<Vec<String>>,

    /// Qualified symbol globs excluded from mutation generation.
    #[option(
        default = "[]",
        value_type = "list[str]",
        example = r#"exclude-symbols = ["*.generated"]"#
    )]
    pub exclude_symbols: Option<Vec<String>>,

    /// Regular expressions excluding matching source lines.
    #[option(
        default = "[]",
        value_type = "list[str]",
        example = r##"exclude-lines = ["# generated"]"##
    )]
    pub exclude_lines: Option<Vec<String>>,

    /// Maximum simultaneous mutant workers. Must be positive; defaults to host parallelism.
    #[option(
        default = "null",
        value_type = "positive integer",
        example = "workers = 4"
    )]
    pub workers: Option<usize>,

    /// Per-mutant wall-time budget in seconds. Must be positive and finite.
    /// Unset budgets are sized from the measured full-suite baseline.
    #[option(
        default = "null",
        value_type = "float (seconds)",
        example = "timeout = 30.0"
    )]
    pub timeout: Option<TestTimeoutSecs>,
}
