use std::collections::{HashMap, HashSet};

use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::contracts::{GRAPH_PATCH_SCHEMA_VERSION, NODE_BODY_MAX_WORDS};
use crate::error::{CommandError, CommandResult};

const EXCERPT_MAX_CHARS: usize = 260;
const CONTEXT_EDGE_LIMIT: usize = 16;
pub(crate) const STARTUP_CANVAS_NODE_LIMIT: usize = 160;
pub(crate) const STARTUP_CANVAS_EDGE_LIMIT: usize = 320;

pub(crate) fn active_graph_snapshot(conn: &Connection) -> CommandResult<Value> {
  let mut nodes_stmt = conn.prepare(
    r#"
    SELECT
      graph_nodes.id,
      graph_nodes.node_type,
      graph_nodes.title,
      graph_nodes.preview,
      graph_nodes.status,
      graph_nodes.created_at,
      graph_nodes.updated_at,
      graph_nodes.authored_by_user,
      node_body_versions.id AS body_version_id,
      node_body_versions.version_number,
      node_body_versions.compiled_body,
      node_body_versions.authored_by_user AS body_authored_by_user
    FROM graph_nodes
    JOIN node_body_versions ON graph_nodes.current_body_version_id = node_body_versions.id
    WHERE graph_nodes.status = 'active'
    ORDER BY graph_nodes.title, graph_nodes.id
    "#,
  )?;
  let node_rows = nodes_stmt.query_map([], |row| {
    Ok(NodeRow {
      id: row.get(0)?,
      node_type: row.get(1)?,
      title: row.get(2)?,
      preview: row.get(3)?,
      status: row.get(4)?,
      created_at: row.get(5)?,
      updated_at: row.get(6)?,
      authored_by_user: row.get::<_, i64>(7)? == 1,
      body_version_id: row.get(8)?,
      version_number: row.get(9)?,
      compiled_body: row.get(10)?,
      body_authored_by_user: row.get::<_, i64>(11)? == 1,
    })
  })?;
  let node_rows = node_rows.collect::<Result<Vec<_>, _>>()?;
  drop(nodes_stmt);

  let mut nodes = Vec::new();
  for node in node_rows {
    nodes.push(full_node_value(conn, node)?);
  }

  let mut edges_stmt = conn.prepare(
    r#"
    SELECT
      id,
      source_node_id,
      target_node_id,
      edge_type,
      bridge_text,
      status,
      authored_by_user,
      created_at,
      updated_at
    FROM graph_edges
    WHERE status = 'active'
    ORDER BY source_node_id, target_node_id, id
    "#,
  )?;
  let edge_rows = edges_stmt.query_map([], |row| {
    Ok(EdgeRow {
      id: row.get(0)?,
      source_node_id: row.get(1)?,
      target_node_id: row.get(2)?,
      edge_type: row.get(3)?,
      bridge_text: row.get(4)?,
      status: row.get(5)?,
      authored_by_user: row.get::<_, i64>(6)? == 1,
      created_at: row.get(7)?,
      updated_at: row.get(8)?,
    })
  })?;
  let edge_rows = edge_rows.collect::<Result<Vec<_>, _>>()?;
  drop(edges_stmt);

  let mut edges = Vec::new();
  for edge in edge_rows {
    let provenance = edge_provenance(conn, &edge.id, edge.authored_by_user)?;
    edges.push(json!({
      "id": edge.id,
      "source_node_id": edge.source_node_id,
      "target_node_id": edge.target_node_id,
      "type": edge.edge_type,
      "bridge_text": edge.bridge_text,
      "status": edge.status,
      "created_at": edge.created_at,
      "updated_at": edge.updated_at,
      "markers": provenance.markers,
      "source_chunk_ids": provenance.source_chunk_ids,
      "evidence": provenance.evidence
    }));
  }

  Ok(json!({
    "schema_version": GRAPH_PATCH_SCHEMA_VERSION,
    "nodes": nodes,
    "edges": edges,
    "paths": []
  }))
}

pub(crate) fn active_graph_startup_canvas_snapshot(conn: &Connection) -> CommandResult<Value> {
  let nodes = active_graph_node_cards_limited(conn, Some(STARTUP_CANVAS_NODE_LIMIT))?;
  let node_ids = node_ids_from_values(&nodes);
  let edges = active_graph_canvas_edges_for_node_ids(conn, &node_ids)?;
  let total_node_count = active_graph_count(conn, "graph_nodes")?;
  let total_edge_count = active_graph_count(conn, "graph_edges")?;

  Ok(json!({
    "schema_version": GRAPH_PATCH_SCHEMA_VERSION,
    "nodes": nodes,
    "edges": edges,
    "paths": [],
    "is_partial": total_node_count > node_ids.len() || total_edge_count > edges.len(),
    "node_limit": STARTUP_CANVAS_NODE_LIMIT,
    "edge_limit": STARTUP_CANVAS_EDGE_LIMIT,
    "total_node_count": total_node_count,
    "total_edge_count": total_edge_count
  }))
}

