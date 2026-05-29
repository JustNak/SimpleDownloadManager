use crate::state::SharedState;
use crate::storage::{Theme, TransferKind};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::utils::config::Color;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, Monitor, PhysicalPosition, PhysicalSize, Position,
    Runtime, Size, Theme as TauriTheme, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window,
    WindowEvent,
};
#[cfg(windows)]
use winreg::enums::HKEY_CURRENT_USER;
#[cfg(windows)]
use winreg::RegKey;

pub const DOWNLOAD_PROMPT_WINDOW: &str = "download-prompt";
pub const SELECT_JOB_EVENT: &str = "app://select-job";
const PROGRESS_WINDOW_PREFIX: &str = "download-progress-";
const TORRENT_PROGRESS_WINDOW_PREFIX: &str = "torrent-progress-";
const BATCH_PROGRESS_WINDOW_PREFIX: &str = "batch-progress-";
const PROGRESS_WINDOW_STACK_OFFSET: f64 = 28.0;
const POPUP_READY_TIMEOUT: Duration = Duration::from_millis(1500);
const DEFAULT_POPUP_ACCENT_COLOR: &str = "#3b82f6";

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

#[derive(Debug, Clone)]
struct PopupAppearanceQuery {
    theme: Theme,
    accent_color: String,
    native_theme: TauriTheme,
    background_color: Color,
}

pub async fn show_download_prompt_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(DOWNLOAD_PROMPT_WINDOW) {
        return show_existing_popup_window(&window, download_prompt_window_geometry());
    }

    let policy = download_prompt_window_policy();
    let geometry = download_prompt_window_geometry();
    let appearance = popup_window_appearance_query(app).await;
    let url = popup_window_url("download-prompt", &[], appearance.as_ref());
    let popup_init_script = popup_initialization_script(appearance.as_ref()).unwrap_or_default();

    let builder =
        WebviewWindowBuilder::new(app, DOWNLOAD_PROMPT_WINDOW, WebviewUrl::App(url.into()))
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
            .initialization_script(popup_init_script.clone())
            .visible(false);
    let builder = apply_popup_native_appearance(builder, appearance.as_ref());
    let window = builder.build().map_err(|error| error.to_string())?;
    schedule_popup_ready_timeout(&window, geometry);
    Ok(())
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

pub async fn show_progress_window(app: &AppHandle, job_id: &str) -> Result<(), String> {
    let label = progress_window_label(job_id);
    if let Some(window) = app.get_webview_window(&label) {
        return show_existing_popup_window(&window, progress_window_geometry());
    }

    let open_progress_windows = open_progress_popup_count(app);
    let prompt_position =
        current_download_prompt_position(app).or_else(last_download_prompt_position);
    let appearance = popup_window_appearance_query(app).await;
    let url = popup_window_url(
        "download-progress",
        &[("jobId", job_id)],
        appearance.as_ref(),
    );
    let popup_init_script = popup_initialization_script(appearance.as_ref()).unwrap_or_default();
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
        .always_on_top(policy.always_on_top)
        .initialization_script(popup_init_script.clone())
        .visible(false);
    let builder = apply_popup_native_appearance(builder, appearance.as_ref());

    let builder =
        if let Some(position) = progress_window_position(prompt_position, open_progress_windows) {
            builder.position(position.x, position.y)
        } else {
            builder.center()
        };

    let window = builder.build().map_err(|error| error.to_string())?;
    schedule_popup_ready_timeout(&window, geometry);
    Ok(())
}

