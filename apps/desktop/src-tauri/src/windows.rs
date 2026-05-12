use crate::storage::TransferKind;
use std::sync::{Mutex, OnceLock};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, Monitor, PhysicalPosition, PhysicalSize, Position,
    Runtime, Size, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window, WindowEvent,
};

pub const DOWNLOAD_PROMPT_WINDOW: &str = "download-prompt";
pub const SELECT_JOB_EVENT: &str = "app://select-job";
const PROGRESS_WINDOW_PREFIX: &str = "download-progress-";
const TORRENT_PROGRESS_WINDOW_PREFIX: &str = "torrent-progress-";
const BATCH_PROGRESS_WINDOW_PREFIX: &str = "batch-progress-";
const PROGRESS_WINDOW_STACK_OFFSET: f64 = 28.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct PopupWindowPosition {
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PopupWindowGeometry {
    width: f64,
    height: f64,
    min_width: f64,
    min_height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MonitorRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DownloadPromptWindowPolicy {
    minimizable: bool,
    always_on_top: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProgressWindowPolicy {
    minimizable: bool,
    always_on_top: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopupRestoreFocus {
    Preserve,
}

pub fn show_download_prompt_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(DOWNLOAD_PROMPT_WINDOW) {
        return show_existing_popup_window(&window, download_prompt_window_geometry());
    }

    let policy = download_prompt_window_policy();
    let geometry = download_prompt_window_geometry();

    WebviewWindowBuilder::new(
        app,
        DOWNLOAD_PROMPT_WINDOW,
        WebviewUrl::App("index.html?window=download-prompt".into()),
    )
    .title("New download detected")
    .inner_size(geometry.width, geometry.height)
    .min_inner_size(geometry.min_width, geometry.min_height)
    .max_inner_size(geometry.width, geometry.height)
    .resizable(false)
    .minimizable(policy.minimizable)
    .maximizable(false)
    .decorations(false)
    .always_on_top(policy.always_on_top)
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
        return show_existing_popup_window(&window, progress_window_geometry());
    }

    let open_progress_windows = open_progress_popup_count(app);
    let prompt_position =
        current_download_prompt_position(app).or_else(last_download_prompt_position);
    let url = format!("index.html?window=download-progress&jobId={job_id}");
    let geometry = progress_window_geometry();
    let policy = progress_window_policy();

    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title("Download progress")
        .inner_size(geometry.width, geometry.height)
        .min_inner_size(geometry.min_width, geometry.min_height)
        .max_inner_size(geometry.width, geometry.height)
        .resizable(false)
        .minimizable(policy.minimizable)
        .maximizable(false)
        .decorations(false)
        .always_on_top(policy.always_on_top);

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

pub fn show_torrent_progress_window(app: &AppHandle, job_id: &str) -> Result<(), String> {
    let label = torrent_progress_window_label(job_id);
    if let Some(window) = app.get_webview_window(&label) {
        return show_existing_popup_window(&window, torrent_progress_window_geometry());
    }

    let url = format!("index.html?window=torrent-progress&jobId={job_id}");
    let geometry = torrent_progress_window_geometry();
    let policy = progress_window_policy();

    WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title("Torrent session")
        .inner_size(geometry.width, geometry.height)
        .min_inner_size(geometry.min_width, geometry.min_height)
        .max_inner_size(geometry.width, geometry.height)
        .resizable(false)
        .minimizable(policy.minimizable)
        .maximizable(false)
        .decorations(false)
        .always_on_top(policy.always_on_top)
        .center()
        .build()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn show_progress_window_for_transfer_kind(
    app: &AppHandle,
    job_id: &str,
    transfer_kind: TransferKind,
) -> Result<(), String> {
    match transfer_kind {
        TransferKind::Torrent => show_torrent_progress_window(app, job_id),
        TransferKind::Http => show_progress_window(app, job_id),
    }
}

pub fn show_batch_progress_window(app: &AppHandle, batch_id: &str) -> Result<(), String> {
    let label = batch_progress_window_label(batch_id);
    if let Some(window) = app.get_webview_window(&label) {
        return show_existing_popup_window(&window, batch_progress_window_geometry());
    }

    let open_progress_windows = open_progress_popup_count(app);
    let prompt_position =
        current_download_prompt_position(app).or_else(last_download_prompt_position);
    let url = format!("index.html?window=batch-progress&batchId={batch_id}");
    let geometry = batch_progress_window_geometry();
    let policy = progress_window_policy();

    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title("Batch progress")
        .inner_size(geometry.width, geometry.height)
        .min_inner_size(geometry.min_width, geometry.min_height)
        .max_inner_size(geometry.width, geometry.height)
        .resizable(false)
        .minimizable(policy.minimizable)
        .maximizable(false)
        .decorations(false)
        .always_on_top(policy.always_on_top);

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
    if let Err(error) = crate::lifecycle::show_main_window(app) {
        eprintln!("failed to focus main window: {error}");
    }
}

pub async fn focus_main_window_async(app: &AppHandle) {
    if let Err(error) = crate::lifecycle::show_main_window_async(app).await {
        eprintln!("failed to focus main window: {error}");
    }
}

pub fn focus_job_in_main_window(app: &AppHandle, job_id: &str) {
    let main_window_exists = app.get_webview_window("main").is_some();
    if let Err(error) = crate::lifecycle::show_main_window_with_selected_job(app, job_id) {
        eprintln!("failed to focus main window for job: {error}");
        return;
    }
    if main_window_exists {
        let _ = app.emit_to("main", SELECT_JOB_EVENT, job_id);
    }
}

pub async fn focus_job_in_main_window_async(app: &AppHandle, job_id: &str) {
    let main_window_exists = app.get_webview_window("main").is_some();
    if let Err(error) =
        crate::lifecycle::show_main_window_with_selected_job_async(app, job_id).await
    {
        eprintln!("failed to focus main window for job: {error}");
        return;
    }
    if main_window_exists {
        let _ = app.emit_to("main", SELECT_JOB_EVENT, job_id);
    }
}

pub fn reset_popup_windows<R: Runtime>(app: &AppHandle<R>) {
    for (label, window) in app.webview_windows() {
        if let Some(geometry) = popup_window_geometry_for_label(&label) {
            repair_existing_popup_window(&window, geometry, PopupRestoreFocus::Preserve);
        }
    }
}

pub fn handle_popup_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    let WindowEvent::Focused(true) = event else {
        return;
    };

    let label = window.label();
    let Some(geometry) = popup_window_geometry_for_label(label) else {
        return;
    };
    let Some(webview_window) = window.app_handle().get_webview_window(label) else {
        return;
    };

    repair_existing_popup_window(&webview_window, geometry, PopupRestoreFocus::Preserve);
}

pub fn progress_window_label(job_id: &str) -> String {
    let safe_job_id: String = job_id
        .chars()
        .filter(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | ':' | '/'))
        .collect();
    format!("{PROGRESS_WINDOW_PREFIX}{safe_job_id}")
}

pub fn batch_progress_window_label(batch_id: &str) -> String {
    let safe_batch_id: String = batch_id
        .chars()
        .filter(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_'))
        .collect();
    format!("{BATCH_PROGRESS_WINDOW_PREFIX}{safe_batch_id}")
}

pub fn torrent_progress_window_label(job_id: &str) -> String {
    let safe_job_id: String = job_id
        .chars()
        .filter(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | ':' | '/'))
        .collect();
    format!("{TORRENT_PROGRESS_WINDOW_PREFIX}{safe_job_id}")
}

fn download_prompt_window_geometry() -> PopupWindowGeometry {
    PopupWindowGeometry {
        width: 460.0,
        height: 280.0,
        min_width: 460.0,
        min_height: 280.0,
    }
}

fn progress_window_geometry() -> PopupWindowGeometry {
    PopupWindowGeometry {
        width: 460.0,
        height: 280.0,
        min_width: 460.0,
        min_height: 280.0,
    }
}

fn torrent_progress_window_geometry() -> PopupWindowGeometry {
    PopupWindowGeometry {
        width: 720.0,
        height: 520.0,
        min_width: 720.0,
        min_height: 520.0,
    }
}

fn batch_progress_window_geometry() -> PopupWindowGeometry {
    PopupWindowGeometry {
        width: 640.0,
        height: 480.0,
        min_width: 640.0,
        min_height: 480.0,
    }
}

fn download_prompt_window_policy() -> DownloadPromptWindowPolicy {
    DownloadPromptWindowPolicy {
        minimizable: true,
        always_on_top: true,
    }
}

fn progress_window_policy() -> ProgressWindowPolicy {
    ProgressWindowPolicy {
        minimizable: true,
        always_on_top: false,
    }
}

fn show_existing_popup_window(
    window: &WebviewWindow,
    geometry: PopupWindowGeometry,
) -> Result<(), String> {
    let _ = window.unminimize();
    window.show().map_err(|error| error.to_string())?;
    repair_popup_window_bounds(window, geometry);
    window.set_focus().map_err(|error| error.to_string())
}

fn repair_existing_popup_window<R: Runtime>(
    window: &WebviewWindow<R>,
    geometry: PopupWindowGeometry,
    _focus: PopupRestoreFocus,
) {
    let _ = window.unminimize();
    if let Err(error) = window.show() {
        eprintln!("failed to show popup window during repair: {error}");
        return;
    }
    repair_popup_window_bounds(window, geometry);
}

fn repair_popup_window_bounds<R: Runtime>(
    window: &WebviewWindow<R>,
    geometry: PopupWindowGeometry,
) {
    if let Err(error) = window.set_size(Size::Logical(LogicalSize::new(
        geometry.width,
        geometry.height,
    ))) {
        eprintln!("failed to reset popup window size: {error}");
    }

    let size = popup_outer_size(window, geometry);
    if size.width == 0 || size.height == 0 {
        return;
    }

    let monitors = match window.available_monitors() {
        Ok(monitors) => monitors,
        Err(error) => {
            eprintln!("failed to inspect monitors for popup repair: {error}");
            return;
        }
    };
    if monitors.is_empty() {
        return;
    }

    let position = window.outer_position().ok();
    let rect = position.map(|position| WindowRect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    });

    if rect
        .map(|rect| popup_rect_is_visible_on_any_monitor(rect, &monitors))
        .unwrap_or(false)
    {
        return;
    }

    let Some(monitor) = preferred_popup_monitor(window, &monitors) else {
        return;
    };
    let _ = window.set_position(Position::Physical(centered_popup_position(size, monitor)));
}

fn popup_outer_size<R: Runtime>(
    window: &WebviewWindow<R>,
    geometry: PopupWindowGeometry,
) -> PhysicalSize<u32> {
    window.outer_size().unwrap_or_else(|_| {
        let scale_factor = window.scale_factor().unwrap_or(1.0);
        PhysicalSize::new(
            (geometry.width * scale_factor).round().max(1.0) as u32,
            (geometry.height * scale_factor).round().max(1.0) as u32,
        )
    })
}

fn preferred_popup_monitor<R: Runtime>(
    window: &WebviewWindow<R>,
    monitors: &[Monitor],
) -> Option<MonitorRect> {
    if let Ok(Some(monitor)) = window.current_monitor() {
        return Some(MonitorRect::from_monitor(&monitor));
    }
    if let Ok(Some(monitor)) = window.primary_monitor() {
        return Some(MonitorRect::from_monitor(&monitor));
    }
    monitors.first().map(MonitorRect::from_monitor)
}

fn popup_window_geometry_for_label(label: &str) -> Option<PopupWindowGeometry> {
    if label == DOWNLOAD_PROMPT_WINDOW {
        return Some(download_prompt_window_geometry());
    }
    if label.starts_with(PROGRESS_WINDOW_PREFIX) {
        return Some(progress_window_geometry());
    }
    if label.starts_with(TORRENT_PROGRESS_WINDOW_PREFIX) {
        return Some(torrent_progress_window_geometry());
    }
    if label.starts_with(BATCH_PROGRESS_WINDOW_PREFIX) {
        return Some(batch_progress_window_geometry());
    }
    None
}

fn popup_rect_is_visible_on_any_monitor(rect: WindowRect, monitors: &[Monitor]) -> bool {
    monitors
        .iter()
        .map(MonitorRect::from_monitor)
        .any(|monitor| popup_rect_is_visible_on_monitor(rect, monitor))
}

fn popup_rect_is_visible_on_monitor(rect: WindowRect, monitor: MonitorRect) -> bool {
    const MIN_VISIBLE_WIDTH: i64 = 80;
    const MIN_VISIBLE_HEIGHT: i64 = 44;

    let visible_width = intersection_length(rect.x, rect.width, monitor.x, monitor.width);
    let visible_height = intersection_length(rect.y, rect.height, monitor.y, monitor.height);
    visible_width >= MIN_VISIBLE_WIDTH.min(rect.width as i64)
        && visible_height >= MIN_VISIBLE_HEIGHT.min(rect.height as i64)
}

fn intersection_length(a_start: i32, a_length: u32, b_start: i32, b_length: u32) -> i64 {
    let a_start = a_start as i64;
    let b_start = b_start as i64;
    let a_end = a_start + a_length as i64;
    let b_end = b_start + b_length as i64;
    (a_end.min(b_end) - a_start.max(b_start)).max(0)
}

fn centered_popup_position(size: PhysicalSize<u32>, monitor: MonitorRect) -> PhysicalPosition<i32> {
    let x = monitor.x as i64 + ((monitor.width as i64 - size.width as i64) / 2).max(0);
    let y = monitor.y as i64 + ((monitor.height as i64 - size.height as i64) / 2).max(0);
    PhysicalPosition::new(clamp_i64_to_i32(x), clamp_i64_to_i32(y))
}

fn clamp_i64_to_i32(value: i64) -> i32 {
    value.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

impl MonitorRect {
    fn from_monitor(monitor: &Monitor) -> Self {
        let position = monitor.position();
        let size = monitor.size();
        Self {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        }
    }
}

fn open_progress_popup_count(app: &AppHandle) -> usize {
    app.webview_windows()
        .keys()
        .filter(|label| {
            label.starts_with(PROGRESS_WINDOW_PREFIX)
                || label.starts_with(TORRENT_PROGRESS_WINDOW_PREFIX)
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

        assert_eq!(geometry.width, 460.0);
        assert_eq!(geometry.height, 280.0);
        assert_eq!(geometry.min_width, geometry.width);
        assert_eq!(geometry.min_height, geometry.height);
    }

    #[test]
    fn download_prompt_window_minimum_matches_content_size() {
        let geometry = super::download_prompt_window_geometry();

        assert_eq!(geometry.width, 460.0);
        assert_eq!(geometry.height, 280.0);
        assert_eq!(geometry.min_width, geometry.width);
        assert_eq!(geometry.min_height, geometry.height);
    }

    #[test]
    fn progress_windows_are_minimizable_and_not_always_on_top() {
        let policy = super::progress_window_policy();

        assert!(policy.minimizable);
        assert!(!policy.always_on_top);
    }

    #[test]
    fn download_prompt_window_is_minimizable_and_always_on_top() {
        let policy = super::download_prompt_window_policy();

        assert!(policy.minimizable);
        assert!(policy.always_on_top);
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
    fn popup_rect_requires_meaningful_visible_area() {
        let monitor = super::MonitorRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };

        assert!(super::popup_rect_is_visible_on_monitor(
            super::WindowRect {
                x: 100,
                y: 100,
                width: 460,
                height: 280,
            },
            monitor,
        ));
        assert!(!super::popup_rect_is_visible_on_monitor(
            super::WindowRect {
                x: -450,
                y: 100,
                width: 460,
                height: 280,
            },
            monitor,
        ));
    }

    #[test]
    fn centered_popup_position_uses_monitor_bounds() {
        assert_eq!(
            super::centered_popup_position(
                tauri::PhysicalSize::new(460, 280),
                super::MonitorRect {
                    x: -1920,
                    y: 120,
                    width: 1920,
                    height: 1080,
                },
            ),
            tauri::PhysicalPosition::new(-1190, 520)
        );
    }

    #[test]
    fn popup_label_maps_to_restore_geometry() {
        assert_eq!(
            super::popup_window_geometry_for_label(super::DOWNLOAD_PROMPT_WINDOW),
            Some(super::download_prompt_window_geometry())
        );
        assert_eq!(
            super::popup_window_geometry_for_label("download-progress-job-1"),
            Some(super::progress_window_geometry())
        );
        assert_eq!(
            super::popup_window_geometry_for_label("torrent-progress-job-1"),
            Some(super::torrent_progress_window_geometry())
        );
        assert_eq!(
            super::popup_window_geometry_for_label("batch-progress-batch-1"),
            Some(super::batch_progress_window_geometry())
        );
        assert_eq!(super::popup_window_geometry_for_label("main"), None);
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

        assert_eq!(geometry.width, 640.0);
        assert_eq!(geometry.height, 480.0);
        assert_eq!(geometry.min_width, geometry.width);
        assert_eq!(geometry.min_height, geometry.height);
    }

    #[test]
    fn torrent_progress_window_label_is_sanitized_and_stable() {
        assert_eq!(
            super::torrent_progress_window_label("job:one/../two?bad"),
            "torrent-progress-job:one//twobad"
        );
    }

    #[test]
    fn torrent_progress_window_minimum_matches_content_size() {
        let geometry = super::torrent_progress_window_geometry();

        assert_eq!(geometry.width, 720.0);
        assert_eq!(geometry.height, 520.0);
        assert_eq!(geometry.min_width, geometry.width);
        assert_eq!(geometry.min_height, geometry.height);
    }

    #[test]
    fn batch_progress_windows_are_minimizable_and_not_always_on_top() {
        let policy = super::progress_window_policy();

        assert!(policy.minimizable);
        assert!(!policy.always_on_top);
    }
}
