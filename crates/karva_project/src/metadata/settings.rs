use crate::metadata::options::OutputFormat;

#[derive(Default, Debug, Clone)]
pub struct ProjectSettings {
    pub(crate) terminal: TerminalSettings,
    pub(crate) src: SrcSettings,
    pub(crate) test: TestSettings,
}

impl ProjectSettings {
    pub const fn terminal(&self) -> &TerminalSettings {
        &self.terminal
    }

    pub const fn src(&self) -> &SrcSettings {
        &self.src
    }

    pub const fn test(&self) -> &TestSettings {
        &self.test
    }

    pub const fn fail_fast(&self) -> bool {
        self.test.fail_fast
    }
}

#[derive(Default, Debug, Clone)]
pub struct TerminalSettings {
    pub output_format: OutputFormat,
    pub show_python_output: bool,
}

#[derive(Default, Debug, Clone)]
pub struct SrcSettings {
    pub respect_ignore_files: bool,
    pub include_paths: Vec<String>,
}

#[derive(Default, Debug, Clone)]
pub struct TestSettings {
    pub test_function_prefix: String,
    pub fail_fast: bool,
}
