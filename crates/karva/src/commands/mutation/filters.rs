//! Validated report and execution filters over durable candidates.

use anyhow::{Context, Result, bail};
use globset::{Glob, GlobSet, GlobSetBuilder};
use karva_cli::MutationFilters;
use karva_mutation::{MutationRecord, Operator, Outcome};

pub(super) struct Filters {
    paths: GlobSet,
    symbols: GlobSet,
    operators: Vec<Operator>,
    outcomes: Vec<Outcome>,
}

pub(super) fn globs(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            Glob::new(pattern)
                .with_context(|| format!("invalid mutation path/symbol glob `{pattern}`"))?,
        );
    }
    Ok(builder.build()?)
}

impl Filters {
    pub(super) fn new(args: &MutationFilters) -> Result<Self> {
        let outcomes = args.outcome.iter().map(|value| {
            serde_json::from_value(serde_json::Value::String(value.clone())).map_err(|_| anyhow::anyhow!("unknown mutation outcome `{value}`; allowed: pending, killed, survived, uncovered, timed-out, invalid, errored"))
        }).collect::<Result<Vec<Outcome>>>()?;
        Ok(Self {
            paths: globs(&args.path)?,
            symbols: globs(&args.symbol)?,
            operators: args
                .operator
                .iter()
                .map(|value| Operator::parse(value))
                .collect::<Result<_>>()?,
            outcomes,
        })
    }

    pub(super) fn matches(&self, record: &MutationRecord) -> bool {
        (self.paths.is_empty() || self.paths.is_match(&record.mutant.path))
            && (self.symbols.is_empty() || self.symbols.is_match(&record.mutant.symbol))
            && (self.operators.is_empty() || self.operators.contains(&record.mutant.operator))
            && (self.outcomes.is_empty() || self.outcomes.contains(&record.outcome))
    }
}

pub(super) fn resolve_id<'a>(
    records: impl Iterator<Item = &'a MutationRecord>,
    prefix: &str,
) -> Result<String> {
    if prefix.is_empty() {
        bail!("mutant ID must not be empty; supply a full ID or unique prefix");
    }
    let mut matches = records.filter(|record| record.mutant.id.starts_with(prefix));
    let record = matches.next().with_context(|| {
        format!("unknown mutant `{prefix}`; inspect `uv run karva mutate results`")
    })?;
    if matches.next().is_some() {
        bail!("ambiguous mutant ID `{prefix}`; supply a longer prefix or the full ID");
    }
    Ok(record.mutant.id.clone())
}

/// Anchors regex selection and escapes the filter delimiter independently of regex syntax.
pub(super) fn test_filter(test: &str) -> String {
    let escaped = regex::escape(test).replace('/', "\\/");
    format!("test(/\\A{escaped}\\z/)")
}

#[cfg(test)]
mod tests {
    use karva_metadata::filter::{EvalContext, FiltersetSet};
    use rstest::rstest;

    #[rstest]
    fn test_names_round_trip(
        #[values(
            "module::test(path\\value)",
            "module::test(quote\")",
            "module::test(line\nvalue)",
            "ends\\",
            "slash/"
        )]
        name: &str,
    ) {
        let filters = FiltersetSet::new(&[super::test_filter(name)]).expect("valid filter");
        assert!(filters.matches(&EvalContext {
            test_name: name,
            tags: &[]
        }));
        assert!(!filters.matches(&EvalContext {
            test_name: &format!("{name}extra"),
            tags: &[]
        }));
    }
}
