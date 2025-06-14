use std::{collections::HashMap, io::Write};

use colored::{Color, Colorize};
use karva_project::project::Project;
use pyo3::prelude::*;

use crate::{
    diagnostic::{
        Diagnostic, DiagnosticScope, SubDiagnosticType,
        reporter::{DummyReporter, Reporter},
    },
    discovery::Discoverer,
    fixture::{CalledFixtures, FixtureScope, HasFixtures, TestCaseFixtures},
    module::Module,
    package::Package,
    utils::add_to_sys_path,
};

pub trait TestRunner {
    fn test(&self) -> RunDiagnostics;
    fn test_with_reporter(&self, reporter: &mut dyn Reporter) -> RunDiagnostics;
}

pub struct StandardTestRunner<'proj> {
    project: &'proj Project,
}

impl<'proj> StandardTestRunner<'proj> {
    #[must_use]
    pub const fn new(project: &'proj Project) -> Self {
        Self { project }
    }

    fn test_impl(&self, reporter: &mut dyn Reporter) -> RunDiagnostics {
        let (session, discovery_diagnostics) = Discoverer::new(self.project).discover();

        let total_files = session.total_test_modules();

        let total_test_cases = session.total_test_cases();

        tracing::info!(
            "Discovered {} tests in {} files",
            total_test_cases,
            total_files
        );

        reporter.set(total_files);

        let mut diagnostics = Vec::new();

        diagnostics.extend(discovery_diagnostics);
        Python::with_gil(|py| {
            if add_to_sys_path(&py, self.project.cwd()).is_err() {
                tracing::error!("Failed to add {} to sys.path", self.project.cwd());
                return;
            }

            // let session_fixtures = HashMap::new();
            let session_fixtures =
                session.called_fixtures(py, &[FixtureScope::Session], &session.test_cases());

            self.test_package(
                py,
                &session,
                &[],
                &session_fixtures,
                &mut diagnostics,
                reporter,
            );
        });

        RunDiagnostics {
            diagnostics,
            total_tests: total_test_cases,
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::unused_self)]
    fn test_module<'a>(
        &self,
        py: Python<'a>,
        module: &'a Module<'a>,
        package: &'a Package<'a>,
        parents: &'a [&'a Package<'a>],
        parent_fixtures: &'a CalledFixtures<'a>,
        diagnostics: &mut Vec<Diagnostic>,
        reporter: &dyn Reporter,
    ) {
        if module.total_test_cases() == 0 {
            return;
        }

        let mut current_module_fixtures: CalledFixtures<'a> = HashMap::new();

        let module_fixtures = module.called_fixtures(
            py,
            &[
                FixtureScope::Module,
                FixtureScope::Package,
                FixtureScope::Session,
            ],
            &module.test_cases(),
        );
        for (name, fixture) in module_fixtures {
            current_module_fixtures.insert(name, fixture);
        }

        let package_fixtures =
            package.called_fixtures(py, &[FixtureScope::Module], &module.test_cases());
        for (name, fixture) in package_fixtures {
            current_module_fixtures.insert(name, fixture);
        }

        for parent in parents {
            let parent_fixtures =
                parent.called_fixtures(py, &[FixtureScope::Module], &module.test_cases());
            for (name, fixture) in parent_fixtures {
                current_module_fixtures.insert(name, fixture);
            }
        }

        for (name, fixture) in parent_fixtures {
            current_module_fixtures.insert(name.clone(), fixture.clone());
        }

        let py_module = match PyModule::import(py, module.name()) {
            Ok(py_module) => py_module,
            Err(err) => {
                diagnostics.extend(vec![Diagnostic::from_py_err(
                    py,
                    &err,
                    DiagnosticScope::Setup,
                    &module.path().to_string(),
                )]);
                return;
            }
        };

        for function in module.test_cases() {
            let mut current_function_fixtures: CalledFixtures<'a> = HashMap::new();

            let function_module_fixtures =
                module.called_fixtures(py, &[FixtureScope::Function], &[function]);
            for (name, fixture) in function_module_fixtures {
                current_function_fixtures.insert(name, fixture);
            }

            let function_package_fixtures =
                package.called_fixtures(py, &[FixtureScope::Function], &[function]);
            for (name, fixture) in function_package_fixtures {
                current_function_fixtures.insert(name, fixture);
            }

            for parent in parents {
                let parent_fixtures =
                    parent.called_fixtures(py, &[FixtureScope::Function], &[function]);
                for (name, fixture) in parent_fixtures {
                    current_function_fixtures.insert(name, fixture);
                }
            }

            for (name, fixture) in &current_module_fixtures {
                current_function_fixtures.insert(name.clone(), fixture.clone());
            }

            let test_name = function.to_string();
            tracing::info!("Running test: {}", test_name);

            let test_case_fixtures = TestCaseFixtures::new(&current_function_fixtures);
            if let Some(result) = function.run_test(py, &py_module, &test_case_fixtures) {
                diagnostics.push(result);
            }
        }

        reporter.report();
    }

    fn test_package<'a>(
        &self,
        py: Python<'a>,
        package: &'a Package<'a>,
        parents: &'a [&'a Package<'a>],
        parent_fixtures: &'a CalledFixtures<'a>,
        diagnostics: &mut Vec<Diagnostic>,
        reporter: &dyn Reporter,
    ) {
        if package.total_test_cases() == 0 {
            return;
        }

        let mut current_package_fixtures: CalledFixtures<'a> = HashMap::new();

        let package_fixtures = package.called_fixtures(
            py,
            &[FixtureScope::Package, FixtureScope::Session],
            &package.direct_test_cases(),
        );

        for (name, fixture) in package_fixtures {
            current_package_fixtures.insert(name, fixture);
        }

        for parent in parents {
            let parent_fixtures =
                parent.called_fixtures(py, &[FixtureScope::Package], &package.direct_test_cases());

            for (name, fixture) in parent_fixtures {
                current_package_fixtures.insert(name, fixture);
            }
        }

        for (name, fixture) in parent_fixtures {
            current_package_fixtures.insert(name.clone(), fixture.clone());
        }

        let mut new_parents = Vec::new();
        new_parents.extend_from_slice(parents);
        new_parents.push(package);

        for module in package.modules().values() {
            self.test_module(
                py,
                module,
                package,
                parents,
                &current_package_fixtures,
                diagnostics,
                reporter,
            );
        }

        for sub_package in package.packages().values() {
            self.test_package(
                py,
                sub_package,
                &new_parents,
                &current_package_fixtures,
                diagnostics,
                reporter,
            );
        }
    }
}

