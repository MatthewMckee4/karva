use std::fmt;
use std::num::NonZeroU32;
use std::str::FromStr;

/// Selection of a single partition from the collected tests.
///
/// Used by `--partition <strategy>:M/N` to run only the tests assigned to
/// partition `M` of `N`. Partition indices are 1-indexed: `slice:1/3`,
/// `slice:2/3`, `slice:3/3` together cover every collected test exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartitionSelection {
    strategy: PartitionStrategy,
    index: NonZeroU32,
    total: NonZeroU32,
}

impl PartitionSelection {
    #[must_use]
    pub fn new(index: NonZeroU32, total: NonZeroU32) -> Option<Self> {
        Self::with_strategy(PartitionStrategy::Slice, index, total)
    }

    fn with_strategy(
        strategy: PartitionStrategy,
        index: NonZeroU32,
        total: NonZeroU32,
    ) -> Option<Self> {
        if index <= total {
            Some(Self {
                strategy,
                index,
                total,
            })
        } else {
            None
        }
    }

    #[must_use]
    pub fn index(self) -> NonZeroU32 {
        self.index
    }

    #[must_use]
    pub fn total(self) -> NonZeroU32 {
        self.total
    }

    /// Returns true if the zero-based test position belongs to this slice.
    #[must_use]
    pub fn contains(self, position: usize) -> bool {
        self.contains_position(position)
    }

    #[must_use]
    pub fn contains_test(self, position: usize, qualified_name: &str) -> bool {
        match self.strategy {
            PartitionStrategy::Slice => self.contains_position(position),
            PartitionStrategy::Hash => self.contains_hash(qualified_name),
        }
    }

    fn contains_position(self, position: usize) -> bool {
        // 1-indexed input -> 0-indexed modulo target.
        let target = (self.index.get() - 1) as usize;
        let total = self.total.get() as usize;
        position % total == target
    }

    fn contains_hash(self, qualified_name: &str) -> bool {
        let target = u64::from(self.index.get() - 1);
        let total = u64::from(self.total.get());
        stable_hash(qualified_name.as_bytes()) % total == target
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartitionStrategy {
    Slice,
    Hash,
}

impl PartitionStrategy {
    fn as_str(self) -> &'static str {
        match self {
            Self::Slice => "slice",
            Self::Hash => "hash",
        }
    }
}

impl FromStr for PartitionStrategy {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "slice" => Ok(Self::Slice),
            "hash" => Ok(Self::Hash),
            _ => Err(format!(
                "unknown partition strategy `{raw}`; supported strategies: `slice`, `hash`"
            )),
        }
    }
}

impl fmt::Display for PartitionSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}/{}",
            self.strategy.as_str(),
            self.index,
            self.total
        )
    }
}

impl FromStr for PartitionSelection {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let (kind, body) = raw.split_once(':').ok_or_else(|| {
            format!("expected `<strategy>:<M>/<N>` (e.g. `slice:1/3`), got `{raw}`")
        })?;

        let strategy = kind.parse::<PartitionStrategy>()?;

        let (m, n) = body
            .split_once('/')
            .ok_or_else(|| format!("expected `{kind}:<M>/<N>`, got `{kind}:{body}`"))?;

        let index: u32 = m
            .parse()
            .map_err(|err| format!("`{m}` is not a valid partition index: {err}"))?;
        let total: u32 = n
            .parse()
            .map_err(|err| format!("`{n}` is not a valid partition count: {err}"))?;

        let Some(index) = NonZeroU32::new(index) else {
            return Err("partition index `M` must be at least 1".to_string());
        };
        let Some(total) = NonZeroU32::new(total) else {
            return Err("partition count `N` must be at least 1".to_string());
        };

        if index > total {
            return Err(format!(
                "partition index `M` ({index}) must not exceed partition count `N` ({total})"
            ));
        }

        Ok(Self {
            strategy,
            index,
            total,
        })
    }
}

