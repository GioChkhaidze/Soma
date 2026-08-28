use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use soma_ai_runtime::AgentTaskCancellation;

static ACTIVE_CHAT_TURNS: OnceLock<Mutex<HashMap<String, AgentTaskCancellation>>> = OnceLock::new();

pub fn begin(request_id: &str) -> AgentTaskCancellation {
  let cancellation = AgentTaskCancellation::new();
  active_turns()
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
    .insert(request_id.to_string(), cancellation.clone());
  cancellation
}

pub fn cancel(request_id: &str) -> bool {
  let cancellation = active_turns().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(request_id).cloned();
  if let Some(cancellation) = cancellation {
    cancellation.cancel();
    true
  } else {
    false
  }
}

pub fn cancel_all() -> usize {
  let cancellations: Vec<_> = {
    let mut turns = active_turns().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    turns.drain().map(|(_, cancellation)| cancellation).collect()
  };
  let count = cancellations.len();
  for cancellation in cancellations {
    cancellation.cancel();
  }
  count
}

pub fn finish(request_id: &str) {
  active_turns().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(request_id);
}

fn active_turns() -> &'static Mutex<HashMap<String, AgentTaskCancellation>> {
  ACTIVE_CHAT_TURNS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
mod tests {
  use super::{begin, cancel, cancel_all};

  #[test]
  fn cancel_all_stops_and_removes_every_active_turn() {
    cancel_all();
    let first = begin("shutdown-first");
    let second = begin("shutdown-second");
    assert_eq!(cancel_all(), 2);
    assert!(first.is_cancelled());
    assert!(second.is_cancelled());
    assert!(!cancel("shutdown-first"));
    assert!(!cancel("shutdown-second"));
  }
}
