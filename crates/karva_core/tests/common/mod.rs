use karva_core::{StandardTestRunner, TestRunResult, TestRunner};
use karva_project::ProjectDatabase;
use karva_test::TestContext;

pub trait TestRunnerExt {
    fn test(&self) -> TestRunResult;
}

impl TestRunnerExt for TestContext {
    fn test(&self) -> TestRunResult {
        let db = ProjectDatabase::test_db(self.cwd(), &[self.cwd()]);
        let test_runner = StandardTestRunner::new(&db);
        test_runner.test()
    }
}

pub fn get_auto_use_kw(framework: &str) -> &str {
    match framework {
        "pytest" => "autouse",
        "karva" => "auto_use",
        _ => panic!("Invalid framework"),
    }
}

pub fn get_skip_function(framework: &str) -> &str {
    match framework {
        "pytest" => "pytest.mark.skip",
        "karva" => "karva.tags.skip",
        _ => panic!("Invalid framework"),
    }
}

pub fn get_parametrize_function(framework: &str) -> &str {
    match framework {
        "pytest" => "pytest.mark.parametrize",
        "karva" => "karva.tags.parametrize",
        _ => panic!("Invalid framework"),
    }
}