fn stable_hash(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 14_695_981_039_346_656_037;
    const FNV_PRIME: u64 = 1_099_511_628_211;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_slice() {
        assert_eq!(
            "slice:1/3".parse::<PartitionSelection>().unwrap(),
            selection(1, 3),
        );
        assert_eq!(
            "slice:3/3".parse::<PartitionSelection>().unwrap(),
            selection(3, 3),
        );
        assert_eq!(
            "slice:1/1".parse::<PartitionSelection>().unwrap(),
            selection(1, 1),
        );
    }

    #[test]
    fn parses_valid_hash() {
        assert_eq!(
            "hash:1/3".parse::<PartitionSelection>().unwrap(),
            hash_selection(1, 3),
        );
        assert_eq!(
            "hash:3/3".parse::<PartitionSelection>().unwrap(),
            hash_selection(3, 3),
        );
    }

    #[test]
    fn rejects_zero_total() {
        assert!("slice:1/0".parse::<PartitionSelection>().is_err());
    }

    #[test]
    fn rejects_zero_index() {
        assert!("slice:0/3".parse::<PartitionSelection>().is_err());
    }

    #[test]
    fn rejects_index_above_total() {
        assert!("slice:4/3".parse::<PartitionSelection>().is_err());
    }

    #[test]
    fn rejects_unknown_strategy() {
        assert!("random:1/3".parse::<PartitionSelection>().is_err());
    }

    #[test]
    fn rejects_missing_separators() {
        assert!("slice".parse::<PartitionSelection>().is_err());
        assert!("slice:13".parse::<PartitionSelection>().is_err());
        assert!("1/3".parse::<PartitionSelection>().is_err());
    }

    #[test]
    fn contains_round_robin() {
        let p = selection(1, 3);
        assert!(p.contains(0));
        assert!(!p.contains(1));
        assert!(!p.contains(2));
        assert!(p.contains(3));

        let q = selection(3, 3);
        assert!(!q.contains(0));
        assert!(!q.contains(1));
        assert!(q.contains(2));
        assert!(q.contains(5));
    }

    #[test]
    fn contains_hash_bucket() {
        let p = hash_selection(1, 3);
        let q = hash_selection(2, 3);
        let r = hash_selection(3, 3);

        for name in [
            "test_module::test_a",
            "test_module::test_b",
            "test_module::test_c",
            "test_module::test_d",
        ] {
            let matches = [
                p.contains_test(0, name),
                q.contains_test(0, name),
                r.contains_test(0, name),
            ];
            assert_eq!(
                matches.into_iter().filter(|matched| *matched).count(),
                1,
                "`{name}` should belong to exactly one hash partition",
            );
        }
    }

    #[test]
    fn hash_partition_is_independent_of_position() {
        let p = hash_selection(2, 3);

        assert_eq!(
            p.contains_test(0, "test_module::test_a"),
            p.contains_test(42, "test_module::test_a"),
        );
    }

    #[test]
    fn display_round_trip() {
        let p = selection(2, 5);
        assert_eq!(p.to_string(), "slice:2/5");
        assert_eq!(p.to_string().parse::<PartitionSelection>().unwrap(), p);

        let q = hash_selection(2, 5);
        assert_eq!(q.to_string(), "hash:2/5");
        assert_eq!(q.to_string().parse::<PartitionSelection>().unwrap(), q);
    }

    #[test]
    fn constructor_rejects_index_above_total() {
        let Some(index) = NonZeroU32::new(3) else {
            panic!("test constant should be non-zero");
        };
        let Some(total) = NonZeroU32::new(2) else {
            panic!("test constant should be non-zero");
        };

        assert_eq!(PartitionSelection::new(index, total), None);
    }

    fn selection(index: u32, total: u32) -> PartitionSelection {
        let Some(index) = NonZeroU32::new(index) else {
            panic!("test partition index should be non-zero");
        };
        let Some(total) = NonZeroU32::new(total) else {
            panic!("test partition total should be non-zero");
        };
        PartitionSelection::new(index, total).expect("valid partition selection")
    }

    fn hash_selection(index: u32, total: u32) -> PartitionSelection {
        let Some(index) = NonZeroU32::new(index) else {
            panic!("test partition index should be non-zero");
        };
        let Some(total) = NonZeroU32::new(total) else {
            panic!("test partition total should be non-zero");
        };
        PartitionSelection::with_strategy(PartitionStrategy::Hash, index, total)
            .expect("valid partition selection")
    }
}
