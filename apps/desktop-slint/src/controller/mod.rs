use simple_download_manager_desktop_core::contracts::ProgressBatchContext;
use simple_download_manager_desktop_core::storage::{
    ConnectionState, DesktopSnapshot, DownloadJob, DownloadPrompt, JobState,
};

#[derive(Debug, Clone, PartialEq)]
pub struct JobRow {
    pub id: String,
    pub filename: String,
    pub state: String,
    pub progress: f64,
    pub bytes_text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PromptWindowDetails {
    pub id: String,
    pub title: String,
    pub filename: String,
    pub url: String,
    pub destination: String,
    pub size_text: String,
    pub duplicate_text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProgressWindowDetails {
    pub id: String,
    pub title: String,
    pub filename: String,
    pub state: String,
    pub bytes_text: String,
    pub progress: f64,
    pub error_text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatchProgressDetails {
    pub batch_id: String,
    pub title: String,
    pub summary: String,
    pub bytes_text: String,
    pub progress: f64,
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
    snapshot.jobs.iter().map(slint_job_row_from_job).collect()
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

pub fn prompt_details_from_prompt(prompt: &DownloadPrompt) -> PromptWindowDetails {
    let duplicate_text = prompt
        .duplicate_reason
        .as_ref()
        .map(|reason| {
            let duplicate_name = prompt
                .duplicate_filename
                .as_deref()
                .unwrap_or(prompt.filename.as_str());
            format!("Duplicate {reason}: {duplicate_name}")
        })
        .unwrap_or_default();
    let title = if prompt.duplicate_job.is_some()
        || prompt.duplicate_path.is_some()
        || !duplicate_text.is_empty()
    {
        "Duplicate download detected"
    } else {
        "New download detected"
    };

    PromptWindowDetails {
        id: prompt.id.clone(),
        title: title.into(),
        filename: prompt.filename.clone(),
        url: prompt.url.clone(),
        destination: prompt.target_path.clone(),
        size_text: prompt
            .total_bytes
            .map(format_bytes)
            .unwrap_or_else(|| "Unknown".into()),
        duplicate_text,
    }
}

pub fn slint_prompt_details_from_prompt(prompt: &DownloadPrompt) -> crate::PromptDetails {
    slint_prompt_details_from_details(prompt_details_from_prompt(prompt))
}

pub fn waiting_prompt_details() -> crate::PromptDetails {
    slint_prompt_details_from_details(PromptWindowDetails {
        id: String::new(),
        title: "Download prompt".into(),
        filename: "Waiting for a download...".into(),
        url: String::new(),
        destination: String::new(),
        size_text: "Unknown".into(),
        duplicate_text: String::new(),
    })
}

pub fn progress_details_from_job(job: &DownloadJob, title: &str) -> ProgressWindowDetails {
    ProgressWindowDetails {
        id: job.id.clone(),
        title: title.into(),
        filename: job.filename.clone(),
        state: job_state_label(job.state).into(),
        bytes_text: bytes_text(job.downloaded_bytes, job.total_bytes),
        progress: job.progress,
        error_text: job.error.clone().unwrap_or_default(),
    }
}

pub fn slint_progress_details_from_job(job: &DownloadJob, title: &str) -> crate::ProgressDetails {
    slint_progress_details_from_details(progress_details_from_job(job, title))
}

pub fn empty_progress_details(id: &str, title: &str) -> crate::ProgressDetails {
    slint_progress_details_from_details(ProgressWindowDetails {
        id: id.into(),
        title: title.into(),
        filename: id.into(),
        state: "Unknown".into(),
        bytes_text: "0 B / Unknown".into(),
        progress: 0.0,
        error_text: String::new(),
    })
}

pub fn batch_details_from_context(
    context: &ProgressBatchContext,
    snapshot: &DesktopSnapshot,
) -> BatchProgressDetails {
    let jobs: Vec<&DownloadJob> = context
        .job_ids
        .iter()
        .filter_map(|id| snapshot.jobs.iter().find(|job| job.id == *id))
        .collect();
    let completed = jobs
        .iter()
        .filter(|job| job.state == JobState::Completed)
        .count();
    let total = context.job_ids.len();
    let downloaded_bytes: u64 = jobs.iter().map(|job| job.downloaded_bytes).sum();
    let total_bytes: u64 = jobs.iter().map(|job| job.total_bytes).sum();
    let progress = if total_bytes > 0 {
        (downloaded_bytes as f64 / total_bytes as f64) * 100.0
    } else if total > 0 {
        jobs.iter().map(|job| job.progress).sum::<f64>() / total as f64
    } else {
        0.0
    };

    BatchProgressDetails {
        batch_id: context.batch_id.clone(),
        title: context.title.clone(),
        summary: format!("{completed} of {total} completed"),
        bytes_text: bytes_text(downloaded_bytes, total_bytes),
        progress,
    }
}

pub fn slint_batch_details_from_context(
    context: &ProgressBatchContext,
    snapshot: &DesktopSnapshot,
) -> crate::BatchDetails {
    slint_batch_details_from_details(batch_details_from_context(context, snapshot))
}

pub fn empty_batch_details(batch_id: &str) -> crate::BatchDetails {
    slint_batch_details_from_details(BatchProgressDetails {
        batch_id: batch_id.into(),
        title: "Batch progress".into(),
        summary: "Waiting for batch details".into(),
        bytes_text: "0 B / Unknown".into(),
        progress: 0.0,
    })
}

fn slint_prompt_details_from_details(details: PromptWindowDetails) -> crate::PromptDetails {
    crate::PromptDetails {
        id: details.id.into(),
        title: details.title.into(),
        filename: details.filename.into(),
        url: details.url.into(),
        destination: details.destination.into(),
        size_text: details.size_text.into(),
        duplicate_text: details.duplicate_text.into(),
    }
}

fn slint_progress_details_from_details(details: ProgressWindowDetails) -> crate::ProgressDetails {
    crate::ProgressDetails {
        id: details.id.into(),
        title: details.title.into(),
        filename: details.filename.into(),
        state: details.state.into(),
        bytes_text: details.bytes_text.into(),
        progress: details.progress as f32,
        error_text: details.error_text.into(),
    }
}

fn slint_batch_details_from_details(details: BatchProgressDetails) -> crate::BatchDetails {
    crate::BatchDetails {
        batch_id: details.batch_id.into(),
        title: details.title.into(),
        summary: details.summary.into(),
        bytes_text: details.bytes_text.into(),
        progress: details.progress as f32,
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
