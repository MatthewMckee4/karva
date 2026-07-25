use insta::assert_snapshot;
use insta_cmd::assert_cmd_snapshot;
use regex::Regex;

use crate::common::TestContext;

fn normalize_junit_xml(xml: &str) -> String {
    Regex::new(r#"time="[0-9.]+""#)
        .expect("valid time regex")
        .replace_all(xml, r#"time="[TIME]""#)
        .to_string()
}

#[test]
fn writes_junit_xml_report() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.ci.junit]
path = "reports/test-results.xml"
report-name = "karva-ci"
store-success-output = true
store-failure-output = true
"#,
        ),
        (
            "test_alpha.py",
            r#"
import sys

def test_pass():
    print("pass <stdout>")
    print("pass & stderr", file=sys.stderr)

def test_fail():
    print("fail stdout")
    print("fail stderr", file=sys.stderr)
    assert False
"#,
        ),
        (
            "test_beta.py",
            r#"
import karva

@karva.tags.skip("skip & wait")
def test_skip():
    assert False
"#,
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--profile=ci", "--status-level=none"]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_alpha::test_fail:

    error[test-failure]: Test `test_fail` failed
     --> test_alpha.py:8:5
      |
    8 | def test_fail():
      |     ^^^^^^^^^
      |
    info: Test failed here
      --> test_alpha.py:11:5
       |
    11 |     assert False
       |     ^^^^^^^^^^^^
       |

    captured stdout:
    fail stdout
    captured stderr:
    fail stderr

    ────────────
         Summary [TIME] 3 tests run: 1 passed, 1 failed, 1 skipped

    ----- stderr -----
    "
    );

    let xml = normalize_junit_xml(&context.read_file("reports/test-results.xml"));
    assert_snapshot!(xml, @r#"
    <?xml version="1.0" encoding="UTF-8"?>
    <testsuites name="karva-ci" tests="3" failures="1" skipped="1" errors="0" time="[TIME]">
      <testsuite name="test_alpha" tests="2" failures="1" skipped="0" errors="0" time="[TIME]">
        <testcase classname="test_alpha" name="test_fail" time="[TIME]">
          <failure message="Test `test_fail` failed" type="test-failure">error[test-failure]: Test `test_fail` failed
     --&gt; test_alpha.py:8:5
      |
    8 | def test_fail():
      |     ^^^^^^^^^
      |
    info: Test failed here
      --&gt; test_alpha.py:11:5
       |
    11 |     assert False
       |     ^^^^^^^^^^^^
       |

    </failure>
          <system-out>fail stdout
    </system-out>
          <system-err>fail stderr
    </system-err>
        </testcase>
        <testcase classname="test_alpha" name="test_pass" time="[TIME]">
          <system-out>pass &lt;stdout&gt;
    </system-out>
          <system-err>pass &amp; stderr
    </system-err>
        </testcase>
      </testsuite>
      <testsuite name="test_beta" tests="1" failures="0" skipped="1" errors="0" time="[TIME]">
        <testcase classname="test_beta" name="test_skip" time="[TIME]">
          <skipped message="skip &amp; wait"/>
        </testcase>
      </testsuite>
    </testsuites>
    "#);
}

#[test]
fn junit_reports_final_retry_outcomes() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.ci.junit]
path = "reports/test-results.xml"
store-success-output = true
"#,
        ),
        (
            "test_retry.py",
            r#"
import os

def test_fail():
    assert False

def test_flaky():
    print(f"attempt {os.environ['KARVA_ATTEMPT']}")
    assert os.environ["KARVA_ATTEMPT"] == "2"
"#,
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--profile=ci", "--retry=1", "--status-level=none"]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_retry::test_fail:

    error[test-failure]: Test `test_fail` failed
     --> test_retry.py:4:5
      |
    4 | def test_fail():
      |     ^^^^^^^^^
      |
    info: Test failed here
     --> test_retry.py:5:5
      |
    5 |     assert False
      |     ^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 2 tests run: 1 passed (1 flaky), 1 failed, 0 skipped
       FLAKY 2/2 [TIME] test_retry::test_flaky

    ----- stderr -----
    "
    );

    let xml = normalize_junit_xml(&context.read_file("reports/test-results.xml"));
    assert_snapshot!(xml, @r#"
    <?xml version="1.0" encoding="UTF-8"?>
    <testsuites name="karva-tests" tests="2" failures="1" skipped="0" errors="0" time="[TIME]">
      <testsuite name="test_retry" tests="2" failures="1" skipped="0" errors="0" time="[TIME]">
        <testcase classname="test_retry" name="test_fail" time="[TIME]">
          <failure message="Test `test_fail` failed" type="test-failure">error[test-failure]: Test `test_fail` failed
     --&gt; test_retry.py:4:5
      |
    4 | def test_fail():
      |     ^^^^^^^^^
      |
    info: Test failed here
     --&gt; test_retry.py:5:5
      |
    5 |     assert False
      |     ^^^^^^^^^^^^
      |

    </failure>
          <rerunFailure message="Test `test_fail` failed" type="test-failure" time="[TIME]">error[test-failure]: Test `test_fail` failed
     --&gt; test_retry.py:4:5
      |
    4 | def test_fail():
      |     ^^^^^^^^^
      |
    info: Test failed here
     --&gt; test_retry.py:5:5
      |
    5 |     assert False
      |     ^^^^^^^^^^^^
      |

    </rerunFailure>
        </testcase>
        <testcase classname="test_retry" name="test_flaky" time="[TIME]">
          <flakyFailure message="Test `test_flaky` failed" type="test-failure" time="[TIME]">error[test-failure]: Test `test_flaky` failed
     --&gt; test_retry.py:7:5
      |
    7 | def test_flaky():
      |     ^^^^^^^^^^
      |
    info: Test failed here
     --&gt; test_retry.py:9:5
      |
    9 |     assert os.environ[&quot;KARVA_ATTEMPT&quot;] == &quot;2&quot;
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |

    </flakyFailure>
          <system-out>attempt 1
    attempt 2
    </system-out>
        </testcase>
      </testsuite>
    </testsuites>
    "#);
}

