pub(crate) mod anomaly;
mod builder;
mod model;
mod render;
pub(crate) mod time;

#[cfg(test)]
mod tests;

// Public API re-exports
pub use anomaly::find_template_vars;
pub use builder::{build_trace, build_trace_with_meta};
pub use model::{CycleTrace, StepTrace, TaskTrace, TraceTaskMeta, TraceSummary};
pub use render::render_trace_terminal;
