use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for an environment variable store.
/// Used by both EnvStore (sensitive=false) and SecretStore (sensitive=true) resources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvStoreConfig {
    pub data: HashMap<String, String>,
    /// When true, values from this store are redacted in logs.
    #[serde(default)]
    pub sensitive: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_store_config_default_not_sensitive() {
        let cfg: EnvStoreConfig = serde_json::from_str(r#"{"data":{"K":"V"}}"#).unwrap();
        assert_eq!(cfg.data.get("K").unwrap(), "V");
        assert!(!cfg.sensitive);
    }

    #[test]
    fn env_store_config_sensitive() {
        let cfg: EnvStoreConfig =
            serde_json::from_str(r#"{"data":{"SECRET":"val"},"sensitive":true}"#).unwrap();
        assert!(cfg.sensitive);
    }
}
