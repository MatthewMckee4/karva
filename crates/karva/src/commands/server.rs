use anyhow::Result;
use karva_cli::ExitStatus;

pub fn server() -> Result<ExitStatus> {
    karva_language_server::run_server()?;
    Ok(ExitStatus::Success)
}
