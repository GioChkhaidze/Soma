use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use crate::error::{CommandError, CommandResult};

static APP_DATA_WRITE_LOCK: Mutex<()> = Mutex::new(());

pub(crate) struct AppDataWriteGuard {
  _guard: MutexGuard<'static, ()>,
}

pub(crate) fn lock_app_data_writes() -> AppDataWriteGuard {
  let guard = APP_DATA_WRITE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
  AppDataWriteGuard { _guard: guard }
}

pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> CommandResult<()> {
  let guard = lock_app_data_writes();
  atomic_write_locked(&guard, path, contents)
}

pub(crate) fn atomic_write_locked(_guard: &AppDataWriteGuard, path: &Path, contents: &[u8]) -> CommandResult<()> {
  atomic_write_locked_with(_guard, path, contents, |from, to| fs::rename(from, to))
}

fn atomic_write_locked_with(
  _guard: &AppDataWriteGuard,
  path: &Path,
  contents: &[u8],
  publish: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> CommandResult<()> {
  let parent = path
    .parent()
    .filter(|parent| !parent.as_os_str().is_empty())
    .ok_or_else(|| CommandError::storage("App-data file must have a parent directory."))?;
  let file_name = path.file_name().ok_or_else(|| CommandError::storage("App-data file must have a file name."))?;
  fs::create_dir_all(parent)?;

  let mut temp_name = OsString::from(".");
  temp_name.push(file_name);
  temp_name.push(format!(".{}.tmp", uuid::Uuid::new_v4()));
  let temp_path = parent.join(temp_name);

  let result = (|| -> CommandResult<()> {
    let mut temp = OpenOptions::new().write(true).create_new(true).open(&temp_path)?;
    temp.write_all(contents)?;
    temp.sync_all()?;
    drop(temp);
    publish(&temp_path, path)?;
    Ok(())
  })();

  if result.is_err() {
    let _ = fs::remove_file(&temp_path);
  }
  result
}

#[cfg(test)]
mod tests {
  use std::sync::{Arc, Barrier};

  use super::*;

  #[test]
  fn concurrent_writes_publish_one_complete_value() {
    let root = std::env::temp_dir().join(format!("soma-app-data-write-test-{}", uuid::Uuid::new_v4()));
    let target = root.join("state.json");
    fs::create_dir_all(&root).unwrap();
    atomic_write(&target, b"initial value").unwrap();
    let payloads =
      Arc::new((0..12).map(|index| format!("value-{index}:{}", "x".repeat(16_384)).into_bytes()).collect::<Vec<_>>());
    let barrier = Arc::new(Barrier::new(payloads.len()));

    let threads: Vec<_> = (0..payloads.len())
      .map(|index| {
        let barrier = Arc::clone(&barrier);
        let payloads = Arc::clone(&payloads);
        let target = target.clone();
        std::thread::spawn(move || {
          barrier.wait();
          atomic_write(&target, &payloads[index]).unwrap();
        })
      })
      .collect();
    for thread in threads {
      thread.join().unwrap();
    }

    let published = fs::read(&target).unwrap();
    assert!(payloads.iter().any(|payload| payload == &published));
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn failed_publish_preserves_the_existing_file_and_removes_the_temporary_file() {
    let root = std::env::temp_dir().join(format!("soma-app-data-failure-test-{}", uuid::Uuid::new_v4()));
    let target = root.join("state.json");
    fs::create_dir_all(&root).unwrap();
    fs::write(&target, br#"{"state":"previous"}"#).unwrap();
    let guard = lock_app_data_writes();

    let result = atomic_write_locked_with(&guard, &target, br#"{"state":"replacement"}"#, |_, _| {
      Err(std::io::Error::other("injected failure before publish"))
    });
    drop(guard);

    assert!(result.is_err());
    let published: serde_json::Value = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
    assert_eq!(published, serde_json::json!({ "state": "previous" }));
    let entries: Vec<_> = fs::read_dir(&root).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(entries, vec![OsString::from("state.json")]);
    let _ = fs::remove_dir_all(root);
  }
}
