#![warn(
    clippy::disallowed_methods,
    reason = "Prefer System trait methods over std methods in ty crates"
)]

use std::{collections::HashMap, hash::BuildHasher};

use ruff_python_ast::PythonVersion;

/// Combine two values, preferring the values in `self`.
pub trait Combine {
    #[must_use]
    fn combine(mut self, other: Self) -> Self
    where
        Self: Sized,
    {
        self.combine_with(other);
        self
    }

    fn combine_with(&mut self, other: Self);
}

impl<T> Combine for Option<T>
where
    T: Combine,
{
    fn combine(self, other: Self) -> Self
    where
        Self: Sized,
    {
        match (self, other) {
            (Some(a), Some(b)) => Some(a.combine(b)),
            (None, Some(b)) => Some(b),
            (a, _) => a,
        }
    }

    fn combine_with(&mut self, other: Self) {
        match (self, other) {
            (Some(a), Some(b)) => {
                a.combine_with(b);
            }
            (a @ None, Some(b)) => {
                *a = Some(b);
            }
            _ => {}
        }
    }
}

impl<T> Combine for Vec<T> {
    fn combine_with(&mut self, mut other: Self) {
        // `self` takes precedence over `other` but values with higher precedence must be placed after.
        // Swap the vectors so that `other` is the one that gets extended, so that the values of `self` come after.
        std::mem::swap(self, &mut other);
        self.extend(other);
    }
}

impl<K, V, S> Combine for HashMap<K, V, S>
where
    K: Eq + std::hash::Hash,
    S: BuildHasher,
{
    fn combine_with(&mut self, mut other: Self) {
        // `self` takes precedence over `other` but `extend` overrides existing values.
        // Swap the hash maps so that `self` is the one that gets extended.
        std::mem::swap(self, &mut other);
        self.extend(other);
    }
}

/// Implements [`Combine`] for a value that always returns `self` when combined with another value.
macro_rules! impl_noop_combine {
    ($name:ident) => {
        impl Combine for $name {
            #[inline(always)]
            fn combine_with(&mut self, _other: Self) {}

            #[inline(always)]
            fn combine(self, _other: Self) -> Self {
                self
            }
        }
    };
}

impl_noop_combine!(PythonVersion);

// std types
impl_noop_combine!(bool);
impl_noop_combine!(usize);
impl_noop_combine!(u8);
impl_noop_combine!(u16);
impl_noop_combine!(u32);
impl_noop_combine!(u64);
impl_noop_combine!(u128);
impl_noop_combine!(isize);
impl_noop_combine!(i8);
impl_noop_combine!(i16);
impl_noop_combine!(i32);
impl_noop_combine!(i64);
impl_noop_combine!(i128);
impl_noop_combine!(String);

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::Combine;

    #[test]
    fn combine_option() {
        assert_eq!(Some(1).combine(Some(2)), Some(1));
        assert_eq!(None.combine(Some(2)), Some(2));
        assert_eq!(Some(1).combine(None), Some(1));
    }

    #[test]
    fn combine_vec() {
        assert_eq!(None.combine(Some(vec![1, 2, 3])), Some(vec![1, 2, 3]));
        assert_eq!(Some(vec![1, 2, 3]).combine(None), Some(vec![1, 2, 3]));
        assert_eq!(
            Some(vec![1, 2, 3]).combine(Some(vec![4, 5, 6])),
            Some(vec![4, 5, 6, 1, 2, 3])
        );
    }

    #[test]
    fn combine_map() {
        let a: HashMap<u32, _> = HashMap::from_iter([(1, "a"), (2, "a"), (3, "a")]);
        let b: HashMap<u32, _> = HashMap::from_iter([(0, "b"), (2, "b"), (5, "b")]);

        assert_eq!(None.combine(Some(b.clone())), Some(b.clone()));
        assert_eq!(Some(a.clone()).combine(None), Some(a.clone()));
        assert_eq!(
            Some(a).combine(Some(b)),
            Some(HashMap::from_iter([
                (0, "b"),
                // The value from `a` takes precedence
                (1, "a"),
                (2, "a"),
                (3, "a"),
                (5, "b")
            ]))
        );
    }
}
