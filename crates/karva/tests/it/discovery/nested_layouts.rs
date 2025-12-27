use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_deeply_nested_structure() {
    let context = TestContext::with_files([
        (
            "level1/level2/level3/level4/test_deep.py",
            r"
def test_nested(): pass",
        ),
        (
            "level1/level2/test_mid.py",
            r"
def test_middle(): pass",
        ),
        (
            "test_root.py",
            r"
def test_root(): pass",
        ),
    ]);

    assert_cmd_snapshot!(context.command().arg("-q"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_mixed_nesting_siblings() {
    let context = TestContext::with_files([
        (
            "tests/unit/test_a.py",
            r"
def test_unit_a(): pass",
        ),
        (
            "tests/integration/deep/nested/test_b.py",
            r"
def test_integration_b(): pass",
        ),
        (
            "tests/e2e/test_c.py",
            r"
def test_e2e_c(): pass",
        ),
        (
            "tests/test_d.py",
            r"
def test_direct(): pass",
        ),
    ]);

    assert_cmd_snapshot!(context.command().arg("-q"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test result: ok. 4 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_folder_with_underscores_and_numbers() {
    let context = TestContext::with_files([
        (
            "test_package_v2/sub_module_123/test_feature.py",
            r"
def test_v2_feature(): pass",
        ),
        (
            "_private/test_internal.py",
            r"
def test_internal(): pass",
        ),
        (
            "package_2024/v1_0_0/test_versioned.py",
            r"
def test_versioned(): pass",
        ),
    ]);

    assert_cmd_snapshot!(context.command().arg("-q"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parallel_directory_trees() {
    let context = TestContext::with_files([
        (
            "src/a/b/c/test_path1.py",
            r"
def test_path_1(): pass",
        ),
        (
            "lib/a/b/c/test_path2.py",
            r"
def test_path_2(): pass",
        ),
        (
            "app/x/y/z/test_path3.py",
            r"
def test_path_3(): pass",
        ),
    ]);

    assert_cmd_snapshot!(context.command().arg("-q"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
