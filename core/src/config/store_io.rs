use serde::{Deserialize, Serialize};

/// Configuration for reading a value from a workflow store before step execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreInputConfig {
    /// Store name (WorkflowStore resource)
    pub store: String,
    /// Key to read from the store
    pub key: String,
    /// Pipeline variable name to inject the value into
    pub as_var: String,
    /// If true, a missing key causes the step to fail
    #[serde(default)]
    pub required: bool,
}

/// Configuration for writing a pipeline variable to a workflow store after step execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreOutputConfig {
    /// Store name (WorkflowStore resource)
    pub store: String,
    /// Key to write in the store
    pub key: String,
    /// Pipeline variable to read the value from
    pub from_var: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_input_config_serde_round_trip() {
        let cfg = StoreInputConfig {
            store: "metrics".to_string(),
            key: "baseline".to_string(),
            as_var: "baseline_metrics".to_string(),
            required: true,
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: StoreInputConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.store, "metrics");
        assert_eq!(back.key, "baseline");
        assert_eq!(back.as_var, "baseline_metrics");
        assert!(back.required);
    }

    #[test]
    fn store_input_config_required_defaults_false() {
        let json = r#"{"store":"s","key":"k","as_var":"v"}"#;
        let cfg: StoreInputConfig = serde_json::from_str(json).expect("deserialize");
        assert!(!cfg.required);
    }

    #[test]
    fn store_output_config_serde_round_trip() {
        let cfg = StoreOutputConfig {
            store: "results".to_string(),
            key: "final_score".to_string(),
            from_var: "score".to_string(),
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: StoreOutputConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.store, "results");
        assert_eq!(back.key, "final_score");
        assert_eq!(back.from_var, "score");
    }
}
