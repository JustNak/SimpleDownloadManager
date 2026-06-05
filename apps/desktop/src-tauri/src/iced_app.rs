use crate::state::SharedState;
use crate::storage::{
    BulkArchiveStatus, ConnectionState, DesktopSnapshot, DownloadJob, JobState, RemovalState,
    TransferKind,
};
use std::time::Duration;

const ICED_DESKTOP_TITLE: &str = "Simple Download Manager";
const SNAPSHOT_REFRESH_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub enum IcedDesktopMessage {
    NoOp,
    RefreshSnapshot,
    SnapshotLoaded(DesktopSnapshot),
    RunQueueAction(IcedQueueAction),
    QueueActionFinished(Result<DesktopSnapshot, String>),
}

#[derive(Clone)]
pub struct IcedDesktopShell {
    state: Option<SharedState>,
    snapshot: DesktopSnapshot,
    queue_action_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IcedQueueRow {
    pub filename: String,
    pub status_label: String,
    pub progress_text: String,
    pub progress_value: f32,
    pub speed_text: String,
    pub time_text: String,
    pub size_text: String,
    pub primary_action: Option<IcedQueueAction>,
    pub cancel_action: Option<IcedQueueAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IcedQueueAction {
    Pause { job_id: String },
    Resume { job_id: String },
    Retry { job_id: String },
    Cancel { job_id: String },
}

impl IcedQueueRow {
    pub fn primary_action_label(&self) -> Option<&'static str> {
        self.primary_action.as_ref().map(IcedQueueAction::label)
    }
}

impl IcedQueueAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pause { .. } => "Pause",
            Self::Resume { .. } => "Resume",
            Self::Retry { .. } => "Retry",
            Self::Cancel { .. } => "Cancel",
        }
    }
}

impl IcedDesktopShell {
    pub fn new(snapshot: DesktopSnapshot) -> Self {
        Self {
            state: None,
            snapshot,
            queue_action_error: None,
        }
    }

    pub fn with_state(state: SharedState, snapshot: DesktopSnapshot) -> Self {
        Self {
            state: Some(state),
            snapshot,
            queue_action_error: None,
        }
    }

