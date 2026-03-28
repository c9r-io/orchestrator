#![cfg_attr(not(test), deny(clippy::panic, clippy::unwrap_used, clippy::expect_used))]

pub mod config;
pub mod connect;

pub use config::ControlPlaneConfig;
pub use connect::{MAX_GRPC_DECODE_SIZE, TransportKind, connect};
