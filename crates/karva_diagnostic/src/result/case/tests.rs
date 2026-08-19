use std::time::Duration;

use karva_python_semantic::{ModulePath, QualifiedFunctionName, QualifiedTestName};
use serde_json::json;

use super::*;

#[test]
fn test_case_result_uses_structured_parameterized_name() {
    let name = QualifiedTestName::with_parameters(
        QualifiedFunctionName::new(
            "test_example".to_string(),
            ModulePath::new_with_name("test.py", "tests.test".to_string()),
        ),
        "value=1".to_string(),
    );

    let result = TestCaseResult::<()>::new(&name, TestCaseOutcome::Passed, Duration::ZERO, None);

    assert_eq!(result.module_name(), "tests.test");
    assert_eq!(result.name(), "test_example(value=1)");
    assert_eq!(result.full_name(), "tests.test::test_example(value=1)");
}

#[test]
fn test_case_result_keeps_its_flat_serialized_shape() {
    let name = QualifiedTestName::new(QualifiedFunctionName::new(
        "test_example".to_string(),
        ModulePath::new_with_name("test.py", "tests.test".to_string()),
    ));
    let result = TestCaseResult::<()>::new(
        &name,
        TestCaseOutcome::Passed,
        Duration::from_millis(2),
        None,
    );

    let value = serde_json::to_value(&result).expect("serialize result");

    assert_eq!(value["module_name"], json!("tests.test"));
    assert_eq!(value["name"], json!("test_example"));
    assert_eq!(value["full_name"], json!("tests.test::test_example"));
    assert_eq!(value["outcome"], json!("passed"));
    assert!(value.get("identity").is_none());
    assert!(value.get("payload").is_none());
    assert_eq!(
        serde_json::from_value::<TestCaseResult<()>>(value).expect("deserialize result"),
        result
    );
}

#[test]
fn test_case_result_deserializes_without_derived_identity_fields() {
    let name = QualifiedTestName::new(QualifiedFunctionName::new(
        "test_example".to_string(),
        ModulePath::new_with_name("test.py", "tests.test".to_string()),
    ));
    let result = TestCaseResult::<()>::new(
        &name,
        TestCaseOutcome::Passed,
        Duration::from_millis(2),
        None,
    );
    let mut value = serde_json::to_value(&result).expect("serialize result");
    let object = value.as_object_mut().expect("result should be an object");
    object.remove("module_name");
    object.remove("name");

    assert_eq!(
        serde_json::from_value::<TestCaseResult<()>>(value).expect("deserialize result"),
        result
    );
}

#[test]
fn test_case_result_round_trips_retry_payload() {
    let name = QualifiedTestName::new(QualifiedFunctionName::new(
        "test_example".to_string(),
        ModulePath::new_with_name("test.py", "tests.test".to_string()),
    ));
    let result = TestCaseResult::retried(
        &name,
        TestCaseOutcome::Failed {
            diagnostic: (),
            related: vec![()],
        },
        Duration::from_millis(3),
        TestCaseRetry::new(2, 2).with_failure_policy(true, true),
        Some(CapturedTestOutput::new(
            "final stdout".to_string(),
            "final stderr".to_string(),
        )),
        vec![TestCaseAttempt::new(
            1,
            TestCaseOutcome::Passed,
            Duration::from_millis(1),
            Some(CapturedTestOutput::new(
                "first stdout".to_string(),
                String::new(),
            )),
        )],
    );

    let value = serde_json::to_value(&result).expect("serialize retry result");

    assert!(value.get("retry").is_some());
    assert!(value.get("captured_output").is_some());
    assert!(value.get("attempts").is_some());
    assert_eq!(
        serde_json::from_value::<TestCaseResult<()>>(value).expect("deserialize retry result"),
        result
    );
}
