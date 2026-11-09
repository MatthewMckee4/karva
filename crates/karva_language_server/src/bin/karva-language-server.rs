//! Standalone stdio server for editors, without the CLI's Python runtime linkage.

fn main() -> anyhow::Result<()> {
    karva_language_server::run_server()
}
