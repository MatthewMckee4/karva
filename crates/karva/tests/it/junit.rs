use insta::assert_snapshot;
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

    let output = context
        .command_no_parallel()
        .args(["--profile=ci", "--status-level=none"])
        .output()
        .expect("run karva");
    assert_eq!(output.status.code(), Some(1));

    let xml = normalize_junit_xml(&context.read_file("reports/test-results.xml"));
    assert_snapshot!(xml, @r#"
    <?xml version="1.0" encoding="UTF-8"?>
    <testsuites name="karva-ci" tests="3" failures="1" skipped="1" errors="0" time="[TIME]">
      <testsuite name="test_alpha" tests="2" failures="1" skipped="0" errors="0" time="[TIME]">
        <testcase classname="test_alpha" name="test_fail" time="[TIME]">
          <failure message="test failed"/>
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
