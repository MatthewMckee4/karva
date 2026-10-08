//! Standalone editor server; no Python runtime or test worker is required.

fn main() -> anyhow::Result<()> {
    karva_language_server::run_server()
}
