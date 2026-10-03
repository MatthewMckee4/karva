use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
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

    assert_cmd_snapshot!(context.command().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(context.command().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 4 tests run: 4 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(context.command().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(context.command().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn shared_ancestors_preserve_sibling_fixture_overrides() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            "from karva import fixture\n@fixture\ndef root(): return 10\n",
        ),
        (
            "tests/conftest.py",
            "from karva import fixture\n@fixture\ndef value(root): return root + 1\n",
        ),
        (
            "tests/nested/conftest.py",
            "from karva import fixture\n@fixture\ndef value(root): return root + 2\n",
        ),
        (
            "tests/test_a.py",
            "def test_value(value): assert value == 11\n",
        ),
        (
            "tests/test_b.py",
            "def test_value(value): assert value == 11\n",
        ),
        (
            "tests/nested/test_c.py",
            "def test_value(value): assert value == 12\n",
        ),
        (
            "tests/nested/test_d.py",
            "def test_value(value): assert value == 12\n",
        ),
        (
            "tests/sibling/test_e.py",
            "def test_value(value): assert value == 11\n",
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command()
            .args(["--num-workers=1", "--status-level=none"]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 5 tests run: 5 passed, 0 skipped

    ----- stderr -----
    "
    );
}
