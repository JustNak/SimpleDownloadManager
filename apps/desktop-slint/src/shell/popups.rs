use crate::controller::{
    empty_batch_details, empty_progress_details, slint_batch_details_from_context,
    slint_progress_details_from_job, slint_prompt_details_from_prompt, waiting_prompt_details,
};
use crate::{BatchProgressWindow, DownloadPromptWindow, HttpProgressWindow, TorrentProgressWindow};
use simple_download_manager_desktop_core::contracts::ProgressBatchContext;
use simple_download_manager_desktop_core::storage::{
    DesktopSnapshot, DownloadPrompt, TransferKind,
};
use slint::{
    CloseRequestResponse, ComponentHandle, PhysicalPosition, PhysicalSize, WindowPosition,
    WindowSize as SlintWindowSize,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::{WindowRole, WindowSize};

const PROGRESS_WINDOW_PREFIX: &str = "download-progress-";
const TORRENT_PROGRESS_WINDOW_PREFIX: &str = "torrent-progress-";
const BATCH_PROGRESS_WINDOW_PREFIX: &str = "batch-progress-";
const PROGRESS_WINDOW_STACK_OFFSET: i32 = 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupWindowConfig {
    pub title: &'static str,
    pub size: WindowSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupWindowPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PopupLifecycleState {
    last_prompt_position: Option<PopupWindowPosition>,
}

impl PopupLifecycleState {
    pub fn close_prompt(&mut self, remember_position: bool, position: Option<PopupWindowPosition>) {
        if remember_position {
            self.last_prompt_position = position;
        }
    }

    pub fn last_prompt_position(&self) -> Option<PopupWindowPosition> {
        self.last_prompt_position
    }
}

#[derive(Clone, Default)]
pub struct PendingSelectedJob {
    value: Arc<Mutex<Option<String>>>,
}

impl PendingSelectedJob {
    pub fn queue(&self, id: String) {
        if let Ok(mut value) = self.value.lock() {
            *value = Some(id);
        }
    }

    pub fn take(&self) -> Option<String> {
        self.value.lock().ok().and_then(|mut value| value.take())
    }
}

#[derive(Default)]
pub struct PopupRegistry {
    prompt: Option<DownloadPromptWindow>,
    current_prompt: Option<DownloadPrompt>,
    http_progress: HashMap<String, HttpProgressWindow>,
    torrent_progress: HashMap<String, TorrentProgressWindow>,
    batch_progress: HashMap<String, BatchProgressWindow>,
    batch_contexts: HashMap<String, ProgressBatchContext>,
    snapshot: Option<DesktopSnapshot>,
    lifecycle: PopupLifecycleState,
}

thread_local! {
    static POPUP_REGISTRY: RefCell<PopupRegistry> = RefCell::new(PopupRegistry::default());
}

pub fn with_popup_registry<R>(operation: impl FnOnce(&mut PopupRegistry) -> R) -> R {
    POPUP_REGISTRY.with(|registry| operation(&mut registry.borrow_mut()))
}

pub fn popup_window_config(role: WindowRole) -> PopupWindowConfig {
    let title = match role {
        WindowRole::Main => "Simple Download Manager",
        WindowRole::DownloadPrompt => "New download detected",
        WindowRole::HttpProgress => "Download progress",
        WindowRole::TorrentProgress => "Torrent session",
        WindowRole::BatchProgress => "Batch progress",
    };

    PopupWindowConfig {
        title,
        size: role.default_size(),
    }
}

pub fn progress_window_label(job_id: &str) -> String {
    format!(
        "{PROGRESS_WINDOW_PREFIX}{}",
        sanitize_progress_identifier(job_id)
    )
}

pub fn torrent_progress_window_label(job_id: &str) -> String {
    format!(
        "{TORRENT_PROGRESS_WINDOW_PREFIX}{}",
        sanitize_progress_identifier(job_id)
    )
}

pub fn batch_progress_window_label(batch_id: &str) -> String {
    format!(
        "{BATCH_PROGRESS_WINDOW_PREFIX}{}",
        sanitize_batch_identifier(batch_id)
    )
}

pub fn progress_window_position(
    prompt_position: Option<PopupWindowPosition>,
    open_progress_windows: usize,
) -> Option<PopupWindowPosition> {
    let prompt_position = prompt_position?;
    let offset = open_progress_windows.min(8) as i32 * PROGRESS_WINDOW_STACK_OFFSET;

    Some(PopupWindowPosition {
        x: prompt_position.x + offset,
        y: prompt_position.y + offset,
    })
}

impl PopupRegistry {
    pub fn apply_snapshot(&mut self, snapshot: &DesktopSnapshot) {
        self.snapshot = Some(snapshot.clone());
        self.refresh_open_progress_windows();
    }

    pub fn set_download_prompt(&mut self, prompt: Option<DownloadPrompt>) -> Result<(), String> {
        self.current_prompt = prompt;
        if self.current_prompt.is_some() {
            self.show_download_prompt_window()
        } else {
            self.close_download_prompt_window(false)
        }
    }

    pub fn show_download_prompt_window(&mut self) -> Result<(), String> {
        let details = self
            .current_prompt
            .as_ref()
            .map(slint_prompt_details_from_prompt)
            .unwrap_or_else(waiting_prompt_details);
        self.ensure_download_prompt_window()?;
        let window = self
            .prompt
            .as_ref()
            .expect("prompt window should exist after ensure");
        window.set_prompt(details);
        show_component(window)
    }

    pub fn close_download_prompt_window(&mut self, remember_position: bool) -> Result<(), String> {
        let position = self
            .prompt
            .as_ref()
            .map(|window| popup_position(window.window()));
        self.lifecycle.close_prompt(remember_position, position);
        if let Some(window) = &self.prompt {
            window.hide().map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub fn show_progress_window(
        &mut self,
        id: String,
        transfer_kind: TransferKind,
    ) -> Result<(), String> {
        match transfer_kind {
            TransferKind::Http => self.show_http_progress_window(id),
            TransferKind::Torrent => self.show_torrent_progress_window(id),
        }
    }

    pub fn show_batch_progress_window(
        &mut self,
        batch_id: String,
        context: Option<ProgressBatchContext>,
    ) -> Result<(), String> {
        if let Some(context) = context {
            self.batch_contexts.insert(batch_id.clone(), context);
        }
        let details = self.batch_details(&batch_id);
        if !self.batch_progress.contains_key(&batch_id) {
            let window = BatchProgressWindow::new().map_err(|error| error.to_string())?;
            configure_component_window(&window, WindowRole::BatchProgress);
            install_hide_on_close(&window);
            if let Some(position) = self.next_progress_position() {
                set_component_position(&window, position);
            }
            self.batch_progress.insert(batch_id.clone(), window);
        }
        let window = self
            .batch_progress
            .get(&batch_id)
            .expect("batch progress window should exist after insert");
        window.set_details(details);
        show_component(window)
    }

    fn show_http_progress_window(&mut self, id: String) -> Result<(), String> {
        let details =
            self.progress_details(&id, popup_window_config(WindowRole::HttpProgress).title);
        if !self.http_progress.contains_key(&id) {
            let window = HttpProgressWindow::new().map_err(|error| error.to_string())?;
            configure_component_window(&window, WindowRole::HttpProgress);
            install_hide_on_close(&window);
            if let Some(position) = self.next_progress_position() {
                set_component_position(&window, position);
            }
            self.http_progress.insert(id.clone(), window);
        }
        let window = self
            .http_progress
            .get(&id)
            .expect("HTTP progress window should exist after insert");
        window.set_details(details);
        show_component(window)
    }

    fn show_torrent_progress_window(&mut self, id: String) -> Result<(), String> {
        let details =
            self.progress_details(&id, popup_window_config(WindowRole::TorrentProgress).title);
        if !self.torrent_progress.contains_key(&id) {
            let window = TorrentProgressWindow::new().map_err(|error| error.to_string())?;
            configure_component_window(&window, WindowRole::TorrentProgress);
            install_hide_on_close(&window);
            if let Some(position) = self.next_progress_position() {
                set_component_position(&window, position);
            }
            self.torrent_progress.insert(id.clone(), window);
        }
        let window = self
            .torrent_progress
            .get(&id)
            .expect("torrent progress window should exist after insert");
        window.set_details(details);
        show_component(window)
    }

    fn ensure_download_prompt_window(&mut self) -> Result<(), String> {
        if self.prompt.is_some() {
            return Ok(());
        }

        let window = DownloadPromptWindow::new().map_err(|error| error.to_string())?;
        configure_component_window(&window, WindowRole::DownloadPrompt);
        install_hide_on_close(&window);
        self.prompt = Some(window);
        Ok(())
    }

    fn refresh_open_progress_windows(&self) {
        for (id, window) in &self.http_progress {
            window.set_details(
                self.progress_details(id, popup_window_config(WindowRole::HttpProgress).title),
            );
        }
        for (id, window) in &self.torrent_progress {
            window.set_details(
                self.progress_details(id, popup_window_config(WindowRole::TorrentProgress).title),
            );
        }
        for (batch_id, window) in &self.batch_progress {
            window.set_details(self.batch_details(batch_id));
        }
    }

    fn progress_details(&self, id: &str, title: &str) -> crate::ProgressDetails {
        self.snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.jobs.iter().find(|job| job.id == id))
            .map(|job| slint_progress_details_from_job(job, title))
            .unwrap_or_else(|| empty_progress_details(id, title))
    }

    fn batch_details(&self, batch_id: &str) -> crate::BatchDetails {
        self.batch_contexts
            .get(batch_id)
            .and_then(|context| {
                self.snapshot
                    .as_ref()
                    .map(|snapshot| slint_batch_details_from_context(context, snapshot))
            })
            .unwrap_or_else(|| empty_batch_details(batch_id))
    }

    fn next_progress_position(&self) -> Option<PopupWindowPosition> {
        progress_window_position(
            self.lifecycle.last_prompt_position(),
            self.open_progress_popup_count(),
        )
    }

    fn open_progress_popup_count(&self) -> usize {
        self.http_progress.len() + self.torrent_progress.len() + self.batch_progress.len()
    }
}

fn configure_component_window(component: &impl ComponentHandle, role: WindowRole) {
    let size = popup_window_config(role).size;
    component
        .window()
        .set_size(SlintWindowSize::Physical(PhysicalSize::new(
            size.width,
            size.height,
        )));
}

fn install_hide_on_close(component: &impl ComponentHandle) {
    component
        .window()
        .on_close_requested(|| CloseRequestResponse::HideWindow);
}

fn show_component(component: &impl ComponentHandle) -> Result<(), String> {
    component.window().set_minimized(false);
    component.show().map_err(|error| error.to_string())
}

fn set_component_position(component: &impl ComponentHandle, position: PopupWindowPosition) {
    component
        .window()
        .set_position(WindowPosition::Physical(PhysicalPosition::new(
            position.x, position.y,
        )));
}

fn popup_position(window: &slint::Window) -> PopupWindowPosition {
    let position = window.position();
    PopupWindowPosition {
        x: position.x,
        y: position.y,
    }
}

fn sanitize_progress_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | ':' | '/')
        })
        .collect()
}

fn sanitize_batch_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .collect()
}
