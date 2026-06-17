use std::fmt;
use std::num::NonZeroU32;
use std::str::FromStr;

/// Selection of a single partition (slice) from the collected tests.
///
/// Used by `--partition slice:M/N` to run only the tests assigned to slice
/// `M` of `N`. Slice indices are 1-indexed: `slice:1/3`, `slice:2/3`,
/// `slice:3/3` together cover every collected test exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartitionSelection {
    index: NonZeroU32,
    total: NonZeroU32,
}

impl PartitionSelection {
    #[must_use]
    pub fn new(index: NonZeroU32, total: NonZeroU32) -> Option<Self> {
        if index <= total {
            Some(Self { index, total })
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

    /// Returns true if the test at `position` (0-indexed, in the deterministic
    /// post-filter ordering) belongs to this slice.
    #[must_use]
    pub fn contains(self, position: usize) -> bool {
        // 1-indexed input -> 0-indexed modulo target.
        let target = (self.index.get() - 1) as usize;
        let total = self.total.get() as usize;
        position % total == target
    }
}

impl fmt::Display for PartitionSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "slice:{}/{}", self.index, self.total)
    }
}

impl FromStr for PartitionSelection {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let (kind, body) = raw.split_once(':').ok_or_else(|| {
            format!("expected `<strategy>:<M>/<N>` (e.g. `slice:1/3`), got `{raw}`")
        })?;

        if kind != "slice" {
            return Err(format!(
                "unknown partition strategy `{kind}`; supported strategies: `slice`"
            ));
        }

        let (m, n) = body
            .split_once('/')
            .ok_or_else(|| format!("expected `slice:<M>/<N>`, got `slice:{body}`"))?;

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

        Ok(Self { index, total })
    }
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
        assert!("hash:1/3".parse::<PartitionSelection>().is_err());
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
    fn display_round_trip() {
        let p = selection(2, 5);
        assert_eq!(p.to_string(), "slice:2/5");
        assert_eq!(p.to_string().parse::<PartitionSelection>().unwrap(), p);
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
}
