use crate::runtime::DaemonRuntimeSnapshot;
use crate::state::InnerState;

pub fn runtime_snapshot(state: &InnerState) -> DaemonRuntimeSnapshot {
    state.daemon_runtime.snapshot()
}
