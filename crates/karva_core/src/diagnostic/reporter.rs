/// A progress reporter.
pub trait Reporter: Send + Sync {
    /// Initialize the reporter with the number of files.
    fn set_files(&mut self, files: usize);

    /// Report the completion of a given file.
    fn report_file(&self, file_name: &str);
}

/// A no-op implementation of [`Reporter`].
#[derive(Default)]
pub struct DummyReporter;

impl Reporter for DummyReporter {
    fn set_files(&mut self, _files: usize) {}
    fn report_file(&self, _file_name: &str) {}
}
