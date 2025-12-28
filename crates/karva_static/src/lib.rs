pub struct EnvVars;

impl EnvVars {
    // Externally defined environment variables

    /// This is a standard Rayon environment variable.
    pub const RAYON_NUM_THREADS: &'static str = "RAYON_NUM_THREADS";

    /// This is a standard Karva environment variable.
    pub const KARVA_MAX_PARALLELISM: &'static str = "KARVA_MAX_PARALLELISM";

    /// This is a standard Karva environment variable.
    pub const KARVA_CONFIG_FILE: &'static str = "KARVA_CONFIG_FILE";
}
