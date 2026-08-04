//! Deterministic projects targeting specific expensive Karva subsystems.

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

mod dense_fixtures;
mod many_modules;
mod nested_fixtures;
mod parametrized_matrix;
mod retries;
mod snapshots;
mod wide_fixtures;

#[derive(Debug, Clone, Copy)]
/// Karva subsystem isolated by a generated benchmark project.
pub enum GeneratedBenchmark {
    /// Dense fixture dependency graph resolution.
    DenseFixtures,

    /// File discovery, collection, import, and scheduling across many modules.
    ManyModules,

    /// Deep fixture dependency resolution.
    NestedFixtures,

    /// Large Cartesian parametrization expansion.
    ParametrizedMatrix,

    /// Repeated comparison of large inline snapshots.
    Snapshots,

    /// Re-execution and result tracking for flaky tests.
    Retries,

    /// Wide fixture dependency resolution and repeated fixture execution.
    WideFixtures,
}

/// Recreates one generated benchmark project, shaped on a small scale like:
///
/// ```text
/// pyproject.toml
/// tests/
///   conftest.py  # when fixtures are needed
///   test_workload.py
/// ```
pub fn generate_project(workload: GeneratedBenchmark, project_root: &Utf8Path) -> Result<()> {
    let tests = project_root.join("tests");
    if tests.exists() {
        fs::remove_dir_all(&tests).context("Failed to clear generated benchmark tests")?;
    }
    fs::create_dir_all(&tests).context("Failed to create generated benchmark tests")?;
    let retry = if matches!(workload, GeneratedBenchmark::Retries) {
        "\n[tool.karva.profile.default.test]\nretry = 1\n"
    } else {
        ""
    };
    fs::write(
        project_root.join("pyproject.toml"),
        format!(
            "[project]\nname = \"karva-custom-benchmark\"\nversion = \"0.0.0\"\nrequires-python = \">=3.13\"\n{retry}"
        ),
    )
    .context("Failed to write generated benchmark project metadata")?;

    match workload {
        GeneratedBenchmark::DenseFixtures => dense_fixtures::generate(&tests),
        GeneratedBenchmark::ManyModules => many_modules::generate(&tests),
        GeneratedBenchmark::NestedFixtures => nested_fixtures::generate(&tests),
        GeneratedBenchmark::ParametrizedMatrix => parametrized_matrix::generate(&tests),
        GeneratedBenchmark::Snapshots => snapshots::generate(&tests),
        GeneratedBenchmark::Retries => retries::generate(&tests),
        GeneratedBenchmark::WideFixtures => wide_fixtures::generate(&tests),
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8Path;
    use fs_err as fs;
    use tempfile::tempdir;

    use super::{
        GeneratedBenchmark, dense_fixtures, generate_project, many_modules, nested_fixtures,
        parametrized_matrix, retries, snapshots, wide_fixtures,
    };

    #[test]
    fn generated_projects_isolate_targeted_workloads() {
        let temp_dir = tempdir().expect("temporary directory should be created");
        let root = Utf8Path::from_path(temp_dir.path())
            .expect("temporary directory path should be valid UTF-8");

        generate_project(GeneratedBenchmark::DenseFixtures, root)
            .expect("dense fixture project should be generated");
        let fixtures = fs::read_to_string(root.join("tests/conftest.py"))
            .expect("fixtures should be readable");
        let fixture_tests = fs::read_to_string(root.join("tests/test_dense_fixtures.py"))
            .expect("dense fixture tests should be readable");
        assert_eq!(
            fixtures.matches("@pytest.fixture").count(),
            dense_fixtures::FIXTURES
        );
        assert!(fixtures.contains("def fixture_2(fixture_0, fixture_1):"));
        assert_eq!(
            fixture_tests.matches("def test_dense_fixtures_").count(),
            dense_fixtures::TESTS
        );

        generate_project(GeneratedBenchmark::ManyModules, root)
            .expect("many-module project should be generated");
        let first_module = fs::read_to_string(root.join("tests/test_0.py"))
            .expect("first test module should be readable");
        assert!(!root.join("tests/conftest.py").exists());
        assert_eq!(
            fs::read_dir(root.join("tests"))
                .expect("tests directory should be readable")
                .count(),
            many_modules::MODULES
        );
        assert_eq!(
            first_module.matches("def test_0_").count(),
            many_modules::TESTS_PER_MODULE
        );

        generate_project(GeneratedBenchmark::NestedFixtures, root)
            .expect("nested fixture project should be generated");
        let fixtures = fs::read_to_string(root.join("tests/conftest.py"))
            .expect("fixtures should be readable");
        let fixture_tests = fs::read_to_string(root.join("tests/test_nested_fixtures.py"))
            .expect("nested fixture tests should be readable");
        assert_eq!(
            fixtures.matches("@pytest.fixture").count(),
            nested_fixtures::DEPTH
        );
        assert_eq!(
            fixture_tests.matches("def test_nested_fixtures_").count(),
            nested_fixtures::TESTS
        );

        generate_project(GeneratedBenchmark::ParametrizedMatrix, root)
            .expect("parametrized matrix project should be generated");
        let parametrized = fs::read_to_string(root.join("tests/test_parametrized_matrix.py"))
            .expect("parametrized tests should be readable");
        let metadata = fs::read_to_string(root.join("pyproject.toml"))
            .expect("project metadata should be readable");
        let parameter_values = (0..parametrized_matrix::VALUES).collect::<Vec<_>>();
        assert!(!root.join("tests/conftest.py").exists());
        assert!(!metadata.contains("retry = 1"));
        assert_eq!(
            parametrized
                .matches(&format!("{parameter_values:?}"))
                .count(),
            3
        );

        generate_project(GeneratedBenchmark::Snapshots, root)
            .expect("snapshot project should be generated");
        let snapshot_source = fs::read_to_string(root.join("tests/test_snapshots.py"))
            .expect("snapshot tests should be readable");
        assert!(!root.join("tests/test_parametrized_matrix.py").exists());
        assert_eq!(
            snapshot_source.matches("def test_snapshot_").count(),
            snapshots::CASES
        );
        assert_eq!(snapshot_source.matches("record ").count(), snapshots::LINES);

        generate_project(GeneratedBenchmark::Retries, root)
            .expect("retry project should be generated");
        let metadata = fs::read_to_string(root.join("pyproject.toml"))
            .expect("project metadata should be readable");
        let retry_source = fs::read_to_string(root.join("tests/test_retries.py"))
            .expect("retry tests should be readable");
        assert!(!root.join("tests/test_snapshots.py").exists());
        assert!(metadata.contains("retry = 1"));
        assert_eq!(
            retry_source.matches("def test_retry_").count(),
            retries::CASES
        );
        assert!(retry_source.contains("\n    assert"));

        generate_project(GeneratedBenchmark::WideFixtures, root)
            .expect("wide fixture project should be generated");
        let fixtures = fs::read_to_string(root.join("tests/conftest.py"))
            .expect("fixtures should be readable");
        let fixture_tests = fs::read_to_string(root.join("tests/test_wide_fixtures.py"))
            .expect("wide fixture tests should be readable");
        assert!(!root.join("tests/test_retries.py").exists());
        assert_eq!(
            fixtures.matches("@pytest.fixture").count(),
            wide_fixtures::FIXTURES
        );
        assert_eq!(
            fixture_tests.matches("def test_wide_fixtures_").count(),
            wide_fixtures::TESTS
        );
    }
}
