use serde::{Deserialize, Serialize};

/// Configuration for a store backend provider.
///
/// Built-in providers (local, file) are handled natively by the engine.
/// User-defined providers delegate CRUD operations to shell commands.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoreBackendProviderConfig {
    /// When true, the engine handles this provider natively (e.g. local=SQLite, file=filesystem).
    #[serde(default)]
    pub builtin: bool,

    /// Shell commands implementing the CRUD protocol. Required when `builtin` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<StoreBackendCommands>,
}

/// Shell commands for a custom store backend provider.
///
/// Each command is a shell template that receives context via environment variables:
/// STORE_NAME, PROJECT_ID, KEY, VALUE, TASK_ID, LIMIT, OFFSET, MAX_ENTRIES, TTL_DAYS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreBackendCommands {
    pub get: String,
    pub put: String,
    pub delete: String,
    pub list: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prune: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_not_builtin() {
        let config = StoreBackendProviderConfig::default();
        assert!(!config.builtin);
        assert!(config.commands.is_none());
    }

    #[test]
    fn serde_round_trip_builtin() {
        let config = StoreBackendProviderConfig {
            builtin: true,
            commands: None,
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let back: StoreBackendProviderConfig = serde_json::from_str(&json).expect("deserialize");
        assert!(back.builtin);
        assert!(back.commands.is_none());
    }

    #[test]
    fn serde_round_trip_custom() {
        let config = StoreBackendProviderConfig {
            builtin: false,
            commands: Some(StoreBackendCommands {
                get: "redis-cli GET $KEY".to_string(),
                put: "redis-cli SET $KEY $VALUE".to_string(),
                delete: "redis-cli DEL $KEY".to_string(),
                list: "redis-cli KEYS *".to_string(),
                prune: Some("scripts/prune.sh".to_string()),
            }),
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let back: StoreBackendProviderConfig = serde_json::from_str(&json).expect("deserialize");
        assert!(!back.builtin);
        let cmds = back.commands.expect("commands");
        assert_eq!(cmds.get, "redis-cli GET $KEY");
        assert!(cmds.prune.is_some());
    }
}
