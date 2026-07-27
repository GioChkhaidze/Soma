use std::collections::HashSet;

use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

use crate::error::{CommandError, CommandResult};

const GRAPH_CONTEXT_QUERY_TERM_LIMIT: usize = 8;

pub(crate) fn active_graph_context_node_ids(
  conn: &Connection,
  search_terms: &[String],
  focus_node_ids: &[String],
  limit: usize,
) -> CommandResult<Vec<String>> {
  if limit == 0 {
    return Ok(Vec::new());
  }

  let mut ids = active_focus_node_ids(conn, focus_node_ids, limit)?;
  if ids.len() >= limit {
    return Ok(ids);
  }

  let terms: Vec<String> =
    search_terms.iter().filter(|term| !term.trim().is_empty()).take(GRAPH_CONTEXT_QUERY_TERM_LIMIT).cloned().collect();
  if terms.is_empty() {
    return Ok(ids);
  }

  append_card_match_node_ids(conn, &terms, &mut ids, limit)?;
  append_body_match_node_ids(conn, &terms, &mut ids, limit)?;
  Ok(ids)
}

fn append_card_match_node_ids(
  conn: &Connection,
  terms: &[String],
  ids: &mut Vec<String>,
  limit: usize,
) -> CommandResult<()> {
  if ids.len() >= limit || terms.is_empty() {
    return Ok(());
  }

  let mut score_parts = Vec::new();
  let mut where_parts = Vec::new();
  let mut query_params = Vec::new();
  for term in terms {
    let pattern = like_pattern(term);
    score_parts.push(
      "(CASE WHEN lower(graph_nodes.title) LIKE ? ESCAPE '\\' THEN 8 ELSE 0 END \
             + CASE WHEN lower(graph_nodes.node_type) LIKE ? ESCAPE '\\' THEN 4 ELSE 0 END \
             + CASE WHEN lower(coalesce(graph_nodes.preview, '')) LIKE ? ESCAPE '\\' THEN 3 ELSE 0 END)"
        .to_string(),
    );
    query_params.extend([pattern.clone(), pattern.clone(), pattern.clone()]);
  }
  for term in terms {
    let pattern = like_pattern(term);
    where_parts.push(
      "(lower(graph_nodes.title) LIKE ? ESCAPE '\\' \
              OR lower(graph_nodes.node_type) LIKE ? ESCAPE '\\' \
              OR lower(coalesce(graph_nodes.preview, '')) LIKE ? ESCAPE '\\')"
        .to_string(),
    );
    query_params.extend([pattern.clone(), pattern.clone(), pattern]);
  }

  let exclusion = exclusion_clause(ids, &mut query_params);
  let remaining = limit - ids.len();
  let sql = format!(
    r#"
    SELECT graph_nodes.id
    FROM graph_nodes
    WHERE graph_nodes.status = 'active'
      AND ({})
      {}
    ORDER BY ({}) DESC, graph_nodes.title, graph_nodes.id
    LIMIT {remaining}
    "#,
    where_parts.join(" OR "),
    exclusion,
    score_parts.join(" + ")
  );
  let mut stmt = conn.prepare(&sql)?;
  let rows = stmt.query_map(params_from_iter(query_params.iter()), |row| row.get::<_, String>(0))?;
  for id in rows {
    ids.push(id?);
  }
  Ok(())
}

fn append_body_match_node_ids(
  conn: &Connection,
  terms: &[String],
  ids: &mut Vec<String>,
  limit: usize,
) -> CommandResult<()> {
  if ids.len() >= limit || terms.is_empty() {
    return Ok(());
  }

  let mut query_params = vec![fts_query(terms)];
  let exclusion = exclusion_clause(ids, &mut query_params);
  let remaining = limit - ids.len();
  let sql = format!(
    r#"
    SELECT graph_nodes.id, MIN(rank) AS best_rank
    FROM node_body_versions_fts
    JOIN graph_nodes ON graph_nodes.current_body_version_id = node_body_versions_fts.body_version_id
    WHERE node_body_versions_fts MATCH ?
      AND graph_nodes.status = 'active'
      {}
    GROUP BY graph_nodes.id
    ORDER BY best_rank, graph_nodes.title, graph_nodes.id
    LIMIT {remaining}
    "#,
    exclusion
  );
  let mut stmt = conn.prepare(&sql)?;
  let rows = stmt.query_map(params_from_iter(query_params.iter()), |row| row.get::<_, String>(0))?;
  for id in rows {
    ids.push(id?);
  }
  Ok(())
}

fn exclusion_clause(ids: &[String], query_params: &mut Vec<String>) -> String {
  if ids.is_empty() {
    return String::new();
  }
  query_params.extend(ids.iter().cloned());
  format!("AND graph_nodes.id NOT IN ({})", vec!["?"; ids.len()].join(", "))
}