fn active_graph_count(conn: &Connection, table: &str) -> CommandResult<usize> {
  let sql = format!("SELECT COUNT(*) FROM {table} WHERE status = 'active'");
  let count = conn.query_row(&sql, [], |row| row.get::<_, i64>(0))?;
  Ok(count.max(0) as usize)
}

fn active_graph_canvas_edges_for_node_ids(conn: &Connection, node_ids: &[String]) -> CommandResult<Vec<Value>> {
  if node_ids.is_empty() {
    return Ok(Vec::new());
  }
  let placeholders = vec!["?"; node_ids.len()].join(", ");
  let mut query_params = Vec::with_capacity(node_ids.len() * 2);
  query_params.extend(node_ids.iter().cloned());
  query_params.extend(node_ids.iter().cloned());
  let sql = format!(
    r#"
    SELECT
      id,
      source_node_id,
      target_node_id,
      edge_type,
      bridge_text,
      status,
      authored_by_user,
      created_at,
      updated_at,
      EXISTS(
        SELECT 1
        FROM graph_evidence
        WHERE graph_evidence.entity_type = 'edge'
          AND graph_evidence.entity_id = graph_edges.id
      )
      OR EXISTS(
        SELECT 1
        FROM graph_message_evidence
        WHERE graph_message_evidence.target_entity_type = 'edge'
          AND graph_message_evidence.target_entity_id = graph_edges.id
      )
      OR EXISTS(
        SELECT 1
        FROM node_message_evidence
        WHERE node_message_evidence.target_entity_type = 'edge'
          AND node_message_evidence.target_entity_id = graph_edges.id
      ) AS has_evidence
    FROM graph_edges
    WHERE status = 'active'
      AND source_node_id IN ({placeholders})
      AND target_node_id IN ({placeholders})
    ORDER BY source_node_id, target_node_id, id
    LIMIT {STARTUP_CANVAS_EDGE_LIMIT}
    "#
  );
  let mut stmt = conn.prepare(&sql)?;
  let rows = stmt.query_map(params_from_iter(query_params.iter()), canvas_edge_from_row)?;
  rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn canvas_edge_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
  let markers = edge_markers(row.get::<_, i64>(9)? == 1, row.get::<_, i64>(6)? == 1);
  Ok(json!({
    "id": row.get::<_, String>(0)?,
    "source_node_id": row.get::<_, String>(1)?,
    "target_node_id": row.get::<_, String>(2)?,
    "type": row.get::<_, String>(3)?,
    "bridge_text": row.get::<_, Option<String>>(4)?,
    "status": row.get::<_, String>(5)?,
    "created_at": row.get::<_, String>(7)?,
    "updated_at": row.get::<_, String>(8)?,
    "markers": markers,
    "source_chunk_ids": []
  }))
}

fn active_graph_node_cards_limited(conn: &Connection, limit: Option<usize>) -> CommandResult<Vec<Value>> {
  query_active_graph_node_cards(conn, "", &[], limit)
}

pub(crate) fn active_graph_node_cards_for_ids(conn: &Connection, node_ids: &[String]) -> CommandResult<Vec<Value>> {
  if node_ids.is_empty() {
    return Ok(Vec::new());
  }

  let filter_clause = format!("AND graph_nodes.id IN ({})", vec!["?"; node_ids.len()].join(", "));
  let cards = query_active_graph_node_cards(conn, &filter_clause, node_ids, Some(node_ids.len()))?;
  let mut cards_by_id = cards
    .into_iter()
    .filter_map(|card| {
      let id = card.get("id").and_then(Value::as_str)?.to_string();
      Some((id, card))
    })
    .collect::<HashMap<_, _>>();
  Ok(node_ids.iter().filter_map(|node_id| cards_by_id.remove(node_id)).collect())
}

