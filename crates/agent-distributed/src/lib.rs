//! Authenticated peer transport, candidate discovery and distributed task routing.
#![forbid(unsafe_code)]
pub mod discovery;
pub mod dispatcher;
pub mod identity;
mod peer_service;
pub mod peers;
pub mod relay_client;
pub mod relay_transport;
pub mod secure_link;
mod task_service;
pub mod worker;
mod worker_run;
