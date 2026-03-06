use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// WP03: Action to generate new task items dynamically from a pipeline variable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenerateItemsAction {
    /// Pipeline variable name containing the JSON data.
    pub from_var: String,
    /// JSON path to the array of candidates within the variable value.
    pub json_path: String,
    /// How to map each array element to a task item.
    pub mapping: DynamicItemMapping,
    /// Whether to replace existing items (default: false, meaning append).
    #[serde(default)]
    pub replace: bool,
}

/// Mapping from a JSON array element to task item fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DynamicItemMapping {
    /// JSON path to the item ID field within each element.
    pub item_id: String,
    /// Optional JSON path to a label field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Per-item variables: key → JSON path within element.
    #[serde(default)]
    pub vars: HashMap<String, String>,
}

/// A dynamically generated task item, ready for DB insertion.
#[derive(Debug, Clone)]
pub struct NewDynamicItem {
    pub item_id: String,
    pub label: Option<String>,
    pub vars: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_items_action_minimal() {
        let json = r#"{
            "from_var": "candidates",
            "json_path": "$.items",
            "mapping": {"item_id": "$.id"}
        }"#;
        let action: GenerateItemsAction =
            serde_json::from_str(json).expect("deserialize generate items");
        assert_eq!(action.from_var, "candidates");
        assert!(!action.replace);
        assert!(action.mapping.label.is_none());
        assert!(action.mapping.vars.is_empty());
    }

    #[test]
    fn test_generate_items_action_full() {
        let json = r#"{
            "from_var": "candidates",
            "json_path": "$.items",
            "mapping": {
                "item_id": "$.id",
                "label": "$.name",
                "vars": {
                    "approach": "$.approach",
                    "config": "$.config_path"
                }
            },
            "replace": true
        }"#;
        let action: GenerateItemsAction =
            serde_json::from_str(json).expect("deserialize full generate items");
        assert!(action.replace);
        assert_eq!(action.mapping.label, Some("$.name".to_string()));
        assert_eq!(action.mapping.vars.len(), 2);
    }
}