pub async fn show_torrent_progress_window(app: &AppHandle, job_id: &str) -> Result<(), String> {
    let label = torrent_progress_window_label(job_id);
    if let Some(window) = app.get_webview_window(&label) {
        return show_existing_popup_window(&window, torrent_progress_window_geometry());
    }

    let appearance = popup_window_appearance_query(app).await;
    let url = popup_window_url(
        "torrent-progress",
        &[("jobId", job_id)],
        appearance.as_ref(),
    );
    let popup_init_script = popup_initialization_script(appearance.as_ref()).unwrap_or_default();
    let geometry = torrent_progress_window_geometry();
    let policy = progress_window_policy();

    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
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
        .initialization_script(popup_init_script.clone())
        .visible(false);
    let builder = apply_popup_native_appearance(builder, appearance.as_ref());
    let window = builder.build().map_err(|error| error.to_string())?;
    schedule_popup_ready_timeout(&window, geometry);
    Ok(())
}

pub async fn show_progress_window_for_transfer_kind(
    app: &AppHandle,
    job_id: &str,
    transfer_kind: TransferKind,
) -> Result<(), String> {
    match transfer_kind {
        TransferKind::Torrent => show_torrent_progress_window(app, job_id).await,
        TransferKind::Http => show_progress_window(app, job_id).await,
        TransferKind::BrowserAdopted => Ok(()),
    }
}

pub async fn show_batch_progress_window(app: &AppHandle, batch_id: &str) -> Result<(), String> {
    let label = batch_progress_window_label(batch_id);
    if let Some(window) = app.get_webview_window(&label) {
        return show_existing_popup_window(&window, batch_progress_window_geometry());
    }

    let open_progress_windows = open_progress_popup_count(app);
    let prompt_position =
        current_download_prompt_position(app).or_else(last_download_prompt_position);
    let appearance = popup_window_appearance_query(app).await;
    let url = popup_window_url(
        "batch-progress",
        &[("batchId", batch_id)],
        appearance.as_ref(),
    );
    let popup_init_script = popup_initialization_script(appearance.as_ref()).unwrap_or_default();
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
        .always_on_top(policy.always_on_top)
        .initialization_script(popup_init_script.clone())
        .visible(false);
    let builder = apply_popup_native_appearance(builder, appearance.as_ref());

    let builder =
        if let Some(position) = progress_window_position(prompt_position, open_progress_windows) {
            builder.position(position.x, position.y)
        } else {
            builder.center()
        };

    let window = builder.build().map_err(|error| error.to_string())?;
    schedule_popup_ready_timeout(&window, geometry);
    Ok(())
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

pub fn mark_popup_ready<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), String> {
    let Some(geometry) = popup_window_geometry_for_label(window.label()) else {
        return Ok(());
    };

    reveal_popup_window(window, geometry)
}

async fn popup_window_appearance_query(app: &AppHandle) -> Option<PopupAppearanceQuery> {
    let state = app
        .try_state::<SharedState>()
        .map(|state| state.inner().clone())?;
    let settings = state.settings().await;
    let native_theme = resolve_popup_native_theme(app, &settings.theme);
    let background_color = popup_background_color(&settings.theme, native_theme);
    Some(PopupAppearanceQuery {
        theme: settings.theme,
        accent_color: settings.accent_color,
        native_theme,
        background_color,
    })
}

fn apply_popup_native_appearance<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    appearance: Option<&PopupAppearanceQuery>,
) -> WebviewWindowBuilder<'a, R, M> {
    let Some(appearance) = appearance else {
        return builder;
    };

    builder
        .theme(Some(appearance.native_theme))
        .background_color(appearance.background_color)
}

fn resolve_popup_native_theme(app: &AppHandle, theme: &Theme) -> TauriTheme {
    match theme {
        Theme::Light => TauriTheme::Light,
        Theme::Dark | Theme::OledDark => TauriTheme::Dark,
        Theme::System => resolve_system_native_theme(app),
    }
}

fn resolve_system_native_theme(app: &AppHandle) -> TauriTheme {
    app.webview_windows()
        .values()
        .find_map(|window| window.theme().ok())
        .or_else(system_theme_from_registry)
        .unwrap_or(TauriTheme::Light)
}

