use rusqlite::{params, params_from_iter, Connection};
use serde_json::{json, Value};

use crate::error::CommandResult;

const CANVAS_WIDTH: f64 = 1000.0;
const CANVAS_HEIGHT: f64 = 650.0;

pub(crate) fn list_node_layout_for_nodes(conn: &Connection, node_ids: &[String]) -> CommandResult<Value> {
  if node_ids.is_empty() {
    return Ok(empty_layout_state());
  }

  let placeholders = vec!["?"; node_ids.len()].join(", ");
  let sql = format!(
    concat!(
      "SELECT node_id, x, y, pinned, updated_at FROM graph_node_layout ",
      "WHERE node_id IN ({placeholders}) ORDER BY updated_at, node_id"
    ),
    placeholders = placeholders
  );
  let mut stmt = conn.prepare(&sql)?;
  let rows = stmt.query_map(params_from_iter(node_ids.iter()), |row| {
    Ok((
      row.get::<_, String>(0)?,
      row.get::<_, f64>(1)?,
      row.get::<_, f64>(2)?,
      row.get::<_, i64>(3)? == 1,
      row.get::<_, String>(4)?,
    ))
  })?;

  layout_rows_value(rows)
}

pub(crate) fn persist_node_position(
  conn: &Connection,
  node_id: &str,
  x: f64,
  y: f64,
  pinned: bool,
  updated_at: &str,
) -> CommandResult<Value> {
  let position = normalize_layout_position(node_id, x, y, pinned, None);
  conn.execute(
    r#"
        INSERT INTO graph_node_layout (node_id, x, y, pinned, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(node_id) DO UPDATE SET
          x = excluded.x,
          y = excluded.y,
          pinned = excluded.pinned,
          updated_at = excluded.updated_at
        "#,
    params![
      node_id,
      position["x"].as_f64().unwrap_or(0.0),
      position["y"].as_f64().unwrap_or(0.0),
      if pinned { 1 } else { 0 },
      updated_at
    ],
  )?;
  let mut result = position;
  result["updated_at"] = json!(updated_at);
  Ok(result)
}

fn empty_layout_state() -> Value {
  json!({
    "layoutOverrides": {},
    "pinnedNodeIds": []
  })
}

fn layout_rows_value<T>(rows: T) -> CommandResult<Value>
where
  T: IntoIterator<Item = rusqlite::Result<(String, f64, f64, bool, String)>>,
{
  let mut layout_overrides = serde_json::Map::new();
  let mut pinned_node_ids = Vec::new();
  for row in rows {
    let (node_id, x, y, pinned, updated_at) = row?;
    let position = normalize_layout_position(&node_id, x, y, pinned, Some(updated_at));
    if pinned {
      pinned_node_ids.push(node_id.clone());
    }
    layout_overrides.insert(node_id, position);
  }

  Ok(json!({
    "layoutOverrides": layout_overrides,
    "pinnedNodeIds": pinned_node_ids
  }))
}

fn normalize_layout_position(node_id: &str, x: f64, y: f64, pinned: bool, updated_at: Option<String>) -> Value {
  let safe_left = clamp((x / CANVAS_WIDTH) * 100.0, 4.0, 96.0);
  let safe_top = clamp((y / CANVAS_HEIGHT) * 100.0, 8.0, 92.0);
  let normalized_x = round2((safe_left / 100.0) * CANVAS_WIDTH);
  let normalized_y = round2((safe_top / 100.0) * CANVAS_HEIGHT);
  let mut value = json!({
    "node_id": node_id,
    "x": normalized_x,
    "y": normalized_y,
    "left": round2(safe_left),
    "top": round2(safe_top),
    "pinned": pinned
  });
  if let Some(updated_at) = updated_at {
    value["updated_at"] = json!(updated_at);
  }
  value
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
  value.max(min).min(max)
}

fn round2(value: f64) -> f64 {
  (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn empty_layout_state_has_bootstrap_shape() {
    assert_eq!(
      list_node_layout_for_nodes(&Connection::open_in_memory().unwrap(), &[]).unwrap(),
      json!({
        "layoutOverrides": {},
        "pinnedNodeIds": []
      })
    );
  }

  #[test]
  fn layout_rows_are_clamped_and_list_pinned_nodes() {
    let rows = vec![
      Ok(("node_a".to_string(), -50.0, 900.0, true, "2026-01-01T00:00:00Z".to_string())),
      Ok(("node_b".to_string(), 500.0, 325.0, false, "2026-01-01T00:00:01Z".to_string())),
    ];

    let layout = layout_rows_value(rows).unwrap();

    assert_eq!(layout["pinnedNodeIds"], json!(["node_a"]));
    assert_eq!(layout["layoutOverrides"]["node_a"]["left"], 4.0);
    assert_eq!(layout["layoutOverrides"]["node_a"]["top"], 92.0);
    assert_eq!(layout["layoutOverrides"]["node_b"]["left"], 50.0);
    assert_eq!(layout["layoutOverrides"]["node_b"]["pinned"], false);
  }
}
