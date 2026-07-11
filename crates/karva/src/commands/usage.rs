use std::io::Write;

use anyhow::Result;
use karva_logging::Printer;

pub fn usage() -> Result<()> {
    let mut stdout = Printer::default().stream_for_message().lock();
    write!(
        stdout,
        "\
Karva usage for agents

Run tests:
  karva test
  karva test path/to/test_file.py
  karva test path/to/test_file.py::test_name

Focus while editing:
  karva test path/to/test_file.py --status-level=fail
  karva test --last-failed
  karva test --filter 'test(~login)'

Debug output:
  karva test --no-parallel
  karva test --no-capture
  karva show-config

Exit codes:
  0  tests passed
  1  tests failed, timed out, or no tests were allowed to run
  2  configuration, discovery, or internal error

Use `karva test --help` for all test options.
"
    )?;

    Ok(())
}
