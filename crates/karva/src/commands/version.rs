use std::io::Write;

use anyhow::Result;
use karva_logging::Printer;

pub fn version() -> Result<()> {
    let mut stdout = Printer::default().stream_for_message().lock();
    writeln!(stdout, "karva {}", karva_version::version())?;

    Ok(())
}
