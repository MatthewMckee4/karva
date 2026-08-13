//! Ordering policies used before worker load balancing.

use std::collections::HashMap;
use std::hash::Hasher;

use siphasher::sip::SipHasher13;

use super::TestOrdering;
use super::collection::TestInfo;

pub(super) fn order_tests_for_partitioning(test_infos: &mut Vec<TestInfo>, ordering: TestOrdering) {
    test_infos.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));

    match ordering {
        TestOrdering::RandomizeUnmeasured => shuffle_tests_without_durations(test_infos),
        TestOrdering::SeededShuffle(seed) => {
            test_infos.sort_by_cached_key(|test| seeded_order_key(seed, &test.qualified_name));
        }
        TestOrdering::Stable => {}
    }
}

/// Assign each test a stable pseudo-random priority from run seed and identity.
pub(super) fn seeded_order_key(seed: u64, qualified_name: &str) -> u64 {
    let mut hasher = SipHasher13::new_with_keys(seed, !seed);
    hasher.write(qualified_name.as_bytes());
    hasher.finish()
}

/// Shuffle unmeasured tests while keeping parametrized function cases together.
fn shuffle_tests_without_durations(test_infos: &mut Vec<TestInfo>) {
    let mut groups: Vec<Vec<TestInfo>> = Vec::new();
    let mut group_index: HashMap<String, usize> = HashMap::new();

    for info in test_infos.drain(..) {
        if let Some(idx) = group_index.get(&info.function_root) {
            groups[*idx].push(info);
        } else {
            group_index.insert(info.function_root.clone(), groups.len());
            groups.push(vec![info]);
        }
    }

    let no_duration_groups: Vec<usize> = groups
        .iter()
        .enumerate()
        .filter(|(_, group)| group.iter().any(|test| test.duration.is_none()))
        .map(|(index, _)| index)
        .collect();

    for index in (1..no_duration_groups.len()).rev() {
        let swap_index = fastrand::usize(..=index);
        let index_a = no_duration_groups[index];
        let index_b = no_duration_groups[swap_index];
        groups.swap(index_a, index_b);
    }

    for group in groups {
        test_infos.extend(group);
    }
}