    pub fn title(&self) -> &'static str {
        ICED_DESKTOP_TITLE
    }

    pub fn connection_text(&self) -> &'static str {
        match self.snapshot.connection_state {
            ConnectionState::Checking => "Checking",
            ConnectionState::Connected => "Connected",
            ConnectionState::HostMissing => "Host missing",
            ConnectionState::AppMissing => "App missing",
            ConnectionState::AppUnreachable => "App unreachable",
            ConnectionState::Error => "Error",
        }
    }

    pub fn queue_summary_text(&self) -> String {
        let total = self.snapshot.jobs.len();
        let active = self
            .snapshot
            .jobs
            .iter()
            .filter(|job| {
                matches!(
                    job.state,
                    JobState::Queued
                        | JobState::Starting
                        | JobState::Downloading
                        | JobState::Seeding
                        | JobState::Paused
                )
            })
            .count();
        let failed = self
            .snapshot
            .jobs
            .iter()
            .filter(|job| job.state == JobState::Failed)
            .count();
        let completed = self
            .snapshot
            .jobs
            .iter()
            .filter(|job| matches!(job.state, JobState::Completed | JobState::Canceled))
            .count();

        format!("{total} total, {active} active, {failed} failed, {completed} completed")
    }

    pub fn startup_recovery_text(&self) -> Option<&str> {
        self.snapshot
            .startup_recovery
            .as_ref()
            .map(|summary| summary.message.as_str())
    }

    pub fn queue_action_error_text(&self) -> Option<&str> {
        self.queue_action_error.as_deref()
    }

    pub fn queue_rows(&self) -> Vec<IcedQueueRow> {
        self.snapshot
            .jobs
            .iter()
            .map(|job| {
                let (primary_action, cancel_action) = queue_actions(job);
                IcedQueueRow {
                    filename: display_filename(job),
                    status_label: queue_status_label(job).to_string(),
                    progress_text: format_progress(job.progress),
                    progress_value: clamp_progress(job.progress) as f32,
                    speed_text: format_queue_speed(job),
                    time_text: format_queue_time(job),
                    size_text: format_queue_size(job),
                    primary_action,
                    cancel_action,
                }
            })
            .collect()
    }

    pub fn subscription(&self) -> iced::Subscription<IcedDesktopMessage> {
        if self.state.is_none() {
            return iced::Subscription::none();
        }

        iced::time::every(SNAPSHOT_REFRESH_INTERVAL).map(|_| IcedDesktopMessage::RefreshSnapshot)
    }

    pub fn update(&mut self, message: IcedDesktopMessage) -> iced::Task<IcedDesktopMessage> {
        match message {
            IcedDesktopMessage::NoOp => iced::Task::none(),
            IcedDesktopMessage::RefreshSnapshot => self
                .state
                .clone()
                .map(|state| {
                    iced::Task::perform(load_snapshot(state), IcedDesktopMessage::SnapshotLoaded)
                })
                .unwrap_or_else(iced::Task::none),
            IcedDesktopMessage::SnapshotLoaded(snapshot) => {
                self.snapshot = snapshot;
                iced::Task::none()
            }
            IcedDesktopMessage::RunQueueAction(action) => self
                .state
                .clone()
                .map(|state| {
                    self.queue_action_error = None;
                    iced::Task::perform(
                        run_queue_action(state, action),
                        IcedDesktopMessage::QueueActionFinished,
                    )
                })
                .unwrap_or_else(iced::Task::none),
            IcedDesktopMessage::QueueActionFinished(Ok(snapshot)) => {
                self.snapshot = snapshot;
                self.queue_action_error = None;
                iced::Task::none()
            }
            IcedDesktopMessage::QueueActionFinished(Err(error)) => {
                self.queue_action_error = Some(error);
                iced::Task::none()
            }
        }
    }

    pub fn view(&self) -> iced::Element<'_, IcedDesktopMessage> {
        use iced::widget::{column, container, text};
        use iced::Length;

        let mut content = column![
            text(ICED_DESKTOP_TITLE).size(28),
            text(self.connection_text()).size(16),
            text(self.queue_summary_text()).size(16),
        ]
        .spacing(10);

        if let Some(message) = self.startup_recovery_text() {
            content = content.push(text(message).size(14));
        }

        if let Some(error) = self.queue_action_error_text() {
            content = content.push(text(error).size(14));
        }

        content = content.push(queue_view(self.queue_rows()));

        container(content)
            .padding(24)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

async fn load_snapshot(state: SharedState) -> DesktopSnapshot {
    state.snapshot().await
}

async fn run_queue_action(
    state: SharedState,
    action: IcedQueueAction,
) -> Result<DesktopSnapshot, String> {
    match action {
        IcedQueueAction::Pause { job_id } => state.pause_job(&job_id).await,
        IcedQueueAction::Resume { job_id } => state.resume_job(&job_id).await,
        IcedQueueAction::Retry { job_id } => state.retry_job(&job_id).await,
        IcedQueueAction::Cancel { job_id } => state.cancel_job(&job_id).await,
    }
    .map_err(|error| error.message)
}

