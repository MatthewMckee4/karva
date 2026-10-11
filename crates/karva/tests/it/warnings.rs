//! Warning capture, policy, isolation, and report behavior across worker boundaries.

use std::fmt::Write as _;

use insta::assert_snapshot;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;
use serde_json::Value;

use crate::common::TestContext;

#[test]
fn captures_unconsumed_warnings_in_every_phase_and_report() {
    let context = TestContext::new();
    context.write_file(
        "test.py",
        r#"
import warnings
import karva

warnings.warn("import warning", DeprecationWarning)

@karva.fixture
def value():
    warnings.warn("setup warning", ResourceWarning)
    yield 1
    warnings.warn("teardown warning", RuntimeWarning)

@karva.fixture(scope="session", auto_use=True)
def session():
    warnings.warn("session setup")
    yield
    warnings.warn("session teardown")

def test_warn(value):
    warnings.warn("call warning", DeprecationWarning)
    assert value == 1

def test_consumed(recwarn):
    warnings.warn("consumed fixture")
    assert len(recwarn) == 1

def test_recorded():
    with karva.warns(UserWarning):
        warnings.warn("consumed helper")
    with warnings.catch_warnings(record=True) as recorded:
        warnings.warn("consumed context")
        assert len(recorded) == 1
"#,
    );
    context.write_file(
        "karva.toml",
        "[profile.default.junit]\npath = \"results.xml\"\n",
    );
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--status-level=none", "--result-output=results.json",])
    );
    let report: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report");
    let warnings = report["warnings"].as_array().expect("warnings");
    assert_eq!(warnings.len(), 6);
    assert_eq!(
        warnings
            .iter()
            .filter(|warning| warning["test"].is_string())
            .count(),
        3
    );
    assert!(warnings.iter().all(|warning| {
        !warning["message"]
            .as_str()
            .expect("message")
            .contains("consumed")
    }));
    let xml = context.read_file("results.xml");
    let duration = regex::Regex::new(r#"time="[0-9.]+""#).expect("duration regex");
    assert_snapshot!(duration.replace_all(&xml, "time=\"[TIME]\""));
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--status-level=none",
        "--result-output=results.jsonl",
        "--result-format=jsonl",
    ]));
    let jsonl = context.read_file("results.jsonl");
    let warnings = jsonl
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("JSONL record"))
        .filter(|record| record["type"] == "warning")
        .collect::<Vec<_>>();
    assert_eq!(warnings.len(), 6);
    assert!(
        warnings
            .iter()
            .any(|warning| warning["test"] == "test::test_warn(value=1)" && warning["attempt"] == 1)
    );
}

#[rstest]
fn repeated_warnings_are_counted_across_workers(#[values(1, 2)] workers: usize) {
    let context = TestContext::new();
    let mut source =
        "import warnings\n\ndef emit():\n    warnings.warn('repeated', DeprecationWarning)\n\n"
            .to_string();
    for index in 0..10 {
        write!(source, "def test_{index}():\n    emit()\n    emit()\n\n")
            .expect("generate test source");
    }
    context.write_file("test.py", &source);
    insta::allow_duplicates! {
        assert_cmd_snapshot!("repeated_warning_budget_exceeded", context.command().args([
            &format!("--num-workers={workers}"), "--status-level=none", "--max-warnings=9",
            "--result-output=results.json",
        ]));
    }
    let report: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report");
    assert_eq!(report["warnings"].as_array().expect("warnings").len(), 10);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["stats"]["passed"], 10);
    insta::allow_duplicates! {
        assert_cmd_snapshot!("repeated_warning_budget_met", context.command().args([
            &format!("--num-workers={workers}"), "--status-level=none", "--max-warnings=10",
        ]));
    }
}

#[test]
fn profiles_cli_and_local_tags_have_ordered_precedence() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import warnings
import karva
import pytest

@karva.tags.filter_warnings("ignore:local:DeprecationWarning")
def test_karva():
    warnings.warn("local", DeprecationWarning)

@pytest.mark.filterwarnings("ignore:local:DeprecationWarning")
def test_pytest():
    warnings.warn("local", DeprecationWarning)

def test_visible():
    warnings.warn("visible", DeprecationWarning)
"#,
    );
    context.write_file(
        "karva.toml",
        r#"
[profile.default.test]
warning-filters = ["ignore"]
[profile.ci.test]
warning-filters = ["error", "ignore:visible:DeprecationWarning"]
"#,
    );
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--profile=ci", "--status-level=none"])
    );
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--profile=ci",
        "--status-level=none",
        "-W",
        "default",
    ]));
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--status-level=none",
        "-W",
        "error",
    ]));
}

#[test]
fn isolates_filters_registries_parameters_and_retries() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import warnings
import os
import karva

@karva.tags.parametrize("value", [1, 2])
def test_case(value):
    warnings.warn("case", DeprecationWarning)
    warnings.simplefilter("ignore")
    assert int(os.environ["KARVA_ATTEMPT"]) == 2
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--retry=1",
        "--status-level=none",
        "--result-output=results.json",
    ]));
    let report: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report");
    let warnings = report["warnings"].as_array().expect("warnings");
    assert_eq!(warnings.len(), 4);
    assert_eq!(
        warnings
            .iter()
            .filter(|warning| warning["attempt"] == 1)
            .count(),
        2
    );
    assert_eq!(
        warnings
            .iter()
            .filter(|warning| warning["attempt"] == 2)
            .count(),
        2
    );
}

