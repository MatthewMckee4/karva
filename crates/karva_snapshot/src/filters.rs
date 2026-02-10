use regex::Regex;

/// A compiled snapshot filter that replaces regex matches with a fixed string.
pub struct SnapshotFilter {
    regex: Regex,
    replacement: String,
}

impl SnapshotFilter {
    /// Compile a new filter from a regex pattern and replacement string.
    ///
    /// Returns `None` if the pattern is not a valid regex.
    pub fn new(pattern: &str, replacement: String) -> Option<Self> {
        Regex::new(pattern)
            .ok()
            .map(|regex| Self { regex, replacement })
    }
}

/// Apply all filters sequentially to the input string.
pub fn apply_filters(input: &str, filters: &[SnapshotFilter]) -> String {
    let mut result = input.to_string();
    for filter in filters {
        result = filter
            .regex
            .replace_all(&result, &*filter.replacement)
            .into_owned();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_filter_replaces_match() {
        let filters = vec![
            SnapshotFilter::new(r"\d{4}-\d{2}-\d{2}", "[date]".to_string()).expect("valid regex"),
        ];
        let result = apply_filters("created on 2024-01-15", &filters);
        assert_eq!(result, "created on [date]");
    }

    #[test]
    fn multiple_filters_applied_sequentially() {
        let filters = vec![
            SnapshotFilter::new(r"\d{4}-\d{2}-\d{2}", "[date]".to_string()).expect("valid regex"),
            SnapshotFilter::new(r"[0-9a-f-]{36}", "[uuid]".to_string()).expect("valid regex"),
        ];
        let result = apply_filters(
            "id=550e8400-e29b-41d4-a716-446655440000 date=2024-01-15",
            &filters,
        );
        assert_eq!(result, "id=[uuid] date=[date]");
    }

    #[test]
    fn no_match_leaves_input_unchanged() {
        let filters =
            vec![SnapshotFilter::new(r"ZZZZZ", "[never]".to_string()).expect("valid regex")];
        let result = apply_filters("hello world", &filters);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn empty_filters_returns_input() {
        let result = apply_filters("hello world", &[]);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn invalid_regex_returns_none() {
        assert!(SnapshotFilter::new(r"(unclosed", "x".to_string()).is_none());
    }
}