impl TestRunner for StandardTestRunner<'_> {
    fn test(&self) -> RunDiagnostics {
        self.test_impl(&mut DummyReporter)
    }

    fn test_with_reporter(&self, reporter: &mut dyn Reporter) -> RunDiagnostics {
        self.test_impl(reporter)
    }
}

impl TestRunner for Project {
    fn test(&self) -> RunDiagnostics {
        let test_runner = StandardTestRunner::new(self);
        test_runner.test()
    }

    fn test_with_reporter(&self, reporter: &mut dyn Reporter) -> RunDiagnostics {
        let test_runner = StandardTestRunner::new(self);
        test_runner.test_with_reporter(reporter)
    }
}

#[derive(Clone)]
pub struct RunDiagnostics {
    diagnostics: Vec<Diagnostic>,
    total_tests: usize,
}

impl RunDiagnostics {
    #[must_use]
    pub const fn new(test_results: Vec<Diagnostic>, total_tests: usize) -> Self {
        Self {
            diagnostics: test_results,
            total_tests,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    #[must_use]
    pub fn test_results(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    #[must_use]
    pub fn stats(&self) -> DiagnosticStats {
        let mut stats = DiagnosticStats::new(self.total_tests);
        for diagnostic in &self.diagnostics {
            if diagnostic.scope() == &DiagnosticScope::Test {
                stats.passed -= 1;
                match diagnostic.diagnostic_type() {
                    SubDiagnosticType::Fail => stats.failed += 1,
                    SubDiagnosticType::Error(_) => stats.error += 1,
                }
            }
        }
        stats
    }

    fn log_test_count(writer: &mut dyn Write, label: &str, count: usize, color: Color) {
        if count > 0 {
            let _ = writeln!(
                writer,
                "{} {}",
                label.color(color),
                count.to_string().color(color)
            );
        }
    }

    pub fn display(&self, writer: &mut dyn Write) {
        let stats = self.stats();

        if stats.total() > 0 {
            let _ = writeln!(writer, "{}", "-------------".bold());
            for (label, num, color) in [
                ("Passed tests:", stats.passed(), Color::Green),
                ("Failed tests:", stats.failed(), Color::Red),
                ("Error tests:", stats.error(), Color::Yellow),
            ] {
                Self::log_test_count(writer, label, num, color);
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter()
    }
}

#[derive(Debug)]
pub struct DiagnosticStats {
    total: usize,
    passed: usize,
    failed: usize,
    error: usize,
}

impl DiagnosticStats {
    const fn new(total: usize) -> Self {
        Self {
            total,
            passed: total,
            failed: 0,
            error: 0,
        }
    }
    #[must_use]
    pub const fn total(&self) -> usize {
        self.total
    }

    #[must_use]
    pub const fn passed(&self) -> usize {
        self.passed
    }

    #[must_use]
    pub const fn failed(&self) -> usize {
        self.failed
    }

    #[must_use]
    pub const fn error(&self) -> usize {
        self.error
    }
}

#[cfg(test)]
mod tests {
    use karva_project::tests::TestEnv;

    use super::*;

    #[test]
    fn test_runner_with_passing_test() {
        let env = TestEnv::new();
        env.create_file(
            "test_pass.py",
            r"
def test_simple_pass():
    assert True
",
        );

        let project = Project::new(env.cwd(), vec![env.temp_path("test_pass.py")]);
        let runner = StandardTestRunner::new(&project);

        let result = runner.test();

        assert_eq!(result.stats().total(), 1);
        assert_eq!(result.stats().passed(), 1);
        assert_eq!(result.stats().failed(), 0);
        assert_eq!(result.stats().error(), 0);
    }

    #[test]
    fn test_runner_with_failing_test() {
        let env = TestEnv::new();
        env.create_file(
            "test_fail.py",
            r#"
def test_simple_fail():
    assert False, "This test should fail"
"#,
        );

        let project = Project::new(env.cwd(), vec![env.temp_path("test_fail.py")]);
        let runner = StandardTestRunner::new(&project);

        let result = runner.test();

        assert_eq!(result.stats().total(), 1);
        assert_eq!(result.stats().passed(), 0);
        assert_eq!(result.stats().failed(), 1);
        assert_eq!(result.stats().error(), 0);
    }

    #[test]
    fn test_runner_with_error_test() {
        let env = TestEnv::new();
        env.create_file(
            "test_error.py",
            r#"
def test_simple_error():
    raise ValueError("This is an error")
"#,
        );

        let project = Project::new(env.cwd(), vec![env.temp_path("test_error.py")]);
        let runner = StandardTestRunner::new(&project);

        let result = runner.test();

        assert_eq!(result.stats().total(), 1);
        assert_eq!(result.stats().passed(), 0);
        assert_eq!(result.stats().failed(), 0);
        assert_eq!(result.stats().error(), 1);
    }

    #[test]
    fn test_runner_with_multiple_tests() {
        let env = TestEnv::new();
        env.create_file(
            "test_mixed.py",
            r#"def test_pass():
    assert True

def test_fail():
    assert False, "This test should fail"

def test_error():
    raise ValueError("This is an error")
"#,
        );

        let project = Project::new(env.cwd(), vec![env.temp_path("test_mixed.py")]);
        let runner = StandardTestRunner::new(&project);

        let result = runner.test();

        assert_eq!(result.stats().total(), 3);
        assert_eq!(result.stats().passed(), 1);
        assert_eq!(result.stats().failed(), 1);
        assert_eq!(result.stats().error(), 1);
    }
}
