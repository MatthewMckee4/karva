use std::fmt::Write;

/// Metadata stored in the YAML frontmatter of a snapshot file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotMetadata {
    pub source: Option<String>,
    pub inline_source: Option<String>,
    pub inline_line: Option<u32>,
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
            } else if let Some(value) = line.strip_prefix("inline_source: ") {
                metadata.inline_source = Some(value.to_string());
            } else if let Some(value) = line.strip_prefix("inline_line: ") {
                metadata.inline_line = value.parse().ok();
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
        if let Some(inline_source) = &self.metadata.inline_source {
            let _ = writeln!(output, "inline_source: {inline_source}");
        }
        if let Some(inline_line) = self.metadata.inline_line {
            let _ = writeln!(output, "inline_line: {inline_line}");
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
    fn parse_snapshot_file() {
        let input = "---\nsource: tests/test_example.py:5::test_example\n---\n{'key': 'value'}\n";
        let snapshot = SnapshotFile::parse(input).expect("should parse");
        insta::assert_snapshot!(snapshot.metadata.source.as_deref().unwrap(), @"tests/test_example.py:5::test_example");
        insta::assert_snapshot!(snapshot.content, @r"
        {'key': 'value'}
        ");
    }

    #[test]
    fn serialize_snapshot_file() {
        let snapshot = SnapshotFile {
            metadata: SnapshotMetadata {
                source: Some("tests/test_example.py:5::test_example".to_string()),
                ..Default::default()
            },
            content: "{'key': 'value'}\n".to_string(),
        };
        insta::assert_snapshot!(snapshot.serialize(), @r"
        ---
        source: tests/test_example.py:5::test_example
        ---
        {'key': 'value'}
        ");
    }

    #[test]
    fn serialize_roundtrip() {
        let snapshot = SnapshotFile {
            metadata: SnapshotMetadata {
                source: Some("tests/test_example.py:5::test_example".to_string()),
                ..Default::default()
            },
            content: "{'key': 'value'}\n".to_string(),
        };
        let serialized = snapshot.serialize();
        let reparsed = SnapshotFile::parse(&serialized).expect("should reparse");
        assert_eq!(snapshot, reparsed);
    }

    #[test]
    fn serialize_no_trailing_newline() {
        let snapshot = SnapshotFile {
            metadata: SnapshotMetadata {
                source: Some("test.py:3::test_foo".to_string()),
                ..Default::default()
            },
            content: "hello".to_string(),
        };
        insta::assert_snapshot!(snapshot.serialize(), @r"
        ---
        source: test.py:3::test_foo
        ---
        hello
        ");
    }

    #[test]
    fn serialize_inline_metadata() {
        let snapshot = SnapshotFile {
            metadata: SnapshotMetadata {
                source: Some("test.py:5::test_hello".to_string()),
                inline_source: Some("/abs/path/to/test.py".to_string()),
                inline_line: Some(5),
            },
            content: "hello world\n".to_string(),
        };
        insta::assert_snapshot!(snapshot.serialize(), @r"
        ---
        source: test.py:5::test_hello
        inline_source: /abs/path/to/test.py
        inline_line: 5
        ---
        hello world
        ");
    }

    #[test]
    fn roundtrip_inline_metadata() {
        let snapshot = SnapshotFile {
            metadata: SnapshotMetadata {
                source: Some("test.py:5::test_hello".to_string()),
                inline_source: Some("/abs/path/to/test.py".to_string()),
                inline_line: Some(5),
            },
            content: "hello world\n".to_string(),
        };
        let serialized = snapshot.serialize();
        let reparsed = SnapshotFile::parse(&serialized).expect("should reparse");
        assert_eq!(snapshot, reparsed);
    }

    #[test]
    fn serialize_content_with_dashes() {
        let snapshot = SnapshotFile {
            metadata: SnapshotMetadata {
                source: Some("test.py:5::test_dashes".to_string()),
                ..Default::default()
            },
            content: "---\nthis looks like yaml\n---\n".to_string(),
        };
        insta::assert_snapshot!(snapshot.serialize(), @r"
        ---
        source: test.py:5::test_dashes
        ---
        ---
        this looks like yaml
        ---
        ");
    }

    #[test]
    fn roundtrip_content_with_dashes() {
        let snapshot = SnapshotFile {
            metadata: SnapshotMetadata {
                source: Some("test.py:5::test_dashes".to_string()),
                ..Default::default()
            },
            content: "---\nthis looks like yaml\n---\n".to_string(),
        };
        let serialized = snapshot.serialize();
        let reparsed = SnapshotFile::parse(&serialized).expect("should reparse");
        assert_eq!(snapshot, reparsed);
    }

    #[test]
    fn parse_malformed_no_closing_separator() {
        assert!(SnapshotFile::parse("---\nsource: test.py::test\nno closing").is_none());
    }

    #[test]
    fn parse_malformed_no_opening() {
        assert!(SnapshotFile::parse("just content").is_none());
    }

    #[test]
    fn parse_no_metadata() {
        let input = "---\n\n---\nsome content\n";
        let snapshot = SnapshotFile::parse(input).expect("should parse");
        assert!(snapshot.metadata.source.is_none());
        insta::assert_snapshot!(snapshot.content, @r"
        some content
        ");
    }
}
