use std::collections::{HashMap, HashSet};

use karva_cli::PartitionSelection;
use karva_python_semantic::TestCacheKey;

use super::super::{TestOrdering, partition_collected_tests};
use super::helpers::collected_package;

#[test]
fn partition_selection_filters_after_sorting_by_qualified_name() {
    let (_temp_dir, test_path, package) = collected_package(
        "def test_c(): pass\n\
         def test_a(): pass\n\
         def test_b(): pass\n",
    );
    let selection = "slice:2/3"
        .parse::<PartitionSelection>()
        .expect("valid partition selection");

    let partitions = partition_collected_tests(
        &package,
        1,
        &HashMap::new(),
        &HashSet::new(),
        Some(selection),
        TestOrdering::Stable,
    );

    assert_eq!(
        partitions[0].test_paths().collect::<Vec<_>>(),
        [format!("{test_path}::test_b")]
    );
}

#[test]
fn last_failed_filters_before_explicit_partition_selection() {
    let (_temp_dir, test_path, package) = collected_package(
        "def test_c(): pass\n\
         def test_a(): pass\n\
         def test_b(): pass\n\
         def test_d(): pass\n",
    );
    let selection = "slice:2/2"
        .parse::<PartitionSelection>()
        .expect("valid partition selection");
    let last_failed = HashSet::from([
        TestCacheKey::function_name("test_sample::test_b"),
        TestCacheKey::function_name("test_sample::test_c"),
    ]);

    let partitions = partition_collected_tests(
        &package,
        1,
        &HashMap::new(),
        &last_failed,
        Some(selection),
        TestOrdering::Stable,
    );

    assert_eq!(
        partitions[0].test_paths().collect::<Vec<_>>(),
        [format!("{test_path}::test_c")]
    );
}
