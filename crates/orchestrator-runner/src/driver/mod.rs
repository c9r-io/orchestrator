mod contracts;
mod process;
mod providers;
mod registry;

pub use contracts::{
    AgentDriver, DriverCapabilities, DriverEvent, DriverEventStream, DriverInput, DriverOutcome,
    DriverRunResult, DriverSession, DriverStartRequest, McpCallbackConfig, PermissionScope,
    SessionRef, TokenCounts,
};
pub(crate) use providers::prepare_legacy_claude_streaming_command;
pub use registry::{
    create_driver, driver_capabilities, driver_id, validate_driver_command_rules,
    validate_driver_config,
};
