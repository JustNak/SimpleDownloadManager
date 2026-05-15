use super::*;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadUpdateBatch {
    pub jobs: Vec<DownloadJob>,
    pub removed_job_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressJobSnapshot {
    pub job: Option<DownloadJob>,
    pub settings: Settings,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchProgressSnapshot {
    pub context: Option<ProgressBatchContext>,
    pub jobs: Vec<DownloadJob>,
    pub settings: Settings,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub settings: Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationSoundKind {
    Success,
    Failed,
    Update,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSoundEvent {
    pub kind: NotificationSoundKind,
}

#[derive(Debug, Default)]
struct PendingDownloadUpdateBatch {
    jobs: HashMap<String, DownloadJob>,
    removed_job_ids: HashSet<String>,
    scheduled: bool,
}

static DOWNLOAD_UPDATE_BATCH: OnceLock<Mutex<PendingDownloadUpdateBatch>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalUseResult {
    pub paused_torrent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_reseed_retry_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressBatchKind {
    Multi,
    Bulk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressBatchContext {
    pub batch_id: String,
    pub kind: ProgressBatchKind,
    pub job_ids: Vec<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_items: Vec<FailedBatchItem>,
}

#[derive(Debug, Default)]
pub struct ProgressBatchRegistry {
    contexts: RwLock<HashMap<String, ProgressBatchContext>>,
}

impl ProgressBatchRegistry {
    pub fn store(&self, context: ProgressBatchContext) {
        if let Ok(mut contexts) = self.contexts.write() {
            contexts.insert(context.batch_id.clone(), context);
        }
    }

    pub fn get(&self, batch_id: &str) -> Option<ProgressBatchContext> {
        self.contexts
            .read()
            .ok()
            .and_then(|contexts| contexts.get(batch_id).cloned())
    }
}

pub fn emit_snapshot<R: Runtime>(app: &AppHandle<R>, snapshot: &DesktopSnapshot) {
    if let Err(error) = app.emit_to("main", STATE_CHANGED_EVENT, snapshot.clone()) {
        eprintln!("failed to emit state snapshot: {error}");
    }
    emit_popup_snapshots(app, snapshot);
}

pub fn emit_notification_sound<R: Runtime>(app: &AppHandle<R>, kind: NotificationSoundKind) {
    if let Err(error) = app.emit_to(
        "main",
        NOTIFICATION_SOUND_EVENT,
        NotificationSoundEvent { kind },
    ) {
        eprintln!("failed to emit notification sound event: {error}");
    }
}

pub fn emit_download_update<R: Runtime>(
    app: &AppHandle<R>,
    snapshot: &DesktopSnapshot,
    job_id: &str,
) {
    let job = snapshot.jobs.iter().find(|job| job.id == job_id).cloned();
    queue_download_update(app, job, Some(job_id));
    emit_popup_snapshots(app, snapshot);
}

pub fn emit_progress_delta<R: Runtime>(app: &AppHandle<R>, delta: ProgressDelta) {
    queue_download_update(app, Some(delta.job.clone()), None);
    emit_progress_delta_job_snapshots(app, &delta);
    emit_progress_delta_batch_snapshots(app, delta);
}

fn queue_download_update<R: Runtime>(
    app: &AppHandle<R>,
    job: Option<DownloadJob>,
    removed_job_id: Option<&str>,
) {
    let should_schedule = {
        let mut pending = pending_download_update_batch()
            .lock()
            .expect("download update batch lock poisoned");

        if let Some(job) = job {
            pending.removed_job_ids.remove(&job.id);
            pending.jobs.insert(job.id.clone(), job);
        } else if let Some(job_id) = removed_job_id {
            pending.jobs.remove(job_id);
            pending.removed_job_ids.insert(job_id.to_string());
        }

        if pending.scheduled {
            false
        } else {
            pending.scheduled = true;
            true
        }
    };

    if should_schedule {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(DOWNLOAD_UPDATE_BATCH_FLUSH_INTERVAL).await;
            flush_download_update_batch(&app);
        });
    }
}

fn pending_download_update_batch() -> &'static Mutex<PendingDownloadUpdateBatch> {
    DOWNLOAD_UPDATE_BATCH.get_or_init(|| Mutex::new(PendingDownloadUpdateBatch::default()))
}

fn flush_download_update_batch<R: Runtime>(app: &AppHandle<R>) {
    let payload = {
        let mut pending = pending_download_update_batch()
            .lock()
            .expect("download update batch lock poisoned");
        pending.scheduled = false;

        if pending.jobs.is_empty() && pending.removed_job_ids.is_empty() {
            return;
        }

        DownloadUpdateBatch {
            jobs: pending.jobs.drain().map(|(_, job)| job).collect(),
            removed_job_ids: pending.removed_job_ids.drain().collect(),
        }
    };

    if let Err(error) = app.emit_to("main", DOWNLOADS_UPDATE_BATCH_EVENT, payload) {
        eprintln!("failed to emit download update batch: {error}");
    }
}

fn emit_popup_snapshots<R: Runtime>(app: &AppHandle<R>, snapshot: &DesktopSnapshot) {
    emit_settings_snapshot(app, snapshot);
    emit_progress_job_snapshots(app, snapshot);
    emit_batch_progress_snapshots(app, snapshot);
}

fn emit_settings_snapshot<R: Runtime>(app: &AppHandle<R>, snapshot: &DesktopSnapshot) {
    if app.get_webview_window(DOWNLOAD_PROMPT_WINDOW).is_none() {
        return;
    }

    if let Err(error) = app.emit_to(
        DOWNLOAD_PROMPT_WINDOW,
        SETTINGS_SNAPSHOT_EVENT,
        SettingsSnapshot {
            settings: snapshot.settings.clone(),
        },
    ) {
        eprintln!("failed to emit settings snapshot: {error}");
    }
}

fn emit_progress_job_snapshots<R: Runtime>(app: &AppHandle<R>, snapshot: &DesktopSnapshot) {
    for label in app.webview_windows().keys() {
        if !is_progress_window_label(label) {
            continue;
        }

        let job = progress_job_for_window_label(snapshot, label);
        let payload = ProgressJobSnapshot {
            job,
            settings: snapshot.settings.clone(),
        };
        if let Err(error) = app.emit_to(label, PROGRESS_JOB_SNAPSHOT_EVENT, payload) {
            eprintln!("failed to emit progress job snapshot: {error}");
        }
    }
}

fn emit_batch_progress_snapshots<R: Runtime>(app: &AppHandle<R>, snapshot: &DesktopSnapshot) {
    let Some(registry) = app.try_state::<ProgressBatchRegistry>() else {
        return;
    };

    for label in app.webview_windows().keys() {
        let Some(batch_id) = label.strip_prefix("batch-progress-") else {
            continue;
        };
        let context = registry.get(batch_id);
        let jobs = filter_batch_jobs(snapshot, context.as_ref());
        let payload = BatchProgressSnapshot {
            context,
            jobs,
            settings: snapshot.settings.clone(),
        };
        if let Err(error) = app.emit_to(label, BATCH_PROGRESS_SNAPSHOT_EVENT, payload) {
            eprintln!("failed to emit batch progress snapshot: {error}");
        }
    }
}

fn emit_progress_delta_job_snapshots<R: Runtime>(app: &AppHandle<R>, delta: &ProgressDelta) {
    for label in app.webview_windows().keys() {
        if !progress_delta_matches_window_label(&delta.job, label) {
            continue;
        }

        let payload = ProgressJobSnapshot {
            job: Some(delta.job.clone()),
            settings: delta.settings.clone(),
        };
        if let Err(error) = app.emit_to(label, PROGRESS_JOB_SNAPSHOT_EVENT, payload) {
            eprintln!("failed to emit progress job snapshot: {error}");
        }
    }
}

fn emit_progress_delta_batch_snapshots<R: Runtime>(app: &AppHandle<R>, delta: ProgressDelta) {
    let Some(registry) = app.try_state::<ProgressBatchRegistry>() else {
        return;
    };
    let Some(state) = app.try_state::<SharedState>() else {
        return;
    };

    let targets = app
        .webview_windows()
        .keys()
        .filter_map(|label| {
            let batch_id = label.strip_prefix("batch-progress-")?;
            let context = registry.get(batch_id)?;
            context
                .job_ids
                .iter()
                .any(|job_id| job_id == &delta.job.id)
                .then(|| (label.clone(), context))
        })
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return;
    }

    let app = app.clone();
    let settings = delta.settings;
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        for (label, context) in targets {
            let jobs = state.batch_progress_jobs(&context.job_ids).await;
            let payload = BatchProgressSnapshot {
                context: Some(context),
                jobs,
                settings: settings.clone(),
            };
            if let Err(error) = app.emit_to(&label, BATCH_PROGRESS_SNAPSHOT_EVENT, payload) {
                eprintln!("failed to emit batch progress snapshot: {error}");
            }
        }
    });
}