pub(crate) fn active_node_context_node_ids(
  conn: &Connection,
  node_id: &str,
  limit: usize,
) -> CommandResult<Vec<String>> {
  if limit == 0 {
    return Ok(Vec::new());
  }
  let node_id = node_id.trim();
  if node_id.is_empty() {
    return Err(CommandError::validation("Node id is required."));
  }

  let mut ids = active_focus_node_ids(conn, &[node_id.to_string()], 1)?;
  if ids.is_empty() {
    return Err(CommandError::validation(format!("Active node not found: {node_id}")));
  }
  if ids.len() >= limit {
    return Ok(ids);
  }

  let remaining = limit - ids.len();
  let sql = format!(
    r#"
    WITH candidate_neighbors AS (
      SELECT graph_edges.target_node_id AS neighbor_id, graph_edges.id AS edge_id
      FROM graph_edges
      WHERE graph_edges.status = 'active'
        AND graph_edges.source_node_id = ?1
      UNION ALL
      SELECT graph_edges.source_node_id AS neighbor_id, graph_edges.id AS edge_id
      FROM graph_edges
      WHERE graph_edges.status = 'active'
        AND graph_edges.target_node_id = ?1
    )
    SELECT
      candidate_neighbors.neighbor_id,
      MIN(candidate_neighbors.edge_id) AS first_edge_id
    FROM candidate_neighbors
    JOIN graph_nodes AS neighbor_nodes
      ON neighbor_nodes.id = candidate_neighbors.neighbor_id
    WHERE neighbor_nodes.status = 'active'
      AND candidate_neighbors.neighbor_id <> ?1
    GROUP BY candidate_neighbors.neighbor_id
    ORDER BY first_edge_id
    LIMIT {remaining}
    "#
  );
  let mut stmt = conn.prepare(&sql)?;
  let rows = stmt.query_map(params![node_id], |row| row.get::<_, String>(0))?;
  for id in rows {
    ids.push(id?);
  }
  Ok(ids)
}

fn active_focus_node_ids(conn: &Connection, focus_node_ids: &[String], limit: usize) -> CommandResult<Vec<String>> {
  let mut stmt = conn.prepare("SELECT id FROM graph_nodes WHERE status = 'active' AND id = ?1")?;
  let mut seen = HashSet::new();
  let mut ids = Vec::new();
  for node_id in focus_node_ids {
    if ids.len() >= limit {
      break;
    }
    let node_id = node_id.trim();
    if node_id.is_empty() || !seen.insert(node_id.to_string()) {
      continue;
    }
    if let Some(id) = stmt.query_row(params![node_id], |row| row.get::<_, String>(0)).optional()? {
      ids.push(id);
    }
  }
  Ok(ids)
}

fn like_pattern(term: &str) -> String {
  let mut escaped = String::with_capacity(term.len() + 2);
  escaped.push('%');
  for ch in term.to_lowercase().chars() {
    match ch {
      '%' | '_' | '\\' => {
        escaped.push('\\');
        escaped.push(ch);
      }
      _ => escaped.push(ch),
    }
  }
  escaped.push('%');
  escaped
}

fn fts_query(terms: &[String]) -> String {
  terms.iter().map(|term| format!("\"{}\"", term.replace('"', "\"\""))).collect::<Vec<_>>().join(" OR ")
}

#[cfg(test)]
mod tests {
  use super::*;
  use rusqlite::{params, Connection};

  #[test]
  fn active_graph_context_node_ids_include_body_matches() {
    let conn = Connection::open_in_memory().unwrap();
    create_retrieval_schema(&conn);
    seed_node(&conn, "focus", "concept", "Pinned Focus", "manual focus", "active", "");
    seed_node(&conn, "title_alpha", "concept", "Alpha Case", "", "active", "");
    seed_node(&conn, "preview_alpha", "concept", "Preview Match", "alpha appears in the card preview", "active", "");
    seed_node(
      &conn,
      "body_only",
      "concept",
      "Body Only",
      "nothing relevant",
      "active",
      "alpha sentinelbody appears only in compiled body",
    );
    seed_node(&conn, "hidden_alpha", "concept", "Alpha Hidden", "alpha hidden", "hidden", "");

    let terms = vec!["alpha".to_string(), "sentinelbody".to_string()];
    let focus = vec!["focus".to_string(), "focus".to_string(), "hidden_alpha".to_string()];
    let ids = active_graph_context_node_ids(&conn, &terms, &focus, 4).unwrap();

    assert_eq!(ids, vec!["focus", "title_alpha", "preview_alpha", "body_only"]);
    assert!(!ids.contains(&"hidden_alpha".to_string()));
  }