fn query_active_graph_node_cards(
  conn: &Connection,
  filter_clause: &str,
  query_params: &[String],
  limit: Option<usize>,
) -> CommandResult<Vec<Value>> {
  let limit_clause = limit.map(|value| format!(" LIMIT {value}")).unwrap_or_default();
  let sql = format!(
    r#"
    SELECT
      graph_nodes.id,
      graph_nodes.node_type,
      graph_nodes.title,
      graph_nodes.preview,
      graph_nodes.status,
      graph_nodes.created_at,
      graph_nodes.updated_at,
      graph_nodes.authored_by_user,
      node_body_versions.id AS body_version_id,
      node_body_versions.version_number,
      node_body_versions.authored_by_user AS body_authored_by_user,
      EXISTS(
        SELECT 1
        FROM graph_evidence
        WHERE (graph_evidence.entity_type = 'node_body_version' AND graph_evidence.entity_id = node_body_versions.id)
           OR (graph_evidence.entity_type = 'node' AND graph_evidence.entity_id = graph_nodes.id)
      )
      OR EXISTS(
        SELECT 1
        FROM graph_message_evidence
        WHERE (
          graph_message_evidence.target_entity_type = 'node_body_version'
          AND graph_message_evidence.target_entity_id = node_body_versions.id
        )
        OR (
          graph_message_evidence.target_entity_type = 'node'
          AND graph_message_evidence.target_entity_id = graph_nodes.id
        )
      )
      OR EXISTS(
        SELECT 1
        FROM node_message_evidence
        WHERE (
          node_message_evidence.target_entity_type = 'node_body_version'
          AND node_message_evidence.target_entity_id = node_body_versions.id
        )
        OR (
          node_message_evidence.target_entity_type = 'node'
          AND node_message_evidence.target_entity_id = graph_nodes.id
        )
      ) AS has_evidence
    FROM graph_nodes
    JOIN node_body_versions ON graph_nodes.current_body_version_id = node_body_versions.id
    WHERE graph_nodes.status = 'active'
    {filter_clause}
    ORDER BY graph_nodes.title, graph_nodes.id
    {limit_clause}
    "#
  );
  let mut nodes_stmt = conn.prepare(&sql)?;
  let node_rows = nodes_stmt.query_map(params_from_iter(query_params.iter()), |row| {
    Ok(json!({
      "id": row.get::<_, String>(0)?,
      "type": row.get::<_, String>(1)?,
      "title": row.get::<_, String>(2)?,
      "preview": row.get::<_, Option<String>>(3)?,
      "status": row.get::<_, String>(4)?,
      "created_at": row.get::<_, String>(5)?,
      "updated_at": row.get::<_, String>(6)?,
      "authored_by_user": row.get::<_, i64>(7)? == 1,
      "body_version_id": row.get::<_, String>(8)?,
      "body_version": row.get::<_, i64>(9)?,
      "markers": node_markers(row.get::<_, i64>(11)? == 1, row.get::<_, i64>(10)? == 1),
      "source_chunk_ids": []
    }))
  })?;
  node_rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(crate) fn node_ids_from_values(nodes: &[Value]) -> Vec<String> {
  nodes.iter().filter_map(|node| node.get("id").and_then(Value::as_str).map(String::from)).collect()
}

pub(crate) fn active_graph_context_snapshot(conn: &Connection, node_ids: &[String]) -> CommandResult<Value> {
  if node_ids.is_empty() {
    return Ok(json!({
      "schema_version": GRAPH_PATCH_SCHEMA_VERSION,
      "nodes": [],
      "edges": [],
      "paths": []
    }));
  }

  let mut nodes = Vec::with_capacity(node_ids.len());
  for node_id in unique_strings(node_ids.to_vec()) {
    nodes.push(graph_context_node_value(conn, active_node_row(conn, &node_id)?)?);
  }

  Ok(json!({
    "schema_version": GRAPH_PATCH_SCHEMA_VERSION,
    "nodes": nodes,
    "edges": active_context_edges(conn, node_ids)?,
    "paths": []
  }))
}

fn active_context_edges(conn: &Connection, node_ids: &[String]) -> CommandResult<Vec<Value>> {
  let mut stmt = conn.prepare(
    r#"
    SELECT
      graph_edges.id,
      graph_edges.source_node_id,
      graph_edges.target_node_id,
      graph_edges.edge_type,
      graph_edges.bridge_text,
      graph_edges.status,
      graph_edges.authored_by_user,
      graph_edges.created_at,
      graph_edges.updated_at,
      source_nodes.title AS source_title,
      target_nodes.title AS target_title
    FROM graph_edges
    JOIN graph_nodes AS source_nodes ON graph_edges.source_node_id = source_nodes.id
    JOIN graph_nodes AS target_nodes ON graph_edges.target_node_id = target_nodes.id
    WHERE graph_edges.status = 'active'
      AND (graph_edges.source_node_id = ?1 OR graph_edges.target_node_id = ?1)
    ORDER BY graph_edges.source_node_id, graph_edges.target_node_id, graph_edges.id
    "#,
  )?;
  let mut seen = HashSet::new();
  let mut edges = Vec::new();

  for node_id in unique_strings(node_ids.to_vec()) {
    let rows = stmt.query_map(params![node_id], |row| {
      Ok((
        EdgeRow {
          id: row.get(0)?,
          source_node_id: row.get(1)?,
          target_node_id: row.get(2)?,
          edge_type: row.get(3)?,
          bridge_text: row.get(4)?,
          status: row.get(5)?,
          authored_by_user: row.get::<_, i64>(6)? == 1,
          created_at: row.get(7)?,
          updated_at: row.get(8)?,
        },
        row.get::<_, String>(9)?,
        row.get::<_, String>(10)?,
      ))
    })?;

    for row in rows {
      let (edge, source_title, target_title) = row?;
      if !seen.insert(edge.id.clone()) {
        continue;
      }
      edges.push(context_edge_value(conn, edge, source_title, target_title)?);
      if edges.len() >= CONTEXT_EDGE_LIMIT {
        return Ok(edges);
      }
    }
  }

  Ok(edges)
}

pub(crate) fn active_graph_node_detail(conn: &Connection, node_id: &str) -> CommandResult<Value> {
  full_node_value(conn, active_node_row(conn, node_id)?)
}

fn active_node_row(conn: &Connection, node_id: &str) -> CommandResult<NodeRow> {
  let node_id = node_id.trim();
  if node_id.is_empty() {
    return Err(CommandError::validation("Node id is required."));
  }

  conn
    .query_row(
      r#"
    SELECT
      graph_nodes.id,
      graph_nodes.node_type,
      graph_nodes.title,
      graph_nodes.preview,
      graph_nodes.status,
      graph_nodes.created_at,
      graph_nodes.updated_at,
      graph_nodes.authored_by_user,
      node_body_versions.id AS body_version_id,
      node_body_versions.version_number,
      node_body_versions.compiled_body,
      node_body_versions.authored_by_user AS body_authored_by_user
    FROM graph_nodes
    JOIN node_body_versions ON graph_nodes.current_body_version_id = node_body_versions.id
    WHERE graph_nodes.status = 'active'
      AND graph_nodes.id = ?1
    "#,
      params![node_id],
      |row| {
        Ok(NodeRow {
          id: row.get(0)?,
          node_type: row.get(1)?,
          title: row.get(2)?,
          preview: row.get(3)?,
          status: row.get(4)?,
          created_at: row.get(5)?,
          updated_at: row.get(6)?,
          authored_by_user: row.get::<_, i64>(7)? == 1,
          body_version_id: row.get(8)?,
          version_number: row.get(9)?,
          compiled_body: row.get(10)?,
          body_authored_by_user: row.get::<_, i64>(11)? == 1,
        })
      },
    )
    .optional()?
    .ok_or_else(|| CommandError::validation(format!("Active node not found: {node_id}")))
}

fn graph_context_node_value(conn: &Connection, node: NodeRow) -> CommandResult<Value> {
  let provenance = node_provenance(conn, &node)?;
  Ok(json!({
    "id": node.id,
    "type": node.node_type,
    "title": node.title,
    "preview": node.preview,
    "compiled_body": node.compiled_body,
    "body_version": node.version_number,
    "body_version_id": node.body_version_id,
    "status": node.status,
    "created_at": node.created_at,
    "updated_at": node.updated_at,
    "authored_by_user": node.authored_by_user,
    "markers": provenance.markers,
    "source_chunk_ids": provenance.source_chunk_ids,
    "evidence": provenance.evidence
  }))
}

fn context_edge_value(
  conn: &Connection,
  edge: EdgeRow,
  source_title: String,
  target_title: String,
) -> CommandResult<Value> {
  let provenance = edge_provenance(conn, &edge.id, edge.authored_by_user)?;
  Ok(json!({
    "id": edge.id,
    "source_node_id": edge.source_node_id,
    "target_node_id": edge.target_node_id,
    "source_title": source_title,
    "target_title": target_title,
    "type": edge.edge_type,
    "bridge_text": edge.bridge_text,
    "status": edge.status,
    "created_at": edge.created_at,
    "updated_at": edge.updated_at,
    "markers": provenance.markers,
    "source_chunk_ids": provenance.source_chunk_ids,
    "evidence": provenance.evidence
  }))
}

fn full_node_value(conn: &Connection, node: NodeRow) -> CommandResult<Value> {
  let provenance = node_provenance(conn, &node)?;
  Ok(json!({
    "id": node.id,
    "type": node.node_type,
    "title": node.title,
    "preview": node.preview,
    "compiled_body": node.compiled_body,
    "body_version": node.version_number,
    "body_version_id": node.body_version_id,
    "body_max_words": NODE_BODY_MAX_WORDS,
    "body_sections": [],
    "update_history": node_body_history(conn, &node.id, &node.body_version_id)?,
    "status": node.status,
    "created_at": node.created_at,
    "updated_at": node.updated_at,
    "authored_by_user": node.authored_by_user,
    "markers": provenance.markers,
    "source_chunk_ids": provenance.source_chunk_ids,
    "evidence": provenance.evidence
  }))
}

fn node_provenance(conn: &Connection, node: &NodeRow) -> CommandResult<GraphProvenance> {
  let evidence = merge_evidence_refs(vec![
    evidence_refs(conn, "node_body_version", &node.body_version_id)?,
    evidence_refs(conn, "node", &node.id)?,
  ]);
  let source_chunk_ids = source_chunk_ids_from_evidence(&evidence);
  let markers = node_markers(!evidence.is_empty(), node.body_authored_by_user);
  Ok(GraphProvenance { evidence, source_chunk_ids, markers })
}

fn edge_provenance(conn: &Connection, edge_id: &str, authored_by_user: bool) -> CommandResult<GraphProvenance> {
  let evidence = evidence_refs(conn, "edge", edge_id)?;
  let source_chunk_ids = source_chunk_ids_from_evidence(&evidence);
  let markers = edge_markers(!evidence.is_empty(), authored_by_user);
  Ok(GraphProvenance { evidence, source_chunk_ids, markers })
}

fn source_chunk_ids_from_evidence(evidence: &[Value]) -> Vec<String> {
  unique_strings(
    evidence.iter().filter_map(|item| item.get("chunk_id").and_then(Value::as_str).map(String::from)).collect(),
  )
}

fn node_markers(source_backed: bool, authored_by_user: bool) -> Vec<&'static str> {
  let mut markers = Vec::new();
  if source_backed {
    markers.push("source_backed");
  }
  markers.push(if authored_by_user { "edited_by_user" } else { "ai_compiled" });
  markers
}

