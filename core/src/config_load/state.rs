use crate::config::ActiveConfig;
use anyhow::Result;

pub fn read_active_config<'a>(
    state: &'a crate::state::InnerState,
) -> Result<std::sync::RwLockReadGuard<'a, ActiveConfig>> {
    let active_config_error = state
        .active_config_error
        .read()
        .map_err(|_| anyhow::anyhow!("active config error lock is poisoned"))?
        .clone();
    if let Some(message) = active_config_error {
        anyhow::bail!(message);
    }
    read_loaded_config(state)
}

pub fn read_loaded_config<'a>(
    state: &'a crate::state::InnerState,
) -> Result<std::sync::RwLockReadGuard<'a, ActiveConfig>> {
    state
        .active_config
        .read()
        .map_err(|_| anyhow::anyhow!("active config lock is poisoned"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_active_config_rejects_non_runnable_state() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();
        *state
            .active_config_error
            .write()
            .expect("active_config_error lock should be writable") =
            Some("active config is not runnable".to_string());

        let result = read_active_config(&state);
        assert!(result.is_err());
        assert!(result.expect_err("operation should fail").to_string().contains("not runnable"));
    }
}
