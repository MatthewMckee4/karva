use karva_cli::ExitStatus;
use karva_worker::cli::karva_worker_main;

fn main() -> ExitStatus {
    karva_worker_main(|args| args)
}
