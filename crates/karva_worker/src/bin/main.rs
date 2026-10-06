//! Thin worker entry point; IPC setup and embedded-Python startup live in `karva_worker`.

use karva_cli::ExitStatus;
use karva_worker::runtime::karva_worker_main;

fn main() -> ExitStatus {
    karva_worker_main()
}