fn edge_markers(source_backed: bool, authored_by_user: bool) -> Vec<&'static str> {
  let mut markers = Vec::new();
  if source_backed {
    markers.push("source_backed");
  }
  if authored_by_user {
    markers.push("edited_by_user");
  }
  markers
}

fn evidence_refs(conn: &Connection, entity_type: &str, entity_id: &str) -> CommandResult<Vec<Value>> {
  let mut evidence = Vec::new();
  let mut stmt = conn.prepare(
    r#"
    SELECT
      graph_evidence.id,
      graph_evidence.entity_type,
      graph_evidence.entity_id,
      graph_evidence.chunk_id,
      COALESCE(graph_evidence.message_id, messages.id) AS message_id,
      graph_evidence.quote_excerpt,
      graph_evidence.created_at,
      chunks.content,
      chunks.chunk_index,
      chunks.token_count,
      messages.role,
      messages.order_index,
      messages.content,
      conversations.id,
      conversations.title,
      sources.id,
      sources.title,
      sources.original_path,
      sources.raw_path
    FROM graph_evidence
    JOIN chunks ON graph_evidence.chunk_id = chunks.id
    JOIN messages ON chunks.message_id = messages.id
    JOIN conversations ON messages.conversation_id = conversations.id
    JOIN sources ON conversations.source_id = sources.id
    WHERE graph_evidence.entity_type = ?1
      AND graph_evidence.entity_id = ?2
    ORDER BY graph_evidence.created_at, graph_evidence.id
    "#,
  )?;
  let rows = stmt.query_map(params![entity_type, entity_id], |row| {
    let quote_excerpt: Option<String> = row.get(5)?;
    let chunk_content: String = row.get(7)?;
    let message_content: String = row.get(12)?;
    Ok(json!({
      "id": row.get::<_, String>(0)?,
      "entity_type": row.get::<_, String>(1)?,
      "entity_id": row.get::<_, String>(2)?,
      "chunk_id": row.get::<_, String>(3)?,
      "message_id": row.get::<_, String>(4)?,
      "quote_excerpt": quote_excerpt,
      "excerpt": quote_excerpt.unwrap_or_else(|| excerpt_text(&chunk_content)),
      "created_at": row.get::<_, String>(6)?,
      "chunk": {
        "id": row.get::<_, String>(3)?,
        "index": row.get::<_, i64>(8)?,
        "token_count": row.get::<_, i64>(9)?
      },
      "message": {
        "id": row.get::<_, String>(4)?,
        "role": row.get::<_, String>(10)?,
        "order_index": row.get::<_, i64>(11)?,
        "excerpt": excerpt_text(&message_content)
      },
      "conversation": {
        "id": row.get::<_, String>(13)?,
        "title": row.get::<_, String>(14)?
      },
      "source": {
        "id": row.get::<_, String>(15)?,
        "title": row.get::<_, String>(16)?,
        "original_path": row.get::<_, Option<String>>(17)?,
        "raw_path": row.get::<_, Option<String>>(18)?
      }
    }))
  })?;
  evidence.extend(rows.collect::<Result<Vec<_>, _>>()?);
  evidence.extend(graph_message_evidence_refs(conn, entity_type, entity_id)?);
  evidence.extend(node_message_evidence_refs(conn, entity_type, entity_id)?);
  evidence.sort_by(|a, b| {
    a.get("created_at")
      .and_then(Value::as_str)
      .cmp(&b.get("created_at").and_then(Value::as_str))
      .then_with(|| a.get("id").and_then(Value::as_str).cmp(&b.get("id").and_then(Value::as_str)))
  });
  Ok(evidence)
}

