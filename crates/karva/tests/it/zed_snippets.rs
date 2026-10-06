//! Execute the exact default bodies shipped by the editor extension.

use insta_cmd::assert_cmd_snapshot;
use regex::Regex;
use serde_json::Value;

use crate::common::TestContext;

#[test]
fn zed_snippets_expand_to_runnable_karva_examples() {
    let snippets: Value =
        serde_json::from_str(include_str!("../../../../editors/zed/snippets/python.json"))
            .expect("snippet JSON");
    let placeholders = Regex::new(r"\$\{\d+:([^}]*)\}").expect("placeholder pattern");
    let mut files = Vec::new();
    for snippet in snippets.as_object().expect("snippet map").values() {
        let prefix = snippet["prefix"].as_str().expect("snippet prefix");
        let body = snippet["body"]
            .as_array()
            .expect("snippet body")
            .iter()
            .map(|line| line.as_str().expect("snippet line"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut source = placeholders.replace_all(&body, "$1").replace("$0", "");
        assert!(!source.contains("${"), "unexpanded placeholder in {prefix}");
        match prefix {
            "karva_fixture" | "karva_async_fixture" => {
                source.push_str("\n\nasync def test_fixture(example_fixture):\n    assert example_fixture == 1\n");
            }
            "karva_teardown" => {
                source.push_str("\n\ndef test_fixture(example_fixture):\n    assert example_fixture is not None\n");
            }
            "karva_auto_use" => {
                source.push_str("\n\ndef test_fixture():\n    pass\n");
            }
            _ => {}
        }
        files.push((format!("test_{prefix}.py"), source));
    }
    let context = TestContext::with_files(
        files
            .iter()
            .map(|(name, source)| (name.as_str(), source.as_str())),
    );
    assert_cmd_snapshot!(context.command_no_parallel().arg("--doctest-modules"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 11 tests across 1 worker
            PASS [TIME] test_karva_async_fixture::test_fixture(example_fixture=1)
            PASS [TIME] test_karva_auto_use::test_fixture
            PASS [TIME] test_karva_doctest::doctest:add
            PASS [TIME] test_karva_expect_fail::test_example
            PASS [TIME] test_karva_fixture::test_fixture(example_fixture=1)
            PASS [TIME] test_karva_json_snapshot::test_example
            PASS [TIME] test_karva_parametrize::test_example(one)
            PASS [TIME] test_karva_parametrize::test_example(two)
            PASS [TIME] test_karva_snapshot::test_example
            PASS [TIME] test_karva_tag::test_example
            PASS [TIME] test_karva_teardown::test_fixture(example_fixture=[1])
            PASS [TIME] test_karva_test::test_example
    ────────────
         Summary [TIME] 12 tests run: 12 passed, 0 skipped

    ----- stderr -----
    ");
}
