use std::collections::HashMap;

use karva_core::{discovery::Discoverer, package::Package};
use karva_project::{path::SystemPathBuf, project::Project, tests::TestEnv};

fn get_sorted_test_strings(discovered_tests: &HashMap<SystemPathBuf, Package>) -> Vec<String> {
    let mut test_strings = discovered_tests
        .values()
        .flat_map(|package| package.test_cases())
        .map(ToString::to_string)
        .collect::<Vec<String>>();
    test_strings.sort();
    test_strings
}

fn main() {
    let env = TestEnv::new();
    let path = env.create_dir("tests");
    env.create_dir("tests/nested");
    env.create_dir("tests/nested/deeper");

    env.create_file("tests/test_file1.py", "def test_function1(): pass");
    env.create_file("tests/nested/test_file2.py", "def test_function2(): pass");
    env.create_file(
        "tests/nested/deeper/test_file3.py",
        "def test_function3(): pass",
    );

    let project = Project::new(env.cwd(), vec![path]);
    let discoverer = Discoverer::new(&project);
    let discovered_tests = discoverer.discover();
    println!("discovered_tests: {:#?}", discovered_tests);
    println!("{:#?}", get_sorted_test_strings(&discovered_tests.packages));
}
