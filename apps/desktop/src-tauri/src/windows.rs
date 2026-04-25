use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const DOWNLOAD_PROMPT_WINDOW: &str = "download-prompt";
pub const SELECT_JOB_EVENT: &str = "app://select-job";
const PROGRESS_WINDOW_PREFIX: &str = "download-progress-";

struct ProgressWindowGeometry {
    width: f64,
    height: f64,
    min_width: f64,
    min_height: f64,
}

pub fn show_download_prompt_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(DOWNLOAD_PROMPT_WINDOW) {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }

    WebviewWindowBuilder::new(
        app,
        DOWNLOAD_PROMPT_WINDOW,
        WebviewUrl::App("index.html?window=download-prompt".into()),
    )
    .title("New download detected")
    .inner_size(540.0, 430.0)
    .min_inner_size(540.0, 430.0)
    .max_inner_size(540.0, 430.0)
    .resizable(false)
    .maximizable(false)
    .decorations(false)
    .always_on_top(true)
    .center()
    .build()
    .map(|_| ())
    .map_err(|error| error.to_string())
}

pub fn close_download_prompt_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(DOWNLOAD_PROMPT_WINDOW) {
        let _ = window.close();
    }
}

pub fn show_progress_window(app: &AppHandle, job_id: &str) -> Result<(), String> {
    let label = progress_window_label(job_id);
    if let Some(window) = app.get_webview_window(&label) {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }

    let open_progress_windows = app
        .webview_windows()
        .keys()
        .filter(|label| label.starts_with(PROGRESS_WINDOW_PREFIX))
        .count();
    let offset = (open_progress_windows.min(8) as f64) * 28.0;
    let url = format!("index.html?window=download-progress&jobId={job_id}");
    let geometry = progress_window_geometry();

    WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title("Download progress")
        .inner_size(geometry.width, geometry.height)
        .min_inner_size(geometry.min_width, geometry.min_height)
        .max_inner_size(geometry.width, geometry.height)
        .resizable(false)
        .maximizable(false)
        .decorations(false)
        .always_on_top(true)
        .position(340.0 + offset, 150.0 + offset)
        .build()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn focus_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_skip_taskbar(false);
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn focus_job_in_main_window(app: &AppHandle, job_id: &str) {
    focus_main_window(app);
    let _ = app.emit_to("main", SELECT_JOB_EVENT, job_id);
}

fn progress_window_label(job_id: &str) -> String {
    let safe_job_id: String = job_id
        .chars()
        .filter(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | ':' | '/'))
        .collect();
    format!("{PROGRESS_WINDOW_PREFIX}{safe_job_id}")
}

fn progress_window_geometry() -> ProgressWindowGeometry {
    ProgressWindowGeometry {
        width: 500.0,
        height: 360.0,
        min_width: 500.0,
        min_height: 360.0,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn progress_window_minimum_matches_content_size() {
        let geometry = super::progress_window_geometry();

        assert_eq!(geometry.width, 500.0);
        assert_eq!(geometry.height, 360.0);
        assert_eq!(geometry.min_width, geometry.width);
        assert_eq!(geometry.min_height, geometry.height);
    }
}
