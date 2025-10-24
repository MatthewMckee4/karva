use std::{
    path::{Path, PathBuf},
    process::Command,
};

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
    #[must_use]
    pub fn new() -> Self {
        let test_env = TestContext::new();

        Self { test_env }
    }

    #[must_use]
    pub fn karva_bin(&self) -> PathBuf {
        let venv_bin =
            self.test_env
                .cwd()
                .join(".venv")
                .join(if cfg!(windows) { "Scripts" } else { "bin" });
        venv_bin.join(if cfg!(windows) { "karva.exe" } else { "karva" })
    }

    pub fn with_files<'a>(
        files: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> anyhow::Result<Self> {
        let mut case = Self::new();
        case.write_files(files)?;
        Ok(case)
    }

    pub fn with_file(path: impl AsRef<Path>, content: &str) -> anyhow::Result<Self> {
        let mut case = Self::new();
        case.write_file(path, content)?;
        Ok(case)
    }

    pub fn write_files<'a>(
        &mut self,
        files: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> anyhow::Result<()> {
        for (path, content) in files {
            self.write_file(path, content)?;
        }

        Ok(())
    }

    pub fn write_file(&mut self, path: impl AsRef<Path>, content: &str) -> anyhow::Result<()> {
        self.test_env.write_file(path, content)
    }

    #[must_use]
    pub fn command(&self) -> Command {
        let mut command = Command::new(self.karva_bin());
        command.current_dir(self.test_env.cwd()).arg("test");
        command
    }

    #[must_use]
    pub fn command_with_args(&self, args: &[&str]) -> Command {
        let mut command = self.command();
        command.args(args);
        command
    }
}
