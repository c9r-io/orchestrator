mod config;
mod steps;
#[cfg(test)]
mod tests;
mod workflow;

pub use workflow::normalize_workflow_config;

pub(crate) use config::normalize_config;
pub(crate) use steps::normalize_step_execution_mode_recursive;
