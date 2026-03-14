pub(crate) mod accumulator;
mod apply;
mod dispatch;
mod finalize;
mod guard;
pub(crate) mod spill;

#[cfg(test)]
mod tests;

// Public API re-exports (consumed by scheduler.rs and loop_engine.rs)
pub use accumulator::StepExecutionAccumulator;
pub(crate) use dispatch::execute_dynamic_step_config;
pub use dispatch::{
    process_item, process_item_filtered, process_item_filtered_owned, OwnedProcessItemRequest,
    ProcessItemRequest,
};
pub use finalize::finalize_item_execution;
pub use guard::{execute_guard_step, GuardResult};
// Re-exported for tests in parent scheduler module
#[cfg(test)]
pub(crate) use spill::{spill_large_var, spill_to_file};
