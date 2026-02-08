use std::fmt::Write;

/// Metadata stored in the YAML frontmatter of a snapshot file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotMetadata {
    pub source: Option<String>,
    pub expression: Option<String>,
}

/// A parsed snapshot file containing metadata and content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotFile {
    pub metadata: SnapshotMetadata,
    pub content: String,
}

impl SnapshotFile {
    /// Parse a snapshot file from its string representation.
    ///
    /// Expected format:
    /// ```text
    /// ---
    /// source: path/to/test.py::test_name
    /// expression: "str(value)"
    /// ---
    /// snapshot content here
    /// ```
    pub fn parse(input: &str) -> Option<Self> {
        let input = input.strip_prefix("---\n")?;
        let (frontmatter, content) = input.split_once("\n---\n")?;

        let mut metadata = SnapshotMetadata::default();

        for line in frontmatter.lines() {
            if let Some(value) = line.strip_prefix("source: ") {
                metadata.source = Some(value.to_string());
            } else if let Some(value) = line.strip_prefix("expression: ") {
                metadata.expression = Some(value.trim_matches('"').to_string());
            }
        }

        Some(Self {
            metadata,
            content: content.to_string(),
        })
    }

    /// Serialize the snapshot file to its string representation.
    pub fn serialize(&self) -> String {
        let mut output = String::new();
        output.push_str("---\n");

        if let Some(source) = &self.metadata.source {
            let _ = writeln!(output, "source: {source}");
        }
        if let Some(expression) = &self.metadata.expression {
            let _ = writeln!(output, "expression: \"{expression}\"");
        }

        output.push_str("---\n");
        output.push_str(&self.content);

        if !self.content.ends_with('\n') {
            output.push('\n');
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_snapshot_file() {
        let input = "---\nsource: tests/test_example.py::test_example\nexpression: \"str(result)\"\n---\n{'key': 'value'}\n";
        let snapshot = SnapshotFile::parse(input).expect("should parse");
        assert_eq!(
            snapshot.metadata.source.as_deref(),
            Some("tests/test_example.py::test_example")
        );
        assert_eq!(snapshot.metadata.expression.as_deref(), Some("str(result)"));
        assert_eq!(snapshot.content, "{'key': 'value'}\n");
    }

    #[test]
    fn test_serialize_snapshot_file() {
        let snapshot = SnapshotFile {
            metadata: SnapshotMetadata {
                source: Some("tests/test_example.py::test_example".to_string()),
                expression: Some("str(result)".to_string()),
            },
            content: "{'key': 'value'}\n".to_string(),
        };

        let serialized = snapshot.serialize();
        let reparsed = SnapshotFile::parse(&serialized).expect("should reparse");
        assert_eq!(snapshot, reparsed);
    }

    #[test]
    fn test_roundtrip_no_trailing_newline() {
        let snapshot = SnapshotFile {
            metadata: SnapshotMetadata {
                source: Some("test.py::test_foo".to_string()),
                expression: None,
            },
            content: "hello".to_string(),
        };

        let serialized = snapshot.serialize();
        assert!(serialized.ends_with('\n'));
    }

    #[test]
    fn test_parse_no_metadata() {
        let input = "---\n\n---\nsome content\n";
        let snapshot = SnapshotFile::parse(input).expect("should parse");
        assert!(snapshot.metadata.source.is_none());
        assert!(snapshot.metadata.expression.is_none());
        assert_eq!(snapshot.content, "some content\n");
    }
}
