pub mod orchestrator {
    tonic::include_proto!("orchestrator");
}

pub use orchestrator::orchestrator_service_client::OrchestratorServiceClient;
pub use orchestrator::orchestrator_service_server::{
    OrchestratorService, OrchestratorServiceServer,
};

// Re-export commonly used types
pub use orchestrator::*;
