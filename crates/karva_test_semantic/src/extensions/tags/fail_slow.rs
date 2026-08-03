/// Represents a per-test duration budget, in seconds.
///
/// Unlike [`super::timeout::TimeoutTag`], this never kills a running test.
/// The budget is checked after the test's full lifecycle (setup, call, and
/// teardown) completes; see `PackageRunner::execute_test_variant`.
#[derive(Debug, Clone, Copy)]
pub struct FailSlowTag {
    seconds: f64,
}

impl FailSlowTag {
    pub(super) fn new(seconds: f64) -> Self {
        Self { seconds }
    }

    pub(crate) fn seconds(self) -> f64 {
        self.seconds
    }
}
