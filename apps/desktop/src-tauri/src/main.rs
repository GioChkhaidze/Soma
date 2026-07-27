#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  #[cfg(any(target_os = "macos", target_os = "linux"))]
  let _ = fix_path_env::fix();

  soma_desktop_lib::run();
}
