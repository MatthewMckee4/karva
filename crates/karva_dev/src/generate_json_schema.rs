//! Generate a JSON Schema for `karva.toml`.

use anyhow::Result;
use karva_metadata::Config;
use schemars::generate::SchemaSettings;

use crate::{Mode, apply_mode};

#[derive(clap::Args)]
pub struct Args {
    /// Write the generated schema to stdout (rather than to `karva.schema.json`).
    #[arg(long, default_value_t, value_enum)]
    pub mode: Mode,
}

pub fn main(args: &Args) -> Result<()> {
    let generator = SchemaSettings::draft07().into_generator();
    let schema = generator.into_root_schema_for::<Config>();
    let generated = serde_json::to_string_pretty(&schema)? + "\n";
    apply_mode(args.mode, "karva.schema.json", &generated)
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::{Args, main};
    use crate::Mode;

    #[test]
    fn json_schema_up_to_date() -> Result<()> {
        main(&Args { mode: Mode::Check })
    }
}
