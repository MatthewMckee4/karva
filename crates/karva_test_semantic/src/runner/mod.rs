//! Runtime execution for discovered tests and fixtures.
//!
//! `PackageRunner` owns run-wide state and package traversal. Its child
//! modules own fixture setup/teardown, variant retries, and outcome policy.
//! Resolver and iterator modules prepare fixture graphs and concrete variants
//! before those lifecycle layers execute them.

mod finalizer_cache;
mod fixture_arguments;
mod fixture_cache;
mod fixture_resolver;
mod package_runner;
mod scoped_storage;
mod test_iterator;

use finalizer_cache::FinalizerCache;
pub use fixture_arguments::FixtureArguments;
use fixture_cache::FixtureCache;
pub use fixture_resolver::{FixtureResolutionEntry, FixtureResolutionError};
pub use package_runner::{FixtureCallError, FixtureChainEntry, PackageRunner};