fn is_progress_window_label(label: &str) -> bool {
    label.starts_with("download-progress-") || label.starts_with("torrent-progress-")
}

fn progress_delta_matches_window_label(job: &DownloadJob, label: &str) -> bool {
    if let Some(job_id) = label.strip_prefix("download-progress-") {
        return job.id == job_id && job.transfer_kind == TransferKind::Http;
    }

    if let Some(job_id) = label.strip_prefix("torrent-progress-") {
        return job.id == job_id && job.transfer_kind == TransferKind::Torrent;
    }

    false
}

fn progress_job_for_window_label(snapshot: &DesktopSnapshot, label: &str) -> Option<DownloadJob> {
    if let Some(job_id) = label.strip_prefix("download-progress-") {
        return snapshot
            .jobs
            .iter()
            .find(|job| job.id == job_id && job.transfer_kind == TransferKind::Http)
            .cloned();
    }

    if let Some(job_id) = label.strip_prefix("torrent-progress-") {
        return snapshot
            .jobs
            .iter()
            .find(|job| job.id == job_id && job.transfer_kind == TransferKind::Torrent)
            .cloned();
    }

    None
}

fn filter_batch_jobs(
    snapshot: &DesktopSnapshot,
    context: Option<&ProgressBatchContext>,
) -> Vec<DownloadJob> {
    let Some(context) = context else {
        return Vec::new();
    };
    let selected_ids = context
        .job_ids
        .iter()
        .collect::<std::collections::HashSet<_>>();
    snapshot
        .jobs
        .iter()
        .filter(|job| selected_ids.contains(&job.id))
        .cloned()
        .collect()
}
