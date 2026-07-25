use std::fmt;
use std::time::Duration;

use colored::Colorize;
use karva_logging::time::format_duration_bracketed;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlakyTest {
    pub module_name: String,
    pub function_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<String>,
    pub passed_on: u32,
    pub total_attempts: u32,
    pub duration: Duration,
}

impl FlakyTest {
    pub fn from_display_name(
        module_name: &str,
        name: &str,
        passed_on: u32,
        total_attempts: u32,
        duration: Duration,
    ) -> Self {
        let parameter_start = name.find(['(', '[']);
        let (function_name, params) = parameter_start.map_or((name, None), |index| {
            (&name[..index], Some(name[index..].to_string()))
        });
        Self {
            module_name: module_name.to_string(),
            function_name: function_name.to_string(),
            params,
            passed_on,
            total_attempts,
            duration,
        }
    }

    pub fn display(&self) -> DisplayFlakyTest<'_> {
        DisplayFlakyTest(self)
    }
}

pub struct DisplayFlakyTest<'a>(&'a FlakyTest);

impl fmt::Display for DisplayFlakyTest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let record = self.0;
        let label = format!("FLAKY {}/{}", record.passed_on, record.total_attempts);
        let padding = " ".repeat(12usize.saturating_sub(label.len()));
        let colored_label = label.yellow().bold();
        let duration_str = format_duration_bracketed(record.duration);
        let module = record.module_name.cyan();
        let fn_name = record.function_name.blue().bold();
        let params = record
            .params
            .as_deref()
            .map(|p| p.blue().bold().to_string())
            .unwrap_or_default();

        writeln!(
            f,
            "{padding}{colored_label} {duration_str} {module}::{fn_name}{params}"
        )
    }
}

/// Empty slices render as the empty string (no trailing newline).
pub struct DisplayFlakyTests<'a>(&'a [FlakyTest]);

impl<'a> DisplayFlakyTests<'a> {
    pub fn new(records: &'a [FlakyTest]) -> Self {
        Self(records)
    }
}

impl fmt::Display for DisplayFlakyTests<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for record in self.0 {
            write!(f, "{}", record.display())?;
        }
        Ok(())
    }
}
