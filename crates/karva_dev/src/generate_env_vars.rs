//! Generate a Markdown reference for environment variables consumed and
//! produced by Karva.

use std::fmt::Write;

use karva_static::{EnvVarDoc, EnvVars, WorkerEnvVars};

use crate::{Mode, apply_mode};

#[derive(clap::Args)]
pub struct Args {
    /// Write the generated reference to stdout (rather than to `docs/reference/env-vars.md`).
    #[arg(long, default_value_t, value_enum)]
    pub mode: Mode,
}

const FILE_NAME: &str = "docs/reference/env-vars.md";

const HEADER: &str = "<!-- WARNING: This file is auto-generated (cargo run -p karva_dev generate-all). Update the doc comments on the env-var structs in 'crates/karva_static/src/lib.rs' if you want to change anything here. -->\n\n# Environment Variables\n\nThis page lists every environment variable that Karva reads from the \
environment, plus the variables the worker exposes to running tests.\n\n";

pub fn main(args: &Args) -> anyhow::Result<()> {
    apply_mode(args.mode, FILE_NAME, &generate())
}

fn generate() -> String {
    let mut output = String::new();
    output.push_str(HEADER);

    emit_section(
        &mut output,
        "Read by Karva",
        "Variables Karva reads from the environment to influence its own behavior.",
        EnvVars::METADATA,
    );

    emit_section(
        &mut output,
        "Set by the worker on tests",
        "Variables the Karva worker writes into the test process so running \
         test code can introspect the run, the worker, and its own attempt.",
        WorkerEnvVars::METADATA,
    );

    output
}

fn emit_section(output: &mut String, title: &str, description: &str, vars: &[EnvVarDoc]) {
    let _ = writeln!(output, "## {title}\n");
    output.push_str(description);
    output.push_str("\n\n");

    for var in vars {
        let _ = writeln!(output, "### `{}`\n", var.name);
        for line in var.doc_lines {
            let trimmed = line.strip_prefix(' ').unwrap_or(line);
            let _ = writeln!(output, "{trimmed}");
        }
        output.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::{Args, main};
    use crate::Mode;

    #[test]
    #[cfg(unix)]
    fn env_vars_markdown_up_to_date() -> Result<()> {
        main(&Args { mode: Mode::Check })
    }
}