  #[test]
  fn active_graph_context_node_ids_search_current_body_version_only() {
    let conn = Connection::open_in_memory().unwrap();
    create_retrieval_schema(&conn);
    let first_body = seed_node(
      &conn,
      "versioned",
      "concept",
      "Versioned Node",
      "nothing relevant",
      "active",
      "restored body sentinel",
    );
    let second_body = insert_body_version(&conn, "versioned", 2, "new body transient");
    conn
      .execute("UPDATE graph_nodes SET current_body_version_id = ?1 WHERE id = 'versioned'", params![second_body])
      .unwrap();

    let restored_terms = vec!["restored".to_string()];
    let new_terms = vec!["transient".to_string()];

    assert!(active_graph_context_node_ids(&conn, &restored_terms, &[], 3).unwrap().is_empty());
    assert_eq!(active_graph_context_node_ids(&conn, &new_terms, &[], 3).unwrap(), vec!["versioned"]);

    conn
      .execute("UPDATE graph_nodes SET current_body_version_id = ?1 WHERE id = 'versioned'", params![first_body])
      .unwrap();

    assert_eq!(active_graph_context_node_ids(&conn, &restored_terms, &[], 3).unwrap(), vec!["versioned"]);
    assert!(active_graph_context_node_ids(&conn, &new_terms, &[], 3).unwrap().is_empty());
  }

  #[test]
  fn active_node_context_node_ids_are_bounded_to_focused_neighborhood() {
    let conn = Connection::open_in_memory().unwrap();
    create_retrieval_schema(&conn);
    conn
      .execute_batch(
        r#"
            CREATE TABLE graph_edges (
              id TEXT PRIMARY KEY,
              source_node_id TEXT NOT NULL,
              target_node_id TEXT NOT NULL,
              edge_type TEXT NOT NULL,
              bridge_text TEXT,
              status TEXT NOT NULL
            );
            "#,
      )
      .unwrap();
    seed_node(&conn, "focus", "concept", "Focus", "", "active", "focused body");
    seed_node(&conn, "near_a", "concept", "Near A", "", "active", "near body");
    seed_node(&conn, "near_b", "concept", "Near B", "", "active", "near body");
    seed_node(&conn, "hidden_near", "concept", "Hidden Near", "", "hidden", "hidden body");
    seed_node(&conn, "far", "concept", "Far", "", "active", "far body");
    let insert_edge = |id: &str, source: &str, target: &str| {
      conn
        .execute(
          concat!(
            "INSERT INTO graph_edges ",
            "(id, source_node_id, target_node_id, edge_type, bridge_text, status) ",
            "VALUES (?1, ?2, ?3, 'mentions', NULL, 'active')"
          ),
          params![id, source, target],
        )
        .unwrap();
    };
    insert_edge("edge_0_self", "focus", "focus");
    insert_edge("edge_a", "focus", "near_a");
    insert_edge("edge_b", "near_b", "focus");
    insert_edge("edge_hidden", "focus", "hidden_near");
    insert_edge("edge_far", "far", "near_a");

    let ids = active_node_context_node_ids(&conn, "focus", 3).unwrap();

    assert_eq!(ids, vec!["focus", "near_a", "near_b"]);
    assert!(!ids.contains(&"hidden_near".to_string()));
    assert!(!ids.contains(&"far".to_string()));
  }

  fn create_retrieval_schema(conn: &Connection) {
    conn
      .execute_batch(
        r#"
            CREATE TABLE graph_nodes (
              id TEXT PRIMARY KEY,
              node_type TEXT NOT NULL,
              title TEXT NOT NULL,
              preview TEXT,
              current_body_version_id TEXT,
              status TEXT NOT NULL
            );
            CREATE TABLE node_body_versions (
              id TEXT PRIMARY KEY,
              node_id TEXT NOT NULL,
              version_number INTEGER NOT NULL,
              compiled_body TEXT NOT NULL
            );
            CREATE VIRTUAL TABLE node_body_versions_fts USING fts5(
              compiled_body,
              body_version_id UNINDEXED,
              node_id UNINDEXED
            );
            "#,
      )
      .unwrap();
  }

  fn seed_node(
    conn: &Connection,
    id: &str,
    node_type: &str,
    title: &str,
    preview: &str,
    status: &str,
    compiled_body: &str,
  ) -> String {
    let body_id = format!("{id}_body");
    conn
      .execute(
        concat!(
          "INSERT INTO graph_nodes ",
          "(id, node_type, title, preview, current_body_version_id, status) ",
          "VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
        ),
        params![id, node_type, title, preview, body_id, status],
      )
      .unwrap();
    insert_body_version(conn, id, 1, compiled_body);
    body_id
  }

  fn insert_body_version(conn: &Connection, node_id: &str, version_number: i64, compiled_body: &str) -> String {
    let body_id =
      if version_number == 1 { format!("{node_id}_body") } else { format!("{node_id}_body_{version_number}") };
    conn
      .execute(
        "INSERT INTO node_body_versions (id, node_id, version_number, compiled_body) VALUES (?1, ?2, ?3, ?4)",
        params![body_id, node_id, version_number, compiled_body],
      )
      .unwrap();
    conn
      .execute(
        "INSERT INTO node_body_versions_fts (compiled_body, body_version_id, node_id) VALUES (?1, ?2, ?3)",
        params![compiled_body, body_id, node_id],
      )
      .unwrap();
    body_id
  }
}
