//! Deterministic projects targeting specific expensive Karva subsystems.

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

mod nested_fixtures;
mod parametrized_matrix;
mod retries;
mod snapshots;

#[derive(Debug, Clone, Copy)]
/// Karva subsystem isolated by a generated benchmark project.
pub enum GeneratedBenchmark {
    /// Deep fixture dependency resolution.
    NestedFixtures,

    /// Large Cartesian parametrization expansion.
    ParametrizedMatrix,

    /// Repeated comparison of large inline snapshots.
    Snapshots,

    /// Re-execution and result tracking for flaky tests.
    Retries,
}

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
        GeneratedBenchmark::NestedFixtures => nested_fixtures::generate(&tests),
        GeneratedBenchmark::ParametrizedMatrix => parametrized_matrix::generate(&tests),
        GeneratedBenchmark::Snapshots => snapshots::generate(&tests),
        GeneratedBenchmark::Retries => retries::generate(&tests),
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8Path;
    use fs_err as fs;
    use tempfile::tempdir;

    use super::{
        GeneratedBenchmark, generate_project, nested_fixtures, parametrized_matrix, retries,
        snapshots,
    };

    #[test]
    fn generated_projects_isolate_targeted_workloads() {
        let temp_dir = tempdir().expect("temporary directory should be created");
        let root = Utf8Path::from_path(temp_dir.path())
            .expect("temporary directory path should be valid UTF-8");

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
    }
}
