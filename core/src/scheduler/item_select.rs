use crate::config::{ItemSelectConfig, SelectionResult, SelectionStrategy, TieBreak};
use anyhow::{Context, Result};
use std::collections::HashMap;
use tracing::info;

/// Per-item state collected after evaluation, used for selection.
#[derive(Debug, Clone)]
pub struct ItemEvalState {
    /// Candidate item identifier.
    pub item_id: String,
    /// Pipeline variables captured for the candidate item.
    pub pipeline_vars: HashMap<String, String>,
}

/// Execute item selection across evaluated items based on the given config.
pub fn execute_item_select(
    item_states: &[ItemEvalState],
    config: &ItemSelectConfig,
) -> Result<SelectionResult> {
    if item_states.is_empty() {
        anyhow::bail!("item_select: no items to select from");
    }
    if item_states.len() == 1 {
        let item = &item_states[0];
        return Ok(SelectionResult {
            winner_id: item.item_id.clone(),
            eliminated_ids: vec![],
            winner_vars: item.pipeline_vars.clone(),
        });
    }

    let winner_idx = match config.strategy {
        SelectionStrategy::Min => select_min_max(item_states, config, false)?,
        SelectionStrategy::Max => select_min_max(item_states, config, true)?,
        SelectionStrategy::Threshold => select_threshold(item_states, config)?,
        SelectionStrategy::Weighted => select_weighted(item_states, config)?,
    };

    let winner = &item_states[winner_idx];
    let eliminated_ids = item_states
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != winner_idx)
        .map(|(_, s)| s.item_id.clone())
        .collect();

    info!(
        winner = %winner.item_id,
        strategy = ?config.strategy,
        "item_select completed"
    );

    Ok(SelectionResult {
        winner_id: winner.item_id.clone(),
        eliminated_ids,
        winner_vars: winner.pipeline_vars.clone(),
    })
}

/// Select item with min or max metric value.
fn select_min_max(
    items: &[ItemEvalState],
    config: &ItemSelectConfig,
    maximize: bool,
) -> Result<usize> {
    let metric_var = config
        .metric_var
        .as_ref()
        .context("metric_var required for min/max strategy")?;

    let mut scored: Vec<(usize, f64)> = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        if let Some(val_str) = item.pipeline_vars.get(metric_var) {
            if let Ok(val) = val_str.parse::<f64>() {
                scored.push((idx, val));
            }
        }
    }

    if scored.is_empty() {
        anyhow::bail!(
            "item_select: no items have parseable metric_var '{}'",
            metric_var
        );
    }

    scored.sort_by(|a, b| {
        if maximize {
            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
        }
    });

    // Check for ties
    let best_val = scored[0].1;
    let ties: Vec<usize> = scored
        .iter()
        .take_while(|(_, v)| (*v - best_val).abs() < f64::EPSILON)
        .map(|(idx, _)| *idx)
        .collect();

    Ok(break_tie(&ties, config.tie_break))
}

/// Select items that pass a threshold, then pick the best among them.
fn select_threshold(items: &[ItemEvalState], config: &ItemSelectConfig) -> Result<usize> {
    let metric_var = config
        .metric_var
        .as_ref()
        .context("metric_var required for threshold strategy")?;
    let threshold = config
        .threshold
        .context("threshold value required for threshold strategy")?;

    let passing: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            item.pipeline_vars
                .get(metric_var)
                .and_then(|v| v.parse::<f64>().ok())
                .map(|v| v >= threshold)
                .unwrap_or(false)
        })
        .map(|(idx, _)| idx)
        .collect();

    if passing.is_empty() {
        anyhow::bail!(
            "item_select: no items pass threshold {} for '{}'",
            threshold,
            metric_var
        );
    }

    Ok(break_tie(&passing, config.tie_break))
}

/// Weighted multi-metric scoring.
fn select_weighted(items: &[ItemEvalState], config: &ItemSelectConfig) -> Result<usize> {
    let weights = config
        .weights
        .as_ref()
        .context("weights required for weighted strategy")?;

    let mut scores: Vec<(usize, f64)> = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        let mut total = 0.0;
        for (var, weight) in weights {
            let val = item
                .pipeline_vars
                .get(var)
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            total += val * weight;
        }
        scores.push((idx, total));
    }

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if scores.is_empty() {
        anyhow::bail!("item_select: no items to score");
    }

    let best_val = scores[0].1;
    let ties: Vec<usize> = scores
        .iter()
        .take_while(|(_, v)| (*v - best_val).abs() < f64::EPSILON)
        .map(|(idx, _)| *idx)
        .collect();

    Ok(break_tie(&ties, config.tie_break))
}

