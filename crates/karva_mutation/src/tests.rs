//! Exercises the discovery foundation before production consumers are introduced.

#[path = "data.rs"]
mod data;
#[path = "discovery.rs"]
mod discovery;

use crate::fingerprint;
use data::{Mutant, Operator};
