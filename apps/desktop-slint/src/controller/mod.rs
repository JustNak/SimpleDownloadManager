use simple_download_manager_desktop_core::storage::{
    ConnectionState, DesktopSnapshot, DownloadJob, JobState,
};

#[derive(Debug, Clone, PartialEq)]
pub struct JobRow {
    pub id: String,
    pub filename: String,
    pub state: String,
    pub progress: f64,
    pub bytes_text: String,
}

pub fn job_row_from_job(job: &DownloadJob) -> JobRow {
    JobRow {
        id: job.id.clone(),
        filename: job.filename.clone(),
        state: job_state_label(job.state).into(),
        progress: job.progress,
        bytes_text: bytes_text(job.downloaded_bytes, job.total_bytes),
    }
}

pub fn status_text_from_snapshot(snapshot: &DesktopSnapshot) -> String {
    let connection = match snapshot.connection_state {
        ConnectionState::Checking => "Checking browser handoff",
        ConnectionState::Connected => "Connected to browser handoff",
        ConnectionState::HostMissing => "Browser host is missing",
        ConnectionState::AppMissing => "Desktop app is missing",
        ConnectionState::AppUnreachable => "Desktop app is unreachable",
        ConnectionState::Error => "Browser handoff error",
    };

    let count = snapshot.jobs.len();
    let suffix = if count == 1 { "download" } else { "downloads" };
    format!("{connection} | {count} {suffix}")
}

pub fn rows_from_snapshot(snapshot: &DesktopSnapshot) -> Vec<JobRow> {
    snapshot.jobs.iter().map(job_row_from_job).collect()
}

pub fn slint_rows_from_snapshot(snapshot: &DesktopSnapshot) -> Vec<crate::JobRow> {
    snapshot
        .jobs
        .iter()
        .map(slint_job_row_from_job)
        .collect()
}

pub fn slint_job_row_from_job(job: &DownloadJob) -> crate::JobRow {
    let row = job_row_from_job(job);
    crate::JobRow {
        id: row.id.into(),
        filename: row.filename.into(),
        state: row.state.into(),
        progress: row.progress as f32,
        bytes_text: row.bytes_text.into(),
    }
}

fn job_state_label(state: JobState) -> &'static str {
    match state {
        JobState::Queued => "Queued",
        JobState::Starting => "Starting",
        JobState::Downloading => "Downloading",
        JobState::Seeding => "Seeding",
        JobState::Paused => "Paused",
        JobState::Completed => "Completed",
        JobState::Failed => "Failed",
        JobState::Canceled => "Canceled",
    }
}

fn bytes_text(downloaded_bytes: u64, total_bytes: u64) -> String {
    if total_bytes == 0 {
        return format!("{} / Unknown", format_bytes(downloaded_bytes));
    }

    format!(
        "{} / {}",
        format_bytes(downloaded_bytes),
        format_bytes(total_bytes)
    )
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let value = bytes as f64;
    if value >= GIB {
        return format!("{:.1} GiB", value / GIB);
    }
    if value >= MIB {
        return format!("{:.1} MiB", value / MIB);
    }
    if value >= KIB {
        return format!("{:.1} KiB", value / KIB);
    }

    format!("{bytes} B")
}
