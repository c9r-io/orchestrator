use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// WP03: Configuration for the item_select builtin step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemSelectConfig {
    /// Selection strategy.
    pub strategy: SelectionStrategy,
    /// Pipeline variable name containing the metric to evaluate (for min/max/threshold).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_var: Option<String>,
    /// Weight map for weighted strategy: var_name → weight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights: Option<HashMap<String, f64>>,
    /// Threshold value (for threshold strategy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    /// Where to persist the selection result in the workflow store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_result: Option<StoreTarget>,
    /// How to break ties (default: first).
    #[serde(default)]
    pub tie_break: TieBreak,
}

/// Selection strategy for item_select.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelectionStrategy {
    Min,
    Max,
    Threshold,
    Weighted,
}

/// Where to store the selection result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreTarget {
    pub namespace: String,
    pub key: String,
}

/// Tie-breaking strategy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TieBreak {
    #[default]
    First,
    Last,
    Random,
}

/// Result of a selection operation.
#[derive(Debug, Clone)]
pub struct SelectionResult {
    pub winner_id: String,
    pub eliminated_ids: Vec<String>,
    pub winner_vars: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_select_config_min() {
        let json = r#"{
            "strategy": "min",
            "metric_var": "error_count"
        }"#;
        let cfg: ItemSelectConfig =
            serde_json::from_str(json).expect("deserialize item select config");
        assert_eq!(cfg.strategy, SelectionStrategy::Min);
        assert_eq!(cfg.metric_var, Some("error_count".to_string()));
        assert_eq!(cfg.tie_break, TieBreak::First);
    }

    #[test]
    fn test_item_select_config_weighted() {
        let json = r#"{
            "strategy": "weighted",
            "weights": {"quality": 0.7, "speed": 0.3},
            "tie_break": "random",
            "store_result": {"namespace": "results", "key": "winner"}
        }"#;
        let cfg: ItemSelectConfig =
            serde_json::from_str(json).expect("deserialize weighted config");
        assert_eq!(cfg.strategy, SelectionStrategy::Weighted);
        assert_eq!(cfg.tie_break, TieBreak::Random);
        assert!(cfg.weights.is_some());
        assert!(cfg.store_result.is_some());
    }

    #[test]
    fn test_selection_strategy_serde() {
        for s in &["\"min\"", "\"max\"", "\"threshold\"", "\"weighted\""] {
            let strategy: SelectionStrategy =
                serde_json::from_str(s).expect("deserialize strategy");
            let json = serde_json::to_string(&strategy).expect("serialize strategy");
            assert_eq!(&json, s);
        }
    }

    #[test]
    fn test_tie_break_default() {
        let tb = TieBreak::default();
        assert_eq!(tb, TieBreak::First);
    }
}
