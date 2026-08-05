//! Generate a Markdown-compatible listing of configuration options for `pyproject.toml`.

use std::{borrow::Cow, fmt::Write};

use itertools::Itertools;
use karva_metadata::{Config, Options};
use ruff_options_metadata::{OptionField, OptionSet, OptionsMetadata, Visit};
use ruff_python_trivia::textwrap;

use crate::{Mode, apply_mode};

const FILE_NAME: &str = "docs/configuration/configuration.md";
const HEADER: &str = "<!-- WARNING: This file is auto-generated (cargo dev generate-all). Update the doc comments on 'Config' and 'Options' in 'crates/karva_metadata/src/options/' if you want to change anything here. -->\n\n# Configuration\n\nKarva is configured through `karva.toml` (or the `[tool.karva]` table in `pyproject.toml`). All option groups live under a `[profile.<name>]` section; see [Profiles](profiles.md) for how to define and select profiles.\n\nThe reference below documents every project and profile field. Profile examples target the implicit `default` profile.\n\n";

#[derive(clap::Args)]
pub struct Args {
    /// Write the generated reference to stdout (rather than to `docs/configuration/configuration.md`).
    #[arg(long, default_value_t, value_enum)]
    pub mode: Mode,
}

pub fn main(args: &Args) -> anyhow::Result<()> {
    apply_mode(args.mode, FILE_NAME, &generate())
}

fn generate() -> String {
    let mut output = String::new();
    output.push_str(HEADER);

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

    output
}

fn generate_set(output: &mut String, set: Set, parents: &mut Vec<Set>, root: Root) {
    match &set {
        Set::Toplevel(_) => {
            let _ = writeln!(output, "## {}\n", root.name());
        }
        Set::Named { name, .. } => {
            let title = parents
                .iter()
                .filter_map(|set| set.name())
                .chain(std::iter::once(name.as_str()))
                .join(".");
            let header_level = "#".repeat(parents.len() + 2);
            let _ = writeln!(output, "{header_level} `{title}`\n");
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

impl Root {
    const fn name(self) -> &'static str {
        match self {
            Self::Config => "Global",
            Self::Profile => "Profiles",
        }
    }
}

fn emit_field(output: &mut String, name: &str, field: &OptionField, parents: &[Set], root: Root) {
    let header_level = "#".repeat(parents.len() + 2);

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
    output.push_str("**Example usage**:\n\n");

    for configuration_file in [
        ConfigurationFile::KarvaToml,
        ConfigurationFile::PyprojectToml,
    ] {
        let (header, example) = format_snippet(
            field.scope,
            field.example,
            parents,
            root,
            configuration_file,
        );
        output.push_str(&format_tab(configuration_file.name(), &header, &example));

        output.push('\n');
    }
}

fn format_tab(tab_name: &str, header: &str, content: &str) -> String {
    let header = if header.is_empty() {
        String::new()
    } else {
        format!("\n    {header}")
    };
    format!(
        "=== \"{}\"\n\n    ```toml{}\n{}\n    ```\n",
        tab_name,
        header,
        textwrap::indent(content, "    ")
    )
}

/// Format the TOML header for the example usage for a given option.
///
/// For example: `[tool.karva.profile.default.src]`.
fn format_snippet<'a>(
    scope: Option<&str>,
    example: &'a str,
    parents: &[Set],
    root: Root,
    configuration: ConfigurationFile,
) -> (String, Cow<'a, str>) {
    let mut example = Cow::Borrowed(example);

    let header = configuration
        .parent_table(root)
        .into_iter()
        .chain(parents.iter().filter_map(|parent| parent.name()))
        .chain(scope)
        .join(".");

    // Rewrite examples starting with `[tool.karva]` or `[[tool.karva]]` to their `karva.toml` equivalent.
    if matches!(configuration, ConfigurationFile::KarvaToml) {
        example = example.replace("[tool.karva.", "[").into();
    }

    // Ex) `[[tool.karva.xx]]`
    if example.starts_with(&format!("[[{header}")) {
        return (String::new(), example);
    }

    // Ex) `[tool.karva.tags]`
    if example.starts_with(&format!("[{header}")) {
        return (String::new(), example);
    }

    if header.is_empty() {
        (String::new(), example)
    } else {
        (format!("[{header}]"), example)
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
    KarvaToml,
}

impl ConfigurationFile {
    const fn name(self) -> &'static str {
        match self {
            Self::PyprojectToml => "pyproject.toml",
            Self::KarvaToml => "karva.toml",
        }
    }

    const fn parent_table(self, root: Root) -> Option<&'static str> {
        match (self, root) {
            (Self::PyprojectToml, Root::Config) => Some("tool.karva"),
            (Self::KarvaToml, Root::Config) => None,
            (Self::PyprojectToml, Root::Profile) => Some("tool.karva.profile.default"),
            (Self::KarvaToml, Root::Profile) => Some("profile.default"),
        }
    }
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
