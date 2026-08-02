//! Thin process entry point; CLI behavior lives in the `karva` library for integration testing.

use karva::{ExitStatus, karva_main};

fn main() -> ExitStatus {
    karva_main(|args| args)
}
