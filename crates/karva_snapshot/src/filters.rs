use std::borrow::Cow;

use regex::{NoExpand, Regex};

/// A compiled snapshot filter that replaces regex matches with a fixed string.
#[derive(Debug)]
pub struct SnapshotFilter {
    regex: Regex,
    replacement: String,
}

impl SnapshotFilter {
    /// Compile a new filter from a regex pattern and replacement string.
    pub fn new(pattern: &str, replacement: String) -> Result<Self, regex::Error> {
        let regex = Regex::new(pattern)?;
        Ok(Self { regex, replacement })
    }
}

/// Apply all filters sequentially, copying text only for replacements or the final result.
pub fn apply_filters<'a>(
    input: &str,
    filters: impl IntoIterator<Item = &'a SnapshotFilter>,
) -> String {
    let mut result = Cow::Borrowed(input);
    for filter in filters {
        if let Cow::Owned(filtered) = filter
            .regex
            .replace_all(&result, NoExpand(&filter.replacement))
        {
            result = Cow::Owned(filtered);
        }
    }
    result.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_filter_replaces_match() {
        let filters = vec![
            SnapshotFilter::new(r"\d{4}-\d{2}-\d{2}", "[date]".to_string()).expect("valid regex"),
        ];
        insta::assert_snapshot!(apply_filters("created on 2024-01-15", &filters), @"created on [date]");
    }

    #[test]
    fn multiple_filters_applied_sequentially() {
        let filters = vec![
            SnapshotFilter::new(r"\d{4}-\d{2}-\d{2}", "[date]".to_string()).expect("valid regex"),
            SnapshotFilter::new(r"[0-9a-f-]{36}", "[uuid]".to_string()).expect("valid regex"),
        ];
        insta::assert_snapshot!(
            apply_filters("id=550e8400-e29b-41d4-a716-446655440000 date=2024-01-15", &filters),
            @"id=[uuid] date=[date]"
        );
    }

    #[test]
    fn later_filters_match_previous_replacements_after_a_nonmatching_filter() {
        let filters = vec![
            SnapshotFilter::new("token", "cost=42".to_string()).expect("valid regex"),
            SnapshotFilter::new("missing", "unused".to_string()).expect("valid regex"),
            SnapshotFilter::new(r"cost=\d+", "cost=$1".to_string()).expect("valid regex"),
        ];

        assert_eq!(
            apply_filters("before token after", &filters),
            "before cost=$1 after"
        );
    }

    #[test]
    fn no_match_leaves_input_unchanged() {
        let filters =
            vec![SnapshotFilter::new(r"ZZZZZ", "[never]".to_string()).expect("valid regex")];
        insta::assert_snapshot!(apply_filters("hello world", &filters), @"hello world");
    }

    #[test]
    fn replacement_text_is_literal() {
        let filters =
            vec![SnapshotFilter::new(r"cost=\d+", "cost=$1".to_string()).expect("valid regex")];
        insta::assert_snapshot!(apply_filters("cost=42", &filters), @"cost=$1");
    }

    #[test]
    fn empty_filters_returns_input() {
        insta::assert_snapshot!(apply_filters("hello world", &[]), @"hello world");
    }

    #[test]
    fn invalid_regex_returns_error() {
        let err = SnapshotFilter::new(r"(unclosed", "x".to_string())
            .expect_err("invalid regex should fail");

        insta::assert_snapshot!(err.to_string(), @r"
        regex parse error:
            (unclosed
            ^
        error: unclosed group
        ");
    }
}
