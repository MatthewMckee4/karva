use std::fmt::Debug;

use crate::discovery::{DiscoveredModule, DiscoveredPackage};
use crate::extensions::fixtures::{DiscoveredFixture, FixtureScope, RejectedFixture};

/// Result of looking up one public fixture name in a provider.
#[must_use]
#[derive(Clone, Copy, Debug)]
pub enum FixtureLookup<'a> {
    /// The provider exposes an accepted fixture.
    Found(&'a DiscoveredFixture),

    /// The provider defines the name, but discovery rejected the fixture.
    Rejected(&'a RejectedFixture),

    /// The provider does not define the name.
    Missing,
}

impl FixtureLookup<'_> {
    /// Returns whether lookup found an accepted fixture.
    pub(crate) const fn is_found(self) -> bool {
        matches!(self, Self::Found(_))
    }
}

/// Supplies fixtures from one position in the runtime provider chain.
pub trait HasFixtures<'a>: Debug {
    /// Resolves one public name while preserving rejected definitions as shadowing results.
    fn lookup_fixture(&'a self, fixture_name: &str) -> FixtureLookup<'a>;

    /// Get all autouse fixtures
    ///
    /// If this returns a non-empty list, it means that the module or package has a configuration module.
    fn auto_use_fixtures(&'a self, scopes: &[FixtureScope]) -> Vec<&'a DiscoveredFixture>;
}

impl<'a> HasFixtures<'a> for DiscoveredModule {
    fn lookup_fixture(&'a self, fixture_name: &str) -> FixtureLookup<'a> {
        if let Some(fixture) = self
            .fixtures()
            .iter()
            .find(|f| f.name().function_name() == fixture_name)
        {
            return FixtureLookup::Found(fixture);
        }

        self.rejected_fixture(fixture_name)
            .map_or(FixtureLookup::Missing, FixtureLookup::Rejected)
    }

    fn auto_use_fixtures(&'a self, scopes: &[FixtureScope]) -> Vec<&'a DiscoveredFixture> {
        self.fixtures()
            .iter()
            .filter(|f| f.auto_use() && scopes.contains(&f.scope()))
            .collect()
    }
}

impl<'a> HasFixtures<'a> for DiscoveredPackage {
    fn lookup_fixture(&'a self, fixture_name: &str) -> FixtureLookup<'a> {
        if let Some(module) = self.configuration_module_impl() {
            match module.lookup_fixture(fixture_name) {
                FixtureLookup::Missing => {}
                lookup => return lookup,
            }
        }

        self.framework_module_impl()
            .map_or(FixtureLookup::Missing, |module| {
                module.lookup_fixture(fixture_name)
            })
    }

    fn auto_use_fixtures(&'a self, scopes: &[FixtureScope]) -> Vec<&'a DiscoveredFixture> {
        let mut fixtures: Vec<&'a DiscoveredFixture> = Vec::new();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

        // User-defined conftest fixtures win on name collision, so they are
        // collected first and framework fixtures with the same unqualified
        // name are dropped.
        if let Some(module) = self.configuration_module_impl() {
            for fixture in module.auto_use_fixtures(scopes) {
                if seen.insert(fixture.name().function_name()) {
                    fixtures.push(fixture);
                }
            }
        }

        if let Some(module) = self.framework_module_impl() {
            for fixture in module.auto_use_fixtures(scopes) {
                if seen.insert(fixture.name().function_name()) {
                    fixtures.push(fixture);
                }
            }
        }

        fixtures
    }
}

impl<'a> HasFixtures<'a> for &'a DiscoveredPackage {
    fn lookup_fixture(&'a self, fixture_name: &str) -> FixtureLookup<'a> {
        (*self).lookup_fixture(fixture_name)
    }

    fn auto_use_fixtures(&'a self, scopes: &[FixtureScope]) -> Vec<&'a DiscoveredFixture> {
        (*self).auto_use_fixtures(scopes)
    }
}