#[test]
fn junit_reports_run_diagnostics_as_errors() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.ci.junit]
path = "reports/test-results.xml"
"#,
        ),
        (
            "test_import.py",
            r"
import missing_junit_dependency

def test_unreachable():
    pass
",
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--profile=ci", "--status-level=none"]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test_import`: No module named 'missing_junit_dependency'

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    "
    );

    let xml = normalize_junit_xml(&context.read_file("reports/test-results.xml"));
    assert_snapshot!(xml, @r#"
    <?xml version="1.0" encoding="UTF-8"?>
    <testsuites name="karva-tests" tests="1" failures="0" skipped="0" errors="1" time="[TIME]">
      <testsuite name="karva-tests::run" tests="1" failures="0" skipped="0" errors="1" time="[TIME]">
        <testcase classname="karva.run" name="failed-to-import-module-1" time="[TIME]">
          <error message="Failed to import python module `test_import`: No module named &apos;missing_junit_dependency&apos;" type="failed-to-import-module">error[failed-to-import-module]: Failed to import python module `test_import`: No module named &apos;missing_junit_dependency&apos;

    </error>
        </testcase>
      </testsuite>
    </testsuites>
    "#);
}

#[test]
fn junit_distinguishes_fixture_errors_from_test_failures() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.ci.junit]
path = "reports/test-results.xml"
"#,
        ),
        (
            "test_fixture.py",
            r#"
import karva

@karva.fixture(auto_use=True)
def broken_fixture():
    raise RuntimeError("setup failed")

def test_unreachable():
    raise AssertionError("test body ran")
"#,
        ),
        (
            "test_failure.py",
            r"
def test_failure():
    assert False
",
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--profile=ci", "--status-level=none"]),
        @r#"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_failure::test_failure:

    error[test-failure]: Test `test_failure` failed
     --> test_failure.py:2:5
      |
    2 | def test_failure():
      |     ^^^^^^^^^^^^
      |
    info: Test failed here
     --> test_failure.py:3:5
      |
    3 |     assert False
      |     ^^^^^^^^^^^^
      |

    test_fixture::test_unreachable:

    error[fixture-failure]: Fixture `broken_fixture` failed
     --> test_fixture.py:5:5
      |
    5 | def broken_fixture():
      |     ^^^^^^^^^^^^^^
      |
    info: Fixture failed here
     --> test_fixture.py:6:5
      |
    6 |     raise RuntimeError("setup failed")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: setup failed

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 1 failed, 1 error, 0 skipped

    ----- stderr -----
    "#
    );

    let xml = normalize_junit_xml(&context.read_file("reports/test-results.xml"));
    assert_snapshot!(xml, @r#"
    <?xml version="1.0" encoding="UTF-8"?>
    <testsuites name="karva-tests" tests="2" failures="1" skipped="0" errors="1" time="[TIME]">
      <testsuite name="test_failure" tests="1" failures="1" skipped="0" errors="0" time="[TIME]">
        <testcase classname="test_failure" name="test_failure" time="[TIME]">
          <failure message="Test `test_failure` failed" type="test-failure">error[test-failure]: Test `test_failure` failed
     --&gt; test_failure.py:2:5
      |
    2 | def test_failure():
      |     ^^^^^^^^^^^^
      |
    info: Test failed here
     --&gt; test_failure.py:3:5
      |
    3 |     assert False
      |     ^^^^^^^^^^^^
      |

    </failure>
        </testcase>
      </testsuite>
      <testsuite name="test_fixture" tests="1" failures="0" skipped="0" errors="1" time="[TIME]">
        <testcase classname="test_fixture" name="test_unreachable" time="[TIME]">
          <error message="Fixture `broken_fixture` failed" type="fixture-failure">error[fixture-failure]: Fixture `broken_fixture` failed
     --&gt; test_fixture.py:5:5
      |
    5 | def broken_fixture():
      |     ^^^^^^^^^^^^^^
      |
    info: Fixture failed here
     --&gt; test_fixture.py:6:5
      |
    6 |     raise RuntimeError(&quot;setup failed&quot;)
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: setup failed

    </error>
        </testcase>
      </testsuite>
    </testsuites>
    "#);
}
