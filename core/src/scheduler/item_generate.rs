use crate::config::{GenerateItemsAction, NewDynamicItem};
use crate::json_extract::{extract_field, extract_json_array};
use crate::state::InnerState;
use anyhow::{Context, Result};
use rusqlite::params;
use std::collections::HashMap;
use tracing::info;
use uuid::Uuid;

/// Extract dynamic items from a pipeline variable using the action's mapping.
pub fn extract_dynamic_items(
    pipeline_vars: &HashMap<String, String>,
    action: &GenerateItemsAction,
) -> Result<Vec<NewDynamicItem>> {
    let json_str = pipeline_vars.get(&action.from_var).with_context(|| {
        format!(
            "pipeline variable '{}' not found for generate_items",
            action.from_var
        )
    })?;

    let items = extract_json_array(json_str, &action.json_path)?;
    let mut result = Vec::new();

    for item_value in &items {
        let item_id = match extract_field(item_value, &action.mapping.item_id) {
            Some(id) => id,
            None => continue,
        };

        let label = action
            .mapping
            .label
            .as_ref()
            .and_then(|path| extract_field(item_value, path));

        let mut vars = HashMap::new();
        for (var_name, json_path) in &action.mapping.vars {
            if let Some(val) = extract_field(item_value, json_path) {
                vars.insert(var_name.clone(), val);
            }
        }

        result.push(NewDynamicItem {
            item_id,
            label,
            vars,
        });
    }

    Ok(result)
}

/// Insert dynamic items into the database for a given task.
pub fn create_dynamic_task_items(
    conn: &rusqlite::Connection,
    task_id: &str,
    items: &[NewDynamicItem],
    replace: bool,
) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();

    if replace {
        // Remove existing non-static items
        conn.execute(
            "DELETE FROM task_items WHERE task_id = ?1 AND source = 'dynamic'",
            params![task_id],
        )?;
    }

    // Get the current max order_no
    let max_order: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(order_no), 0) FROM task_items WHERE task_id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let mut created = 0;
    for (idx, item) in items.iter().enumerate() {
        let id = Uuid::new_v4().to_string();
        let order_no = max_order + (idx as i64) + 1;
        let dynamic_vars_json = if item.vars.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&item.vars)?)
        };

        conn.execute(
            "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, created_at, updated_at, dynamic_vars_json, label, source) VALUES (?1, ?2, ?3, ?4, 'pending', '[]', '[]', 0, 0, '', NULL, NULL, ?5, ?5, ?6, ?7, 'dynamic')",
            params![
                id,
                task_id,
                order_no,
                item.item_id,
                now,
                dynamic_vars_json,
                item.label,
            ],
        )?;
        created += 1;
    }

    info!(
        task_id = task_id,
        count = created,
        "created dynamic task items"
    );
    Ok(created)
}

/// Async wrapper for `create_dynamic_task_items` that uses the async database writer.
pub async fn create_dynamic_task_items_async(
    state: &InnerState,
    task_id: &str,
    items: &[NewDynamicItem],
    replace: bool,
) -> Result<usize> {
    let task_id = task_id.to_string();
    let items = items.to_vec();
    state
        .async_database
        .writer()
        .call(move |conn| {
            create_dynamic_task_items(conn, &task_id, &items, replace)
                .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
        })
        .await
        .map_err(|e| anyhow::anyhow!("async db error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DynamicItemMapping, GenerateItemsAction};

    #[test]
    fn test_extract_dynamic_items() {
        let mut vars = HashMap::new();
        vars.insert(
            "candidates".to_string(),
            r#"{"items": [
                {"id": "approach_a", "name": "Approach A", "config": "/a.yaml"},
                {"id": "approach_b", "name": "Approach B", "config": "/b.yaml"}
            ]}"#
            .to_string(),
        );

        let action = GenerateItemsAction {
            from_var: "candidates".to_string(),
            json_path: "$.items".to_string(),
            mapping: DynamicItemMapping {
                item_id: "$.id".to_string(),
                label: Some("$.name".to_string()),
                vars: {
                    let mut m = HashMap::new();
                    m.insert("config".to_string(), "$.config".to_string());
                    m
                },
            },
            replace: false,
        };

        let items = extract_dynamic_items(&vars, &action).expect("extract items");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].item_id, "approach_a");
        assert_eq!(items[0].label, Some("Approach A".to_string()));
        assert_eq!(items[0].vars.get("config"), Some(&"/a.yaml".to_string()));
        assert_eq!(items[1].item_id, "approach_b");
    }

    #[test]
    fn test_extract_dynamic_items_missing_var() {
        let vars = HashMap::new();
        let action = GenerateItemsAction {
            from_var: "missing".to_string(),
            json_path: "$.items".to_string(),
            mapping: DynamicItemMapping {
                item_id: "$.id".to_string(),
                label: None,
                vars: HashMap::new(),
            },
            replace: false,
        };
        assert!(extract_dynamic_items(&vars, &action).is_err());
    }

    #[test]
    fn test_extract_dynamic_items_skips_missing_id() {
        let mut vars = HashMap::new();
        vars.insert(
            "data".to_string(),
            r#"{"items": [{"name": "no_id"}, {"id": "has_id"}]}"#.to_string(),
        );

        let action = GenerateItemsAction {
            from_var: "data".to_string(),
            json_path: "$.items".to_string(),
            mapping: DynamicItemMapping {
                item_id: "$.id".to_string(),
                label: None,
                vars: HashMap::new(),
            },
            replace: false,
        };

        let items = extract_dynamic_items(&vars, &action).expect("extract items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].item_id, "has_id");
    }
}
