use crate::runtime::DaemonRuntimeSnapshot;
use crate::state::InnerState;

/// Returns a snapshot of daemon runtime state for diagnostics and APIs.
pub fn runtime_snapshot(state: &InnerState) -> DaemonRuntimeSnapshot {
    state.daemon_runtime.snapshot()
}
