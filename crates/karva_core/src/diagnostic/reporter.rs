/// A progress reporter.
pub trait Reporter: Send + Sync {
    /// Initialize the reporter with the number of files.
    fn set_files(&mut self, files: usize);

    /// Report the completion of a given test.
    fn report_test(&self, test_name: &str);
}

/// A no-op implementation of [`Reporter`].
#[derive(Default)]
pub struct DummyReporter;

impl Reporter for DummyReporter {
    fn set_files(&mut self, _files: usize) {}
    fn report_test(&self, _test_name: &str) {}
}
