use karva_worker::cli::{ExitStatus, karva_worker_main};

fn main() -> ExitStatus {
    karva_worker_main(|args| args)
}
