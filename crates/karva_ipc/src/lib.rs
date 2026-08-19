//! Private, typed communication between Karva's controller and workers.
//!
//! The crate has three deliberately separate layers. [`protocol`] owns the
//! serde wire types, [`worker`] owns the worker-side buffered connection, and
//! [`controller`] owns authentication, reader threads, and event intake. The
//! public surface stays small so callers do not depend on transport details.

mod controller;
mod protocol;
mod transport;
mod worker;

pub use controller::{ControllerEvent, ControllerServer, WorkerCheckpoint, WorkerConnectionClose};
pub use protocol::{WorkerEvent, WorkerPath, WorkerSelection};
pub use transport::ControllerEndpoint;
pub use worker::WorkerClient;