fn break_tie(candidates: &[usize], tie_break: TieBreak) -> usize {
    match tie_break {
        TieBreak::First => candidates[0],
        TieBreak::Last => candidates[candidates.len() - 1],
        TieBreak::Random => {
            // Deterministic for now: pick first
            candidates[0]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SelectionStrategy, TieBreak};

    fn make_item(id: &str, vars: Vec<(&str, &str)>) -> ItemEvalState {
        ItemEvalState {
            item_id: id.to_string(),
            pipeline_vars: vars
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    fn make_config(strategy: SelectionStrategy) -> ItemSelectConfig {
        ItemSelectConfig {
            strategy,
            metric_var: Some("score".to_string()),
            weights: None,
            threshold: None,
            store_result: None,
            tie_break: TieBreak::First,
        }
    }

    #[test]
    fn test_select_min() {
        let items = vec![
            make_item("a", vec![("score", "5.0")]),
            make_item("b", vec![("score", "2.0")]),
            make_item("c", vec![("score", "8.0")]),
        ];
        let config = make_config(SelectionStrategy::Min);
        let result = execute_item_select(&items, &config).unwrap();
        assert_eq!(result.winner_id, "b");
        assert_eq!(result.eliminated_ids.len(), 2);
    }

    #[test]
    fn test_select_max() {
        let items = vec![
            make_item("a", vec![("score", "5.0")]),
            make_item("b", vec![("score", "2.0")]),
            make_item("c", vec![("score", "8.0")]),
        ];
        let config = make_config(SelectionStrategy::Max);
        let result = execute_item_select(&items, &config).unwrap();
        assert_eq!(result.winner_id, "c");
    }

    #[test]
    fn test_select_threshold() {
        let items = vec![
            make_item("a", vec![("score", "3.0")]),
            make_item("b", vec![("score", "7.0")]),
            make_item("c", vec![("score", "9.0")]),
        ];
        let mut config = make_config(SelectionStrategy::Threshold);
        config.threshold = Some(5.0);
        let result = execute_item_select(&items, &config).unwrap();
        // b and c pass threshold; first wins with TieBreak::First
        assert_eq!(result.winner_id, "b");
    }

    #[test]
    fn test_select_weighted() {
        let items = vec![
            make_item("a", vec![("quality", "8.0"), ("speed", "2.0")]),
            make_item("b", vec![("quality", "5.0"), ("speed", "9.0")]),
        ];
        let mut weights = HashMap::new();
        weights.insert("quality".to_string(), 0.7);
        weights.insert("speed".to_string(), 0.3);

        let config = ItemSelectConfig {
            strategy: SelectionStrategy::Weighted,
            metric_var: None,
            weights: Some(weights),
            threshold: None,
            store_result: None,
            tie_break: TieBreak::First,
        };
        // a: 8*0.7 + 2*0.3 = 6.2
        // b: 5*0.7 + 9*0.3 = 6.2
        // Tied, first wins
        let result = execute_item_select(&items, &config).unwrap();
        assert_eq!(result.winner_id, "a");
    }

    #[test]
    fn test_single_item() {
        let items = vec![make_item("only", vec![("score", "5.0")])];
        let config = make_config(SelectionStrategy::Min);
        let result = execute_item_select(&items, &config).unwrap();
        assert_eq!(result.winner_id, "only");
        assert!(result.eliminated_ids.is_empty());
    }

    #[test]
    fn test_empty_items_fails() {
        let items: Vec<ItemEvalState> = vec![];
        let config = make_config(SelectionStrategy::Min);
        assert!(execute_item_select(&items, &config).is_err());
    }

    #[test]
    fn test_tie_break_last() {
        let items = vec![
            make_item("a", vec![("score", "5.0")]),
            make_item("b", vec![("score", "5.0")]),
        ];
        let mut config = make_config(SelectionStrategy::Min);
        config.tie_break = TieBreak::Last;
        let result = execute_item_select(&items, &config).unwrap();
        assert_eq!(result.winner_id, "b");
    }
}
