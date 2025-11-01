use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};

use crate::TestContext;

pub struct IntegrationTestContext {
    test_env: TestContext,
}

impl Default for IntegrationTestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl IntegrationTestContext {
    pub fn new() -> Self {
        let test_env = TestContext::new();

        Self { test_env }
    }

    pub fn karva_bin(&self) -> Utf8PathBuf {
        let venv_bin =
            self.test_env
                .cwd()
                .join(".venv")
                .join(if cfg!(windows) { "Scripts" } else { "bin" });
        venv_bin.join(if cfg!(windows) { "karva.exe" } else { "karva" })
    }

    pub fn with_files<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut case = Self::new();
        case.write_files(files);
        case
    }

    pub fn with_file(path: impl AsRef<Utf8Path>, content: &str) -> Self {
        let mut case = Self::new();
        case.write_file(path, content);
        case
    }

    pub fn write_files<'a>(&mut self, files: impl IntoIterator<Item = (&'a str, &'a str)>) {
        for (path, content) in files {
            self.write_file(path, content);
        }
    }

    pub fn write_file(&mut self, path: impl AsRef<Utf8Path>, content: &str) {
        self.test_env.write_file(path, content);
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(self.karva_bin());
        command.current_dir(self.test_env.cwd()).arg("test");
        command
    }
}
