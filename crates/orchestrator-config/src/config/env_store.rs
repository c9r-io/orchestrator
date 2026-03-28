use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for a non-sensitive environment variable store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvStoreConfig {
    /// Key-value pairs materialized into the target environment.
    pub data: HashMap<String, String>,
}

/// Configuration for a sensitive secret store.
/// All values are treated as sensitive and redacted in logs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretStoreConfig {
    /// Key-value pairs materialized into the target environment.
    /// All values are redacted in logs.
    pub data: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_store_config_serde() {
        let cfg: EnvStoreConfig = serde_json::from_str(r#"{"data":{"K":"V"}}"#).unwrap();
        assert_eq!(cfg.data.get("K").unwrap(), "V");
    }

    #[test]
    fn env_store_config_ignores_legacy_sensitive_field() {
        // Old JSON with "sensitive" field should deserialize without error
        let cfg: EnvStoreConfig =
            serde_json::from_str(r#"{"data":{"K":"V"},"sensitive":false}"#).unwrap();
        assert_eq!(cfg.data.get("K").unwrap(), "V");
    }

    #[test]
    fn secret_store_config_serde() {
        let cfg: SecretStoreConfig =
            serde_json::from_str(r#"{"data":{"SECRET":"val"}}"#).unwrap();
        assert_eq!(cfg.data.get("SECRET").unwrap(), "val");
    }

    #[test]
    fn secret_store_config_round_trip() {
        let cfg = SecretStoreConfig {
            data: [("K".to_string(), "V".to_string())].into(),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let cfg2: SecretStoreConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg2.data.get("K").unwrap(), "V");
    }
}
