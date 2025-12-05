use std::num::NonZeroUsize;

pub struct EnvVars;

impl EnvVars {
    // Externally defined environment variables

    /// This is a standard Rayon environment variable.
    pub const RAYON_NUM_THREADS: &'static str = "RAYON_NUM_THREADS";

    /// This is a standard Karva environment variable.
    pub const KARVA_CONFIG_FILE: &'static str = "KARVA_CONFIG_FILE";
}

pub fn max_parallelism() -> NonZeroUsize {
    std::env::var(EnvVars::RAYON_NUM_THREADS)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism().unwrap_or_else(|_| NonZeroUsize::new(1).unwrap())
        })
}
