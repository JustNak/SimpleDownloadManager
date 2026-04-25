#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use simple_download_manager_desktop_backend::{commands, download, ipc, prompts, state};
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let shared_state = state::SharedState::new()?;
            let prompt_registry = prompts::PromptRegistry::default();
            download::schedule_downloads(app.handle().clone(), shared_state.clone());
            ipc::start_named_pipe_listener(
                app.handle().clone(),
                shared_state.clone(),
                prompt_registry.clone(),
            );
            app.manage(shared_state);
            app.manage(prompt_registry);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_snapshot,
            commands::get_diagnostics,
            commands::export_diagnostics_report,
            commands::add_job,
            commands::add_jobs,
            commands::pause_job,
            commands::resume_job,
            commands::pause_all_jobs,
            commands::resume_all_jobs,
            commands::cancel_job,
            commands::retry_job,
            commands::restart_job,
            commands::retry_failed_jobs,
            commands::remove_job,
            commands::delete_job,
            commands::rename_job,
            commands::clear_completed_jobs,
            commands::save_settings,
            commands::browse_directory,
            commands::get_current_download_prompt,
            commands::confirm_download_prompt,
            commands::show_existing_download_prompt,
            commands::cancel_download_prompt,
            commands::open_progress_window,
            commands::open_job_file,
            commands::reveal_job_in_folder,
            commands::open_install_docs,
            commands::run_host_registration_fix,
            commands::test_extension_handoff,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri app");
}