#[rstest]
#[case("bad-action")]
#[case("ignore::UnknownWarning")]
#[case("ignore::ValueError")]
#[case("ignore:[")]
#[case("ignore::::-1")]
#[case("ignore::::x")]
#[case("ignore::::1:extra")]
fn rejects_invalid_project_filters(#[case] filter: &str) {
    let context = TestContext::with_file("test.py", "def test_pass():\n    assert False\n");
    context.write_file(
        "karva.toml",
        &format!("[profile.default.test]\nwarning-filters = [{filter:?}]\n"),
    );
    insta::with_settings!({snapshot_suffix => filter.replace(':', "_").replace('[', "regex")}, {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"));
    });
}

#[rstest]
fn rejects_invalid_local_filters(
    #[values("karva.tags.filter_warnings", "pytest.mark.filterwarnings")] decorator: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
import karva
import pytest
@{decorator}("ignore::UnknownWarning")
def test_never_runs():
    assert False
"#
        ),
    );
    insta::with_settings!({snapshot_suffix => decorator}, {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"));
    });
}

#[test]
fn restores_registry_objects_and_new_module_registries() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import sys
import warnings
import karva

original = {"sentinel": 1}
__warningregistry__ = original

@karva.fixture(scope="session", auto_use=True)
def verify_restoration():
    filters = warnings.filters[:]
    yield
    assert __warningregistry__ is original
    assert __warningregistry__ == {"sentinel": 1}
    assert warnings.filters == filters
    assert not hasattr(sys.modules["imported"], "__warningregistry__")

@karva.tags.parametrize("value", [1, 2])
def test_modify(value):
    import imported
    assert __warningregistry__ == {}
    warnings.warn("registry warning")
    warnings.filters.clear()
"#,
    );
    context.write_file(
        "imported.py",
        "import warnings\nwarnings.warn('imported during test')\n",
    );
    assert_cmd_snapshot!(context.command_no_parallel().args(["--status-level=none"]));
}

#[test]
fn dotted_categories_message_module_and_line_filters() {
    let context = TestContext::with_file(
        "categories.py",
        "class CustomWarning(ResourceWarning):\n    pass\n",
    );
    context.write_file("test.py", r#"
import warnings
import karva
import categories

@karva.tags.filter_warnings("error", "a:allowed:categories.CustomWarning:^target.module$:42")
def test_filters():
    for _ in range(2):
        warnings.warn_explicit("allowed", categories.CustomWarning, "custom.py", 42, module="target.module")
    with karva.raises(categories.CustomWarning):
        warnings.warn_explicit("allowed", categories.CustomWarning, "custom.py", 43, module="target.module")
    with karva.raises(categories.CustomWarning):
        warnings.warn_explicit("allowed", categories.CustomWarning, "custom.py", 42, module="other.module")
    with karva.raises(categories.CustomWarning):
        warnings.warn_explicit("different", categories.CustomWarning, "custom.py", 42, module="target.module")
"#);
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--status-level=none", "--max-warnings=1"])
    );
}

#[rstest]
fn strict_policy_applies_to_import_setup_and_teardown(
    #[values("import", "setup", "teardown")] phase: &str,
) {
    let body = match phase {
        "import" => "warnings.warn('strict', ResourceWarning)\ndef test_strict():\n    pass\n",
        "setup" => {
            "@karva.fixture\ndef value():\n    warnings.warn('strict', ResourceWarning)\n    return 1\ndef test_strict(value):\n    pass\n"
        }
        _ => {
            "@karva.fixture\ndef value():\n    yield 1\n    warnings.warn('strict', ResourceWarning)\ndef test_strict(value):\n    pass\n"
        }
    };
    let context =
        TestContext::with_file("test.py", &format!("import warnings\nimport karva\n{body}"));
    insta::with_settings!({snapshot_suffix => phase}, {
        assert_cmd_snapshot!(context.command_no_parallel().args(["--status-level=none", "-W", "error"]));
    });
}

#[test]
fn pytest_module_and_parameter_filters_are_local() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import warnings
import pytest

pytestmark = pytest.mark.filterwarnings("ignore:module warning")

@pytest.mark.parametrize("value", [pytest.param(1, marks=pytest.mark.filterwarnings("ignore:parameter warning")), 2])
def test_parameters(value):
    warnings.warn("module warning")
    warnings.warn("parameter warning")
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--status-level=none",
        "-W",
        "error"
    ]));
}

#[test]
fn function_filters_override_module_filters_and_outer_decorators_win() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import warnings
import pytest
import karva

pytestmark = pytest.mark.filterwarnings("error")

@pytest.mark.filterwarnings("ignore:local")
@pytest.mark.filterwarnings("error")
def test_pytest():
    warnings.warn("local")

@karva.tags.filter_warnings("ignore:local")
@karva.tags.filter_warnings("error")
def test_karva():
    warnings.warn("local")
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().args(["--status-level=none"]));
}

#[test]
fn cli_overrides_profile_warning_budget() {
    let context = TestContext::with_file(
        "test.py",
        "import warnings\ndef test_warn():\n    warnings.warn('budget warning')\n",
    );
    context.write_file("karva.toml", "[profile.default.test]\nmax-warnings = 0\n");
    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"));
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--status-level=none", "--max-warnings=1"])
    );
}
