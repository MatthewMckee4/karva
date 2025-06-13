/// A progress reporter.
pub trait Reporter: Send + Sync {
    /// Initialize the reporter with the number of files.
    fn set(&mut self, num: usize);

    /// Report the completion of a given test.
    fn report(&self);
}

/// A no-op implementation of [`Reporter`].
#[derive(Default)]
pub struct DummyReporter;

impl Reporter for DummyReporter {
    fn set(&mut self, _n: usize) {}
    fn report(&self) {}
}