fn queue_view(rows: Vec<IcedQueueRow>) -> iced::Element<'static, IcedDesktopMessage> {
    use iced::widget::{column, container, progress_bar, row, scrollable, text};
    use iced::Length;

    if rows.is_empty() {
        return container(
            column![
                text("No downloads").size(20),
                text("Downloads from the browser extension or the New Download command will appear in this list.")
                    .size(14),
            ]
            .spacing(8),
        )
        .padding(24)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();
    }

    let header = row![
        text("Name").width(Length::FillPortion(4)),
        text("Status").width(Length::FillPortion(2)),
        text("Speed").width(Length::FillPortion(2)),
        text("Time").width(Length::FillPortion(2)),
        text("Size").width(Length::FillPortion(3)),
        text("Actions").width(Length::FillPortion(2)),
    ]
    .spacing(12);

    let mut queue = column![header].spacing(8);

    for queue_row in rows {
        let name = column![
            text(queue_row.filename).size(14),
            progress_bar(0.0..=100.0, queue_row.progress_value)
        ]
        .spacing(4)
        .width(Length::FillPortion(4));

        queue = queue.push(
            row![
                name,
                text(queue_row.status_label).width(Length::FillPortion(2)),
                text(queue_row.speed_text).width(Length::FillPortion(2)),
                text(queue_row.time_text).width(Length::FillPortion(2)),
                text(queue_row.size_text).width(Length::FillPortion(3)),
                action_buttons(queue_row.primary_action, queue_row.cancel_action)
                    .width(Length::FillPortion(2)),
            ]
            .spacing(12),
        );
    }

    scrollable(queue).height(Length::Fill).into()
}

fn action_buttons(
    primary_action: Option<IcedQueueAction>,
    cancel_action: Option<IcedQueueAction>,
) -> iced::widget::Row<'static, IcedDesktopMessage> {
    use iced::widget::{button, row, text};

    let mut actions = row![].spacing(6);

    if let Some(action) = primary_action {
        actions = actions.push(
            button(text(action.label())).on_press(IcedDesktopMessage::RunQueueAction(action)),
        );
    }

    if let Some(action) = cancel_action {
        actions = actions.push(
            button(text(action.label())).on_press(IcedDesktopMessage::RunQueueAction(action)),
        );
    }

    actions
}

fn display_filename(job: &DownloadJob) -> String {
    let filename = job.filename.trim();
    if !filename.is_empty() {
        return filename.to_string();
    }

    if let Some(name) = job
        .torrent
        .as_ref()
        .and_then(|torrent| torrent.name.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        return name.to_string();
    }

    "Metadata pending".into()
}

fn queue_actions(job: &DownloadJob) -> (Option<IcedQueueAction>, Option<IcedQueueAction>) {
    if job.removal_state.is_some() {
        return (None, None);
    }

    let job_id = job.id.clone();
    match job.state {
        JobState::Queued | JobState::Starting | JobState::Downloading | JobState::Seeding => (
            Some(IcedQueueAction::Pause {
                job_id: job_id.clone(),
            }),
            Some(IcedQueueAction::Cancel { job_id }),
        ),
        JobState::Paused => (
            Some(IcedQueueAction::Resume {
                job_id: job_id.clone(),
            }),
            Some(IcedQueueAction::Cancel { job_id }),
        ),
        JobState::Failed | JobState::Canceled => (Some(IcedQueueAction::Retry { job_id }), None),
        JobState::Completed => (None, None),
    }
}

fn queue_status_label(job: &DownloadJob) -> &'static str {
    if job.removal_state == Some(RemovalState::Removing) {
        return "Removing";
    }

    if job.removal_state == Some(RemovalState::CleanupFailed) {
        return "Cleanup failed";
    }

    if let Some(archive) = &job.bulk_archive {
        match archive.archive_status {
            BulkArchiveStatus::Failed => return "Folder failed",
            BulkArchiveStatus::Compressing => return "Finalizing",
            BulkArchiveStatus::Combining | BulkArchiveStatus::CreatingFolder => {
                return "Combining";
            }
            BulkArchiveStatus::Extracting => return "Uncompressing",
            BulkArchiveStatus::Pending | BulkArchiveStatus::Completed => {}
        }
    }

    match job.state {
        JobState::Seeding => "Seeding",
        JobState::Completed if job.transfer_kind == TransferKind::BrowserAdopted => "Browser file",
        JobState::Completed => "Done",
        JobState::Failed => "Error",
        JobState::Queued => "Queued",
        JobState::Paused => "Paused",
        JobState::Canceled => "Canceled",
        JobState::Starting | JobState::Downloading => "Downloading",
    }
}

fn format_progress(progress: f64) -> String {
    format!("{:.0}%", clamp_progress(progress))
}

