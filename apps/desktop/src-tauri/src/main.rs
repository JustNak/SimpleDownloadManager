#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use simple_download_manager_desktop_backend::{
    commands, download, ipc, lifecycle, prompts, state, updates,
};
use tauri::Manager;

fn main() {
    let shared_state = match state::SharedState::new() {
        Ok(state) => state,
        Err(error) => {
            eprintln!("failed to initialize app state: {error}");
            std::process::exit(1);
        }
    };

    match lifecycle::apply_installer_launch_options_from_args(&shared_state, std::env::args()) {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("failed to apply installer startup options: {error}");
            std::process::exit(1);
        }
    }

    #[cfg(windows)]
    let _single_instance_guard = match lifecycle::acquire_single_instance_or_notify() {
        Ok(Some(guard)) => guard,
        Ok(None) => return,
        Err(error) => {
            eprintln!("failed to initialize single-instance guard: {error}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .plugin(
            tauri_plugin_updater::Builder::new()
                .installer_arg(lifecycle::POST_UPDATE_ARG)
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .on_window_event(lifecycle::handle_window_event)
        .setup(move |app| {
            let shared_state = shared_state.clone();
            let prompt_registry = prompts::PromptRegistry::default();
            let progress_batch_registry = commands::ProgressBatchRegistry::default();
            download::schedule_downloads(app.handle().clone(), shared_state.clone());
            commands::initialize_native_host_registration();
            ipc::start_named_pipe_listener(
                app.handle().clone(),
                shared_state.clone(),
                prompt_registry.clone(),
            );
            let state_for_lifecycle = shared_state.clone();
            app.manage(shared_state);
            app.manage(prompt_registry);
            app.manage(progress_batch_registry);
            app.manage(updates::PendingUpdateState::default());
            lifecycle::initialize_app_lifecycle(app, &state_for_lifecycle)?;
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
            commands::browse_torrent_file,
            commands::get_current_download_prompt,
            commands::confirm_download_prompt,
            commands::show_existing_download_prompt,
            commands::swap_download_prompt,
            commands::cancel_download_prompt,
            commands::open_progress_window,
            commands::open_batch_progress_window,
            commands::get_progress_batch_context,
            commands::open_job_file,
            commands::reveal_job_in_folder,
            commands::open_install_docs,
            commands::run_host_registration_fix,
            commands::test_extension_handoff,
            updates::check_for_update,
            updates::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri app");
}
