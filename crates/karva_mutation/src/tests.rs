//! Exercises the discovery foundation before production consumers are introduced.

#[path = "data.rs"]
mod data;
#[path = "discovery.rs"]
mod discovery;
#[path = "identity.rs"]
mod identity;

use data::{Mutant, Operator};
use identity::fingerprint;