fn clamp_progress(progress: f64) -> f64 {
    if !progress.is_finite() {
        return 0.0;
    }

    progress.clamp(0.0, 100.0)
}

fn format_queue_speed(job: &DownloadJob) -> String {
    if job.state == JobState::Downloading {
        return format!("{}/s", format_bytes(job.speed));
    }

    if job.state == JobState::Seeding && job.torrent.is_some() {
        return format!(
            "Up {}",
            format_bytes(
                job.torrent
                    .as_ref()
                    .map(|torrent| torrent.uploaded_bytes)
                    .unwrap_or(0)
            )
        );
    }

    "--".into()
}

fn format_queue_time(job: &DownloadJob) -> String {
    if job.state == JobState::Downloading {
        return format_time(job.eta);
    }

    if job.state == JobState::Seeding && job.torrent.is_some() {
        return format_torrent_ratio(job);
    }

    "--".into()
}

fn format_torrent_ratio(job: &DownloadJob) -> String {
    let Some(ratio) = job.torrent.as_ref().map(|torrent| torrent.ratio) else {
        return "--".into();
    };

    if !ratio.is_finite() || ratio <= 0.0 {
        return "--".into();
    }

    format!("{ratio:.2}x")
}

fn format_queue_size(job: &DownloadJob) -> String {
    if job.total_bytes == 0
        && job.state == JobState::Queued
        && job.transfer_kind != TransferKind::Torrent
        && job.resolved_from_url.is_some()
        && job.bulk_archive.is_some()
    {
        return "Waiting".into();
    }

    if job.total_bytes == 0 {
        return format_bytes(job.downloaded_bytes);
    }

    if job.transfer_kind == TransferKind::Torrent || job.state == JobState::Completed {
        return format_bytes(job.total_bytes);
    }

    format!(
        "{} / {}",
        format_bytes(job.downloaded_bytes),
        format_bytes(job.total_bytes)
    )
}

fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".into();
    }

    let units = ["B", "KB", "MB", "GB", "TB"];
    let value = bytes as f64;
    let index = ((value.log2() / 10.0).floor() as usize).min(units.len() - 1);
    let amount = value / 1024_f64.powi(index as i32);

    if amount >= 10.0 || index == 0 {
        format!("{amount:.0} {}", units[index])
    } else {
        format!("{amount:.1} {}", units[index])
    }
}

fn format_time(seconds: u64) -> String {
    if seconds == 0 {
        return "--".into();
    }

    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let remaining_seconds = seconds % 60;

    if hours > 0 {
        return format!("{hours}h {minutes}m");
    }

    if minutes > 0 {
        return format!("{minutes}m {remaining_seconds}s");
    }

    format!("{remaining_seconds}s")
}

