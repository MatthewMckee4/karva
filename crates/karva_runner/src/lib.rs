mod orchestration;

pub use orchestration::{run_parallel_tests, ParallelTestConfig};

// Re-export from karva_collector
pub use karva_collector::{CollectedModule, CollectedPackage, ModuleType, ParallelCollector};

// Re-export from karva_diagnostic
pub use karva_diagnostic::{TestResultKind, TestResultStats, TestRunResult};
