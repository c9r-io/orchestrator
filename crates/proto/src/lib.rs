#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]

pub mod orchestrator {
    tonic::include_proto!("orchestrator");
}

pub use orchestrator::orchestrator_service_client::OrchestratorServiceClient;
pub use orchestrator::orchestrator_service_server::{
    OrchestratorService, OrchestratorServiceServer,
};

// Re-export commonly used types
pub use orchestrator::*;