#[cfg(windows)]
fn system_theme_from_registry() -> Option<TauriTheme> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        .ok()?;
    let uses_light_theme: u32 = key.get_value("AppsUseLightTheme").ok()?;
    Some(if uses_light_theme == 0 {
        TauriTheme::Dark
    } else {
        TauriTheme::Light
    })
}

#[cfg(not(windows))]
fn system_theme_from_registry() -> Option<TauriTheme> {
    None
}

fn popup_background_color(theme: &Theme, native_theme: TauriTheme) -> Color {
    match (theme, native_theme) {
        (Theme::OledDark, _) => Color(0, 0, 0, 255),
        (_, TauriTheme::Dark) => Color(18, 23, 31, 255),
        _ => Color(250, 251, 253, 255),
    }
}

fn popup_initialization_script(appearance: Option<&PopupAppearanceQuery>) -> Option<String> {
    let appearance = appearance?;
    let accent_color = normalize_popup_accent_color(&appearance.accent_color);
    let accent_foreground = popup_accent_foreground(&accent_color);
    let selected_color = format!("color-mix(in srgb, {accent_color} 16%, transparent)");
    let primary_soft = format!("color-mix(in srgb, {accent_color} 12%, transparent)");
    let is_dark = popup_theme_is_dark(&appearance.theme, appearance.native_theme);
    let is_oled_dark = matches!(appearance.theme, Theme::OledDark);

    Some(format!(
        "(function(){{try{{var root=document.documentElement;root.classList.toggle('dark',{is_dark});root.classList.toggle('oled-dark',{is_oled_dark});root.style.setProperty('--color-primary',{accent});root.style.setProperty('--color-accent',{accent});root.style.setProperty('--color-ring',{accent});root.style.setProperty('--color-selected',{selected});root.style.setProperty('--color-primary-soft',{primary_soft});root.style.setProperty('--color-primary-foreground',{foreground});root.style.setProperty('--color-accent-foreground',{foreground});}}catch(error){{console.error('Failed to apply popup appearance bootstrap.',error);}}}})();",
        is_dark = javascript_bool(is_dark),
        is_oled_dark = javascript_bool(is_oled_dark),
        accent = javascript_string_literal(&accent_color),
        selected = javascript_string_literal(&selected_color),
        primary_soft = javascript_string_literal(&primary_soft),
        foreground = javascript_string_literal(accent_foreground),
    ))
}

fn popup_theme_is_dark(theme: &Theme, native_theme: TauriTheme) -> bool {
    match theme {
        Theme::Light => false,
        Theme::Dark | Theme::OledDark => true,
        Theme::System => native_theme == TauriTheme::Dark,
    }
}

fn normalize_popup_accent_color(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() == 7
        && trimmed.starts_with('#')
        && trimmed
            .chars()
            .skip(1)
            .all(|character| character.is_ascii_hexdigit())
    {
        return trimmed.to_ascii_lowercase();
    }

    DEFAULT_POPUP_ACCENT_COLOR.to_string()
}

fn popup_accent_foreground(accent_color: &str) -> &'static str {
    let Some((red, green, blue)) = parse_rgb_hex_color(accent_color) else {
        return "#ffffff";
    };

    let luminance = 0.299 * f64::from(red) + 0.587 * f64::from(green) + 0.114 * f64::from(blue);
    if luminance > 160.0 {
        "#0f172a"
    } else {
        "#ffffff"
    }
}

fn parse_rgb_hex_color(value: &str) -> Option<(u8, u8, u8)> {
    if value.len() != 7 || !value.starts_with('#') {
        return None;
    }

    let red = u8::from_str_radix(&value[1..3], 16).ok()?;
    let green = u8::from_str_radix(&value[3..5], 16).ok()?;
    let blue = u8::from_str_radix(&value[5..7], 16).ok()?;
    Some((red, green, blue))
}