fn graph_message_evidence_refs(conn: &Connection, entity_type: &str, entity_id: &str) -> CommandResult<Vec<Value>> {
  let mut stmt = conn.prepare(
    r#"
    SELECT
      graph_message_evidence.id,
      graph_message_evidence.target_entity_type,
      graph_message_evidence.target_entity_id,
      graph_message_evidence.graph_thread_message_id,
      graph_message_evidence.quote_excerpt,
      graph_message_evidence.created_at,
      graph_thread_messages.role,
      graph_thread_messages.content
    FROM graph_message_evidence
    JOIN graph_thread_messages ON graph_message_evidence.graph_thread_message_id = graph_thread_messages.id
    WHERE graph_message_evidence.target_entity_type = ?1
      AND graph_message_evidence.target_entity_id = ?2
    ORDER BY graph_message_evidence.created_at, graph_message_evidence.id
    "#,
  )?;
  let rows = stmt.query_map(params![entity_type, entity_id], |row| {
    let quote_excerpt: Option<String> = row.get(4)?;
    let content: String = row.get(7)?;
    Ok(json!({
      "id": row.get::<_, String>(0)?,
      "entity_type": row.get::<_, String>(1)?,
      "entity_id": row.get::<_, String>(2)?,
      "chunk_id": Value::Null,
      "message_id": row.get::<_, String>(3)?,
      "quote_excerpt": quote_excerpt,
      "excerpt": quote_excerpt.unwrap_or_else(|| excerpt_text(&content)),
      "created_at": row.get::<_, String>(5)?,
      "chunk": Value::Null,
      "message": {
        "id": row.get::<_, String>(3)?,
        "role": row.get::<_, String>(6)?,
        "order_index": Value::Null,
        "excerpt": excerpt_text(&content)
      },
      "conversation": { "id": "graph_thread", "title": "Graph Thread" },
      "source": {
        "id": "graph_thread",
        "title": "Graph Thread",
        "original_path": Value::Null,
        "raw_path": Value::Null
      }
    }))
  })?;
  rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn node_message_evidence_refs(conn: &Connection, entity_type: &str, entity_id: &str) -> CommandResult<Vec<Value>> {
  let mut stmt = conn.prepare(
    r#"
    SELECT
      node_message_evidence.id,
      node_message_evidence.target_entity_type,
      node_message_evidence.target_entity_id,
      node_message_evidence.node_thread_message_id,
      node_message_evidence.quote_excerpt,
      node_message_evidence.created_at,
      node_thread_messages.role,
      node_thread_messages.content,
      node_thread_messages.node_id,
      graph_nodes.title
    FROM node_message_evidence
    JOIN node_thread_messages ON node_message_evidence.node_thread_message_id = node_thread_messages.id
    LEFT JOIN graph_nodes ON node_thread_messages.node_id = graph_nodes.id
    WHERE node_message_evidence.target_entity_type = ?1
      AND node_message_evidence.target_entity_id = ?2
    ORDER BY node_message_evidence.created_at, node_message_evidence.id
    "#,
  )?;
  let rows = stmt.query_map(params![entity_type, entity_id], |row| {
    let quote_excerpt: Option<String> = row.get(4)?;
    let content: String = row.get(7)?;
    let node_id: String = row.get(8)?;
    let title: Option<String> = row.get(9)?;
    let source_title =
      title.as_ref().map(|value| format!("Node Thread: {value}")).unwrap_or_else(|| "Node Thread".to_string());
    let source_id = format!("node_thread:{node_id}");
    Ok(json!({
      "id": row.get::<_, String>(0)?,
      "entity_type": row.get::<_, String>(1)?,
      "entity_id": row.get::<_, String>(2)?,
      "chunk_id": Value::Null,
      "message_id": row.get::<_, String>(3)?,
      "quote_excerpt": quote_excerpt,
      "excerpt": quote_excerpt.unwrap_or_else(|| excerpt_text(&content)),
      "created_at": row.get::<_, String>(5)?,
      "chunk": Value::Null,
      "message": {
        "id": row.get::<_, String>(3)?,
        "role": row.get::<_, String>(6)?,
        "order_index": Value::Null,
        "excerpt": excerpt_text(&content)
      },
      "conversation": { "id": source_id.clone(), "title": source_title.clone() },
      "source": {
        "id": source_id,
        "title": source_title,
        "original_path": Value::Null,
        "raw_path": Value::Null
      }
    }))
  })?;
  rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn node_body_history(conn: &Connection, node_id: &str, current_body_version_id: &str) -> CommandResult<Vec<Value>> {
  let mut stmt = conn.prepare(concat!(
    "SELECT id, version_number, authored_by_user, created_at FROM node_body_versions ",
    "WHERE node_id = ?1 ORDER BY version_number DESC"
  ))?;
  let rows = stmt.query_map(params![node_id], |row| {
    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)? == 1, row.get::<_, String>(3)?))
  })?;
  let mut history = Vec::new();
  for row in rows {
    let (id, version_number, authored_by_user, created_at) = row?;
    let evidence = evidence_refs(conn, "node_body_version", &id)?;
    let chunk_ids = source_chunk_ids_from_evidence(&evidence);
    history.push(json!({
      "id": id,
      "version_number": version_number,
      "authored_by_user": authored_by_user,
      "created_at": created_at,
      "is_current": id == current_body_version_id,
      "source_chunk_ids": chunk_ids,
      "evidence": evidence
    }));
  }
  Ok(history)
}

