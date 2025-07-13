#[cfg(not(codspeed))]
pub use criterion::*;

#[cfg(not(codspeed))]
pub type BenchmarkGroup<'a> = criterion::BenchmarkGroup<'a, measurement::WallTime>;

#[cfg(codspeed)]
pub use codspeed_criterion_compat::*;