fn javascript_bool(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn javascript_string_literal(value: &str) -> String {
    match serde_json::to_string(value) {
        Ok(serialized) => serialized,
        Err(_) => "\"\"".to_string(),
    }
}

fn popup_window_url(
    window_mode: &str,
    params: &[(&str, &str)],
    appearance: Option<&PopupAppearanceQuery>,
) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("window", window_mode);
    for (name, value) in params {
        query.append_pair(name, value);
    }
    append_appearance_query_params(&mut query, appearance);
    format!("index.html?{}", query.finish())
}

fn append_appearance_query_params(
    query: &mut url::form_urlencoded::Serializer<'_, String>,
    appearance: Option<&PopupAppearanceQuery>,
) {
    let Some(appearance) = appearance else {
        return;
    };

    query.append_pair("theme", theme_query_value(&appearance.theme));
    query.append_pair(
        "accentColor",
        &normalize_popup_accent_color(&appearance.accent_color),
    );
}

fn theme_query_value(theme: &Theme) -> &'static str {
    match theme {
        Theme::Light => "light",
        Theme::Dark => "dark",
        Theme::OledDark => "oled_dark",
        Theme::System => "system",
    }
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

fn schedule_popup_ready_timeout<R: Runtime>(
    window: &WebviewWindow<R>,
    geometry: PopupWindowGeometry,
) {
    let window = window.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(POPUP_READY_TIMEOUT).await;
        match window.is_visible() {
            Ok(true) => return,
            Ok(false) => {}
            Err(error) => {
                eprintln!("failed to inspect popup visibility before timeout reveal: {error}")
            }
        }

        if let Err(error) = reveal_popup_window(&window, geometry) {
            eprintln!("failed to reveal popup window after readiness timeout: {error}");
        }
    });
}

fn reveal_popup_window<R: Runtime>(
    window: &WebviewWindow<R>,
    geometry: PopupWindowGeometry,
) -> Result<(), String> {
    let _ = window.unminimize();
    window.show().map_err(|error| error.to_string())?;
    repair_popup_window_bounds(window, geometry);
    if should_focus_ready_popup(window.label()) {
        window.set_focus().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn should_focus_ready_popup(label: &str) -> bool {
    label == DOWNLOAD_PROMPT_WINDOW
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

    #[test]
    fn popup_initialization_script_applies_dark_oled_and_accent() {
        let appearance = super::PopupAppearanceQuery {
            theme: crate::storage::Theme::OledDark,
            accent_color: "#06b6d4".to_string(),
            native_theme: tauri::Theme::Dark,
            background_color: tauri::utils::config::Color(0, 0, 0, 255),
        };

        let script = match super::popup_initialization_script(Some(&appearance)) {
            Some(script) => script,
            None => panic!("popup initialization script should be generated"),
        };

        assert!(script.contains("document.documentElement"));
        assert!(script.contains("classList.toggle('dark',true)"));
        assert!(script.contains("classList.toggle('oled-dark',true)"));
        assert!(script.contains("setProperty('--color-primary',\"#06b6d4\")"));
        assert!(script.contains(
            "setProperty('--color-selected',\"color-mix(in srgb, #06b6d4 16%, transparent)\")"
        ));
    }

    #[test]
    fn popup_initialization_script_sanitizes_accent_before_javascript() {
        let appearance = super::PopupAppearanceQuery {
            theme: crate::storage::Theme::System,
            accent_color: "#06b6d4';window.__bad=true;//".to_string(),
            native_theme: tauri::Theme::Dark,
            background_color: tauri::utils::config::Color(18, 23, 31, 255),
        };

        let script = match super::popup_initialization_script(Some(&appearance)) {
            Some(script) => script,
            None => panic!("popup initialization script should be generated"),
        };

        assert!(!script.contains("window.__bad"));
        assert!(script.contains("classList.toggle('dark',true)"));
        assert!(script.contains("classList.toggle('oled-dark',false)"));
        assert!(script.contains("setProperty('--color-primary',\"#3b82f6\")"));
    }
}
