use crate::config::ActiveConfig;
use anyhow::Result;
use std::sync::Arc;

/// Returns the active config snapshot, failing if the snapshot is marked unrunnable.
pub fn read_active_config(state: &crate::state::InnerState) -> Result<Arc<ActiveConfig>> {
    let snapshot = crate::state::config_runtime_snapshot(state);
    if let Some(message) = snapshot.active_config_error.clone() {
        anyhow::bail!(message);
    }
    Ok(Arc::clone(&snapshot.active_config))
}

/// Returns the loaded config snapshot without checking runnable state.
pub fn read_loaded_config(state: &crate::state::InnerState) -> Result<Arc<ActiveConfig>> {
    Ok(Arc::clone(
        &crate::state::config_runtime_snapshot(state).active_config,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_active_config_rejects_non_runnable_state() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();
        crate::state::replace_active_config_status(
            &state,
            Some("active config is not runnable".to_string()),
            None,
        );

        let result = read_active_config(&state);
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("not runnable"));
    }
}
