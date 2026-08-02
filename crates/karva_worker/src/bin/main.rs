//! Thin worker entry point; parsing and embedded-Python startup live in `karva_worker`.

use karva_cli::ExitStatus;
use karva_worker::cli::karva_worker_main;

fn main() -> ExitStatus {
    karva_worker_main(|args| args)
}