fn merge_evidence_refs(groups: Vec<Vec<Value>>) -> Vec<Value> {
  let mut seen = HashSet::new();
  let mut evidence = Vec::new();
  for item in groups.into_iter().flatten() {
    let key = item.get("id").and_then(Value::as_str).map(String::from).unwrap_or_else(|| {
      format!(
        "{}:{}:{}:{}",
        item.get("entity_type").and_then(Value::as_str).unwrap_or(""),
        item.get("entity_id").and_then(Value::as_str).unwrap_or(""),
        item.get("chunk_id").and_then(Value::as_str).unwrap_or(""),
        item.get("message_id").and_then(Value::as_str).unwrap_or("")
      )
    });
    if seen.insert(key) {
      evidence.push(item);
    }
  }
  evidence.sort_by(|a, b| {
    a.get("created_at")
      .and_then(Value::as_str)
      .cmp(&b.get("created_at").and_then(Value::as_str))
      .then_with(|| a.get("id").and_then(Value::as_str).cmp(&b.get("id").and_then(Value::as_str)))
  });
  evidence
}

fn excerpt_text(value: &str) -> String {
  let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
  if normalized.chars().count() <= EXCERPT_MAX_CHARS {
    return normalized;
  }
  let mut excerpt = normalized.chars().take(EXCERPT_MAX_CHARS - 1).collect::<String>();
  excerpt = excerpt.trim().to_string();
  excerpt.push_str("...");
  excerpt
}

