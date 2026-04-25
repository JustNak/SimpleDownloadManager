use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const DOWNLOAD_PROMPT_WINDOW: &str = "download-prompt";
pub const SELECT_JOB_EVENT: &str = "app://select-job";
const PROGRESS_WINDOW_PREFIX: &str = "download-progress-";
const BATCH_PROGRESS_WINDOW_PREFIX: &str = "batch-progress-";
const PROGRESS_WINDOW_STACK_OFFSET: f64 = 28.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct PopupWindowPosition {
    x: f64,
    y: f64,
}

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

pub fn close_download_prompt_window(app: &AppHandle, remember_position: bool) {
    if let Some(window) = app.get_webview_window(DOWNLOAD_PROMPT_WINDOW) {
        if remember_position {
            if let Ok(position) = window.outer_position() {
                remember_download_prompt_position(PopupWindowPosition {
                    x: position.x as f64,
                    y: position.y as f64,
                });
            }
        }
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

    let open_progress_windows = open_progress_popup_count(app);
    let prompt_position =
        current_download_prompt_position(app).or_else(last_download_prompt_position);
    let url = format!("index.html?window=download-progress&jobId={job_id}");
    let geometry = progress_window_geometry();

    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title("Download progress")
        .inner_size(geometry.width, geometry.height)
        .min_inner_size(geometry.min_width, geometry.min_height)
        .max_inner_size(geometry.width, geometry.height)
        .resizable(false)
        .maximizable(false)
        .decorations(false)
        .always_on_top(true);

    let builder =
        if let Some(position) = progress_window_position(prompt_position, open_progress_windows) {
            builder.position(position.x, position.y)
        } else {
            builder.center()
        };

    builder
        .build()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn show_batch_progress_window(app: &AppHandle, batch_id: &str) -> Result<(), String> {
    let label = batch_progress_window_label(batch_id);
    if let Some(window) = app.get_webview_window(&label) {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }

    let open_progress_windows = open_progress_popup_count(app);
    let prompt_position =
        current_download_prompt_position(app).or_else(last_download_prompt_position);
    let url = format!("index.html?window=batch-progress&batchId={batch_id}");
    let geometry = batch_progress_window_geometry();

    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title("Batch progress")
        .inner_size(geometry.width, geometry.height)
        .min_inner_size(geometry.min_width, geometry.min_height)
        .max_inner_size(geometry.width, geometry.height)
        .resizable(false)
        .maximizable(false)
        .decorations(false)
        .always_on_top(true);

    let builder =
        if let Some(position) = progress_window_position(prompt_position, open_progress_windows) {
            builder.position(position.x, position.y)
        } else {
            builder.center()
        };

    builder
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

fn batch_progress_window_label(batch_id: &str) -> String {
    let safe_batch_id: String = batch_id
        .chars()
        .filter(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_'))
        .collect();
    format!("{BATCH_PROGRESS_WINDOW_PREFIX}{safe_batch_id}")
}

fn progress_window_geometry() -> ProgressWindowGeometry {
    ProgressWindowGeometry {
        width: 500.0,
        height: 360.0,
        min_width: 500.0,
        min_height: 360.0,
    }
}

fn batch_progress_window_geometry() -> ProgressWindowGeometry {
    ProgressWindowGeometry {
        width: 560.0,
        height: 430.0,
        min_width: 560.0,
        min_height: 430.0,
    }
}

fn open_progress_popup_count(app: &AppHandle) -> usize {
    app.webview_windows()
        .keys()
        .filter(|label| {
            label.starts_with(PROGRESS_WINDOW_PREFIX)
                || label.starts_with(BATCH_PROGRESS_WINDOW_PREFIX)
        })
        .count()
}

fn progress_window_position(
    prompt_position: Option<PopupWindowPosition>,
    open_progress_windows: usize,
) -> Option<PopupWindowPosition> {
    let prompt_position = prompt_position?;
    let offset = (open_progress_windows.min(8) as f64) * PROGRESS_WINDOW_STACK_OFFSET;

    Some(PopupWindowPosition {
        x: prompt_position.x + offset,
        y: prompt_position.y + offset,
    })
}

fn current_download_prompt_position(app: &AppHandle) -> Option<PopupWindowPosition> {
    app.get_webview_window(DOWNLOAD_PROMPT_WINDOW)
        .and_then(|window| window.outer_position().ok())
        .map(|position| PopupWindowPosition {
            x: position.x as f64,
            y: position.y as f64,
        })
}

fn last_download_prompt_position() -> Option<PopupWindowPosition> {
    remembered_download_prompt_position()
        .lock()
        .ok()
        .and_then(|position| *position)
}

fn remember_download_prompt_position(position: PopupWindowPosition) {
    if let Ok(mut remembered_position) = remembered_download_prompt_position().lock() {
        *remembered_position = Some(position);
    }
}

fn remembered_download_prompt_position() -> &'static Mutex<Option<PopupWindowPosition>> {
    static POSITION: OnceLock<Mutex<Option<PopupWindowPosition>>> = OnceLock::new();
    POSITION.get_or_init(|| Mutex::new(None))
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

    #[test]
    fn progress_window_position_uses_prompt_position_with_stack_offset() {
        let prompt_position = super::PopupWindowPosition { x: 280.0, y: 221.0 };

        assert_eq!(
            super::progress_window_position(Some(prompt_position), 0),
            Some(prompt_position)
        );
        assert_eq!(
            super::progress_window_position(Some(prompt_position), 2),
            Some(super::PopupWindowPosition { x: 336.0, y: 277.0 })
        );
    }

    #[test]
    fn batch_progress_window_label_is_sanitized_and_stable() {
        assert_eq!(
            super::batch_progress_window_label("batch:one/../two?bad"),
            "batch-progress-batchonetwobad"
        );
    }

    #[test]
    fn batch_progress_window_minimum_matches_content_size() {
        let geometry = super::batch_progress_window_geometry();

        assert_eq!(geometry.width, 560.0);
        assert_eq!(geometry.height, 430.0);
        assert_eq!(geometry.min_width, geometry.width);
        assert_eq!(geometry.min_height, geometry.height);
    }
}
