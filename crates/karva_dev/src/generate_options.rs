//! Generate a Markdown-compatible listing of configuration options for `pyproject.toml`.

use std::fmt::Write;

use itertools::Itertools;
use karva_metadata::{Config, Options};
use ruff_options_metadata::{OptionField, OptionSet, OptionsMetadata, Visit};

use crate::{Mode, apply_mode};

#[derive(clap::Args)]
pub struct Args {
    /// Write the generated table to stdout (rather than to `docs/configuration/configuration.md`).
    #[arg(long, default_value_t, value_enum)]
    pub mode: Mode,
}

pub fn main(args: &Args) -> anyhow::Result<()> {
    let mut output = String::new();

    output.push_str(
        "<!-- WARNING: This file is auto-generated (cargo run -p karva_dev generate-all). Update the doc comments on 'Config' and 'Options' in 'crates/karva_metadata/src/options/' if you want to change anything here. -->\n\n",
    );

    generate_set(
        &mut output,
        Set::Toplevel(Config::metadata()),
        &mut Vec::new(),
        Root::Config,
    );
    generate_set(
        &mut output,
        Set::Toplevel(Options::metadata()),
        &mut Vec::new(),
        Root::Profile,
    );

    apply_mode(args.mode, "docs/configuration/configuration.md", &output)
}

fn generate_set(output: &mut String, set: Set, parents: &mut Vec<Set>, root: Root) {
    match (&set, root) {
        (Set::Toplevel(_), Root::Config) => {
            output.push_str("# Configuration\n\n");
            output.push_str(
                "Karva is configured through `karva.toml` (or the `[tool.karva]` table in \
                 `pyproject.toml`). All option groups live under a `[profile.<name>]` section; \
                 see [Profiles](profiles.md) for how to define and select profiles.\n\n",
            );
            output.push_str(
                "The reference below documents every project and profile field. Profile examples \
                 target the implicit `default` profile.\n\n",
            );
        }
        (Set::Toplevel(_), Root::Profile) => {}
        (Set::Named { name, .. }, _) => {
            let title = parents
                .iter()
                .filter_map(|set| set.name())
                .chain(std::iter::once(name.as_str()))
                .join(".");
            let _ = writeln!(output, "## `{title}`\n");
        }
    }

    if let Some(documentation) = set.metadata().documentation() {
        output.push_str(documentation);
        output.push('\n');
        output.push('\n');
    }

    let mut visitor = CollectOptionsVisitor::default();
    set.metadata().record(&mut visitor);

    let (mut fields, mut sets) = (visitor.fields, visitor.groups);

    fields.sort_unstable_by(|(name, _), (name2, _)| name.cmp(name2));
    sets.sort_unstable_by(|(name, _), (name2, _)| name.cmp(name2));

    parents.push(set);

    // Generate the fields.
    for (name, field) in &fields {
        emit_field(output, name, field, parents.as_slice(), root);
        output.push_str("---\n\n");
    }

    // Generate all the sub-sets.
    for (set_name, sub_set) in &sets {
        generate_set(
            output,
            Set::Named {
                name: set_name.clone(),
                set: *sub_set,
            },
            parents,
            root,
        );
    }

    parents.pop();
}

#[derive(Debug)]
enum Set {
    Toplevel(OptionSet),
    Named { name: String, set: OptionSet },
}

impl Set {
    fn name(&self) -> Option<&str> {
        match self {
            Self::Toplevel(_) => None,
            Self::Named { name, .. } => Some(name),
        }
    }

    fn metadata(&self) -> &OptionSet {
        match self {
            Self::Toplevel(set) | Self::Named { set, .. } => set,
        }
    }
}

#[derive(Debug, Copy, Clone)]
enum Root {
    Config,
    Profile,
}

fn emit_field(output: &mut String, name: &str, field: &OptionField, parents: &[Set], root: Root) {
    let header_level = "#".repeat(parents.len() + 1);

    let _ = writeln!(output, "{header_level} `{name}`");

    output.push('\n');

    if let Some(deprecated) = &field.deprecated {
        output.push_str("> [!WARN] \"Deprecated\"\n");
        output.push_str("> This option has been deprecated");

        if let Some(since) = deprecated.since {
            let _ = write!(output, " in {since}");
        }

        output.push('.');

        if let Some(message) = deprecated.message {
            let _ = writeln!(output, " {message}");
        }

        output.push('\n');
    }

    output.push_str(field.doc);
    output.push_str("\n\n");
    let _ = writeln!(output, "**Default value**: `{}`", field.default);
    output.push('\n');
    let _ = writeln!(output, "**Type**: `{}`", field.value_type);
    output.push('\n');
    output.push_str("**Example usage** (`pyproject.toml`):\n\n");
    output.push_str(&format_example(
        &format_header(
            field.scope,
            field.example,
            parents,
            root,
            ConfigurationFile::PyprojectToml,
        ),
        field.example,
    ));
    output.push('\n');
}

fn format_example(header: &str, content: &str) -> String {
    if header.is_empty() {
        format!("```toml\n{content}\n```\n")
    } else {
        format!("```toml\n{header}\n{content}\n```\n")
    }
}

/// Format the TOML header for the example usage for a given option.
///
/// For example: `[tool.karva.profile.default.src]`.
fn format_header(
    scope: Option<&str>,
    example: &str,
    parents: &[Set],
    root: Root,
    configuration: ConfigurationFile,
) -> String {
    let parent_path = match (root, configuration) {
        (Root::Config, ConfigurationFile::PyprojectToml) => "tool.karva",
        (Root::Config, ConfigurationFile::KarvaToml) => "",
        (Root::Profile, ConfigurationFile::PyprojectToml) => "tool.karva.profile.default",
        (Root::Profile, ConfigurationFile::KarvaToml) => "profile.default",
    };

    let header = std::iter::once(parent_path)
        .chain(parents.iter().filter_map(|parent| parent.name()))
        .chain(scope)
        .join(".");

    // Ex) `[[tool.karva.xx]]`
    if example.starts_with(&format!("[[{header}")) {
        return String::new();
    }
    // Ex) `[tool.karva.rules]`
    if example.starts_with(&format!("[{header}")) {
        return String::new();
    }

    if header.is_empty() {
        String::new()
    } else {
        format!("[{header}]")
    }
}

#[derive(Default)]
struct CollectOptionsVisitor {
    groups: Vec<(String, OptionSet)>,
    fields: Vec<(String, OptionField)>,
}

impl Visit for CollectOptionsVisitor {
    fn record_set(&mut self, name: &str, group: OptionSet) {
        self.groups.push((name.to_owned(), group));
    }

    fn record_field(&mut self, name: &str, field: OptionField) {
        self.fields.push((name.to_owned(), field));
    }
}

#[derive(Debug, Copy, Clone)]
enum ConfigurationFile {
    PyprojectToml,
    #[expect(dead_code)]
    KarvaToml,
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::{Args, main};
    use crate::Mode;

    #[test]
    #[cfg(unix)]
    fn configuration_markdown_up_to_date() -> Result<()> {
        main(&Args { mode: Mode::Check })?;
        Ok(())
    }
}