fn unique_strings(items: Vec<String>) -> Vec<String> {
  let mut seen = HashSet::new();
  items.into_iter().filter(|item| !item.is_empty() && seen.insert(item.clone())).collect()
}

struct NodeRow {
  id: String,
  node_type: String,
  title: String,
  preview: Option<String>,
  status: String,
  created_at: String,
  updated_at: String,
  authored_by_user: bool,
  body_version_id: String,
  version_number: i64,
  compiled_body: String,
  body_authored_by_user: bool,
}

struct EdgeRow {
  id: String,
  source_node_id: String,
  target_node_id: String,
  edge_type: String,
  bridge_text: Option<String>,
  status: String,
  authored_by_user: bool,
  created_at: String,
  updated_at: String,
}

struct GraphProvenance {
  evidence: Vec<Value>,
  source_chunk_ids: Vec<String>,
  markers: Vec<&'static str>,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn active_graph_startup_canvas_snapshot_is_bounded() {
    let conn = Connection::open_in_memory().unwrap();
    conn
      .execute_batch(
        r#"
            CREATE TABLE graph_nodes (
              id TEXT PRIMARY KEY,
              node_type TEXT NOT NULL,
              title TEXT NOT NULL,
              preview TEXT,
              current_body_version_id TEXT,
              status TEXT NOT NULL,
              authored_by_user INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL DEFAULT '2026-01-01T00:00:00Z',
              updated_at TEXT NOT NULL DEFAULT '2026-01-01T00:00:00Z'
            );
            CREATE TABLE node_body_versions (
              id TEXT PRIMARY KEY,
              node_id TEXT NOT NULL,
              version_number INTEGER NOT NULL,
              compiled_body TEXT NOT NULL,
              authored_by_user INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE graph_edges (
              id TEXT PRIMARY KEY,
              source_node_id TEXT NOT NULL,
              target_node_id TEXT NOT NULL,
              edge_type TEXT NOT NULL,
              bridge_text TEXT,
              status TEXT NOT NULL,
              authored_by_user INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL DEFAULT '2026-01-01T00:00:00Z',
              updated_at TEXT NOT NULL DEFAULT '2026-01-01T00:00:00Z'
            );
            CREATE TABLE graph_evidence (
              id TEXT PRIMARY KEY,
              entity_type TEXT NOT NULL,
              entity_id TEXT NOT NULL
            );
            CREATE TABLE graph_message_evidence (
              id TEXT PRIMARY KEY,
              target_entity_type TEXT NOT NULL,
              target_entity_id TEXT NOT NULL,
              graph_thread_message_id TEXT NOT NULL,
              quote_excerpt TEXT,
              created_at TEXT NOT NULL
            );
            CREATE TABLE node_message_evidence (
              id TEXT PRIMARY KEY,
              target_entity_type TEXT NOT NULL,
              target_entity_id TEXT NOT NULL,
              node_thread_message_id TEXT NOT NULL,
              quote_excerpt TEXT,
              created_at TEXT NOT NULL
            );
            "#,
      )
      .unwrap();

    for index in 0..(STARTUP_CANVAS_NODE_LIMIT + 5) {
      let id = format!("node_{index:03}");
      seed_node(
        &conn,
        &id,
        "concept",
        &format!("Node {index:03}"),
        "startup card",
        "active",
        "compiled body must not be loaded",
      );
    }
    for index in 0..(STARTUP_CANVAS_NODE_LIMIT + 4) {
      conn
        .execute(
          concat!(
            "INSERT INTO graph_edges (id, source_node_id, target_node_id, edge_type, bridge_text, status) ",
            "VALUES (?1, ?2, ?3, 'mentions', 'bridge', 'active')"
          ),
          params![format!("edge_{index:03}"), format!("node_{index:03}"), format!("node_{:03}", index + 1)],
        )
        .unwrap();
    }
    for source in 0..24 {
      for target in 0..24 {
        if source == target {
          continue;
        }
        conn
          .execute(
            concat!(
              "INSERT INTO graph_edges (id, source_node_id, target_node_id, edge_type, bridge_text, status) ",
              "VALUES (?1, ?2, ?3, 'mentions', 'dense bridge', 'active')"
            ),
            params![
              format!("dense_edge_{source:03}_{target:03}"),
              format!("node_{source:03}"),
              format!("node_{target:03}")
            ],
          )
          .unwrap();
      }
    }

    let snapshot = active_graph_startup_canvas_snapshot(&conn).unwrap();
    let nodes = snapshot["nodes"].as_array().unwrap();
    let edges = snapshot["edges"].as_array().unwrap();

    assert_eq!(nodes.len(), STARTUP_CANVAS_NODE_LIMIT);
    assert_eq!(edges.len(), STARTUP_CANVAS_EDGE_LIMIT);
    assert!(snapshot["is_partial"].as_bool().unwrap());
    assert_eq!(snapshot["node_limit"].as_u64().unwrap(), STARTUP_CANVAS_NODE_LIMIT as u64);
    assert_eq!(snapshot["edge_limit"].as_u64().unwrap(), STARTUP_CANVAS_EDGE_LIMIT as u64);
    assert_eq!(snapshot["total_node_count"].as_u64().unwrap(), (STARTUP_CANVAS_NODE_LIMIT + 5) as u64);
    assert!(snapshot["total_edge_count"].as_u64().unwrap() > STARTUP_CANVAS_EDGE_LIMIT as u64);
    assert!(nodes.iter().all(|node| node.get("compiled_body").is_none()));
    assert!(nodes.iter().any(|node| node["id"] == "node_159"));
    assert!(nodes.iter().all(|node| node["id"] != "node_160"));
    assert!(edges.iter().all(|edge| edge["source_node_id"] != "node_160" && edge["target_node_id"] != "node_160"));
  }

  fn seed_node(
    conn: &Connection,
    id: &str,
    node_type: &str,
    title: &str,
    preview: &str,
    status: &str,
    compiled_body: &str,
  ) {
    let body_id = format!("{id}_body");
    conn
      .execute(
        concat!(
          "INSERT INTO graph_nodes (id, node_type, title, preview, current_body_version_id, status) ",
          "VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
        ),
        params![id, node_type, title, preview, body_id, status],
      )
      .unwrap();
    conn
      .execute(
        "INSERT INTO node_body_versions (id, node_id, version_number, compiled_body) VALUES (?1, ?2, 1, ?3)",
        params![body_id, id, compiled_body],
      )
      .unwrap();
  }
}
