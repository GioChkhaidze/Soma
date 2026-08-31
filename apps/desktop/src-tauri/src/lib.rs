use tauri::Manager;

mod app_data_io;
mod brain_provider_registry;
mod brain_settings;
mod chat_cancellation;
mod chat_runtime;
mod chat_thread_store;
mod chat_turns;
mod commands;
mod contracts;
mod database;
mod error;
mod graph_read_model;
mod graph_write_model;
mod job_files;
mod jobs;
mod layout_state;
#[cfg(test)]
mod live_codex_e2e_tests;
mod repository;
mod retrieval;
mod retrieval_read_model;
mod review_read_model;
mod runtime_adapters;
mod secrets;
mod source_import;
mod workspace;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .on_window_event(|window, event| {
      if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
        chat_cancellation::cancel_all();
        window.app_handle().exit(0);
      }
    })
    .invoke_handler(tauri::generate_handler![
      commands::create_workspace_auto,
      commands::open_workspace_picker,
      commands::get_current_workspace,
      commands::get_current_workspace_with_stats,
      commands::get_brain_settings,
      commands::list_brain_models,
      commands::save_brain_settings,
      commands::authorize_codex_brain,
      commands::enable_codex_brain,
      commands::import_source_file,
      commands::compile_graph_workspace,
      commands::list_jobs,
      commands::clear_job_history,
      commands::open_job_folder,
      commands::run_compile_job,
      commands::import_graph_patch_for_review,
      commands::load_graph_canvas_snapshot,
      commands::load_workspace_bootstrap,
      commands::load_graph_node_detail,
      commands::search_graph_node_cards,
      commands::load_review_queue,
      commands::persist_node_position,
      commands::send_graph_chat_turn,
      commands::cancel_chat_turn,
      commands::list_graph_messages,
      commands::send_node_chat_turn,
      commands::list_node_messages,
      commands::update_node_body,
      commands::rollback_node_body,
      commands::undo_graph_patch,
      commands::accept_graph_proposal,
      commands::reject_graph_proposal,
      commands::defer_graph_proposal
    ])
    .run(tauri::generate_context!())
    .expect("error while running Soma desktop app");
}
