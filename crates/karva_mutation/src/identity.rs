//! Stable identities shared by discovery and source validation in workers.

use std::hash::Hasher;

use siphasher::sip128::{Hasher128, SipHasher13};

/// A stable digest of length-delimited byte strings, independent of Rust's `Hash` format.
pub fn fingerprint(parts: &[&[u8]]) -> String {
    let mut hash = SipHasher13::new();
    for part in parts {
        hash.write(part.len().to_string().as_bytes());
        hash.write(b":");
        hash.write(part);
    }
    format!("{:032x}", hash.finish128().as_u128())
}