pub fn run_iced_desktop(state: SharedState, snapshot: DesktopSnapshot) -> iced::Result {
    iced::application(
        move || IcedDesktopShell::with_state(state.clone(), snapshot.clone()),
        IcedDesktopShell::update,
        IcedDesktopShell::view,
    )
    .title(|shell: &IcedDesktopShell| shell.title().to_string())
    .subscription(IcedDesktopShell::subscription)
    .window_size([960.0, 640.0])
    .centered()
    .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{
        BulkArchiveInfo, BulkArchiveOutputKind, BulkArchiveStatus, ConnectionState,
        DesktopSnapshot, DownloadJob, JobState, Settings, StartupRecoveryStatus,
        StartupRecoverySummary, TransferKind,
    };

    #[test]
    fn iced_shell_model_describes_snapshot() {
        let shell = IcedDesktopShell::new(snapshot(
            vec![
                job("queued", JobState::Queued),
                job("downloading", JobState::Downloading),
                job("completed", JobState::Completed),
                job("failed", JobState::Failed),
            ],
            Some(StartupRecoverySummary {
                status: StartupRecoveryStatus::Recovered,
                message: "Recovered previous queue.".into(),
                source_path: None,
                quarantined_path: None,
            }),
        ));

        assert_eq!(shell.title(), "Simple Download Manager");
        assert_eq!(shell.connection_text(), "Connected");
        assert_eq!(
            shell.queue_summary_text(),
            "4 total, 2 active, 1 failed, 1 completed"
        );
        assert_eq!(
            shell.startup_recovery_text(),
            Some("Recovered previous queue.")
        );
    }

    #[test]
    fn iced_queue_rows_match_existing_queue_presentation() {
        let mut active = job("downloading", JobState::Downloading);
        active.progress = 42.4;
        active.downloaded_bytes = 512 * 1024;
        active.total_bytes = 1024 * 1024;
        active.speed = 2 * 1024 * 1024;
        active.eta = 75;

        let mut browser_file = job("browser", JobState::Completed);
        browser_file.transfer_kind = TransferKind::BrowserAdopted;
        browser_file.progress = 100.0;
        browser_file.downloaded_bytes = 4 * 1024 * 1024;
        browser_file.total_bytes = 4 * 1024 * 1024;

        let mut torrent = job("torrent", JobState::Seeding);
        torrent.transfer_kind = TransferKind::Torrent;
        torrent.torrent = Some(Default::default());
        torrent.total_bytes = 8 * 1024 * 1024;
        torrent.downloaded_bytes = 8 * 1024 * 1024;
        torrent.speed = 4096;

        let mut waiting_bulk = job("bulk", JobState::Queued);
        waiting_bulk.resolved_from_url = Some("https://hoster.example/file".into());
        waiting_bulk.bulk_archive = Some(BulkArchiveInfo {
            id: "bulk_1".into(),
            name: "bulk-download.zip".into(),
            output_kind: BulkArchiveOutputKind::Archive,
            archive_status: BulkArchiveStatus::Pending,
            requires_extraction: None,
            output_path: None,
            error: None,
            warning: None,
            finalize_total_bytes: None,
            finalize_processed_bytes: None,
            finalize_mode: None,
        });

        let shell = IcedDesktopShell::new(snapshot(
            vec![active, browser_file, torrent, waiting_bulk],
            None,
        ));

        let rows = shell.queue_rows();

        assert_eq!(rows[0].filename, "downloading.bin");
        assert_eq!(rows[0].status_label, "Downloading");
        assert_eq!(rows[0].progress_text, "42%");
        assert_eq!(rows[0].speed_text, "2.0 MB/s");
        assert_eq!(rows[0].time_text, "1m 15s");
        assert_eq!(rows[0].size_text, "512 KB / 1.0 MB");

        assert_eq!(rows[1].status_label, "Browser file");
        assert_eq!(rows[1].progress_text, "100%");
        assert_eq!(rows[1].speed_text, "--");
        assert_eq!(rows[1].time_text, "--");
        assert_eq!(rows[1].size_text, "4.0 MB");

        assert_eq!(rows[2].status_label, "Seeding");
        assert_eq!(rows[2].speed_text, "Up 0 B");
        assert_eq!(rows[2].time_text, "--");
        assert_eq!(rows[2].size_text, "8.0 MB");

        assert_eq!(rows[3].status_label, "Queued");
        assert_eq!(rows[3].size_text, "Waiting");
    }

    #[test]
    fn iced_shell_replaces_snapshot_when_loaded() {
        let mut shell = IcedDesktopShell::new(snapshot(vec![job("old", JobState::Queued)], None));

        let _ = shell.update(IcedDesktopMessage::SnapshotLoaded(snapshot(
            vec![
                job("new-a", JobState::Downloading),
                job("new-b", JobState::Completed),
            ],
            None,
        )));

        let rows = shell.queue_rows();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].filename, "new-a.bin");
        assert_eq!(rows[1].filename, "new-b.bin");
    }

    #[test]
    fn iced_queue_rows_expose_primary_actions_for_mutable_states() {
        let shell = IcedDesktopShell::new(snapshot(
            vec![
                job("queued", JobState::Queued),
                job("starting", JobState::Starting),
                job("downloading", JobState::Downloading),
                job("seeding", JobState::Seeding),
                job("paused", JobState::Paused),
                job("failed", JobState::Failed),
                job("canceled", JobState::Canceled),
                job("completed", JobState::Completed),
            ],
            None,
        ));

        let rows = shell.queue_rows();

        assert_eq!(
            rows[0].primary_action,
            Some(IcedQueueAction::Pause {
                job_id: "queued".into()
            })
        );
        assert_eq!(rows[0].primary_action_label(), Some("Pause"));
        assert_eq!(
            rows[1].primary_action,
            Some(IcedQueueAction::Pause {
                job_id: "starting".into()
            })
        );
        assert_eq!(
            rows[2].primary_action,
            Some(IcedQueueAction::Pause {
                job_id: "downloading".into()
            })
        );
        assert_eq!(
            rows[3].primary_action,
            Some(IcedQueueAction::Pause {
                job_id: "seeding".into()
            })
        );
        assert_eq!(
            rows[4].primary_action,
            Some(IcedQueueAction::Resume {
                job_id: "paused".into()
            })
        );
        assert_eq!(rows[4].primary_action_label(), Some("Resume"));
        assert_eq!(
            rows[5].primary_action,
            Some(IcedQueueAction::Retry {
                job_id: "failed".into()
            })
        );
        assert_eq!(rows[5].primary_action_label(), Some("Retry"));
        assert_eq!(
            rows[6].primary_action,
            Some(IcedQueueAction::Retry {
                job_id: "canceled".into()
            })
        );
        assert_eq!(rows[7].primary_action, None);
        assert_eq!(rows[7].primary_action_label(), None);
    }

    #[test]
    fn iced_shell_replaces_snapshot_when_queue_action_finishes() {
        let mut shell = IcedDesktopShell::new(snapshot(vec![job("old", JobState::Paused)], None));

        let _ = shell.update(IcedDesktopMessage::QueueActionFinished(Ok(snapshot(
            vec![job("resumed", JobState::Queued)],
            None,
        ))));

        let rows = shell.queue_rows();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].filename, "resumed.bin");
        assert_eq!(rows[0].status_label, "Queued");
        assert_eq!(shell.queue_action_error_text(), None);
    }

    #[test]
    fn iced_shell_stores_queue_action_errors_without_replacing_snapshot() {
        let mut shell = IcedDesktopShell::new(snapshot(vec![job("kept", JobState::Paused)], None));

        let _ = shell.update(IcedDesktopMessage::QueueActionFinished(Err(
            "Job not found.".into(),
        )));

        let rows = shell.queue_rows();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].filename, "kept.bin");
        assert_eq!(shell.queue_action_error_text(), Some("Job not found."));
    }

    fn snapshot(
        jobs: Vec<DownloadJob>,
        startup_recovery: Option<StartupRecoverySummary>,
    ) -> DesktopSnapshot {
        DesktopSnapshot {
            connection_state: ConnectionState::Connected,
            jobs,
            settings: Settings::default(),
            startup_recovery,
        }
    }

    fn job(id: &str, state: JobState) -> DownloadJob {
        DownloadJob {
            id: id.into(),
            url: format!("https://example.test/{id}.bin"),
            filename: format!("{id}.bin"),
            source: None,
            transfer_kind: Default::default(),
            integrity_check: None,
            torrent: None,
            state,
            removal_state: None,
            error: None,
            failure_category: None,
            created_at: 0,
            progress: 0.0,
            total_bytes: 0,
            downloaded_bytes: 0,
            speed: 0,
            eta: 0,
            active_segments: None,
            planned_segments: None,
            resume_support: Default::default(),
            retry_attempts: 0,
            auto_restart_attempts: 0,
            resolved_from_url: None,
            hoster_preflight: None,
            target_path: format!("C:\\Downloads\\{id}.bin"),
            temp_path: format!("C:\\Downloads\\{id}.bin.part"),
            artifact_exists: None,
            bulk_archive: None,
        }
    }
}
