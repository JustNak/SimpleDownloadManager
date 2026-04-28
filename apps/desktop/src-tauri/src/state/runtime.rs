use super::*;

impl RuntimeState {
    pub(super) fn push_diagnostic_event(
        &mut self,
        level: DiagnosticLevel,
        category: String,
        message: String,
        job_id: Option<String>,
    ) {
        self.diagnostic_events.push(DiagnosticEvent {
            timestamp: current_unix_timestamp_millis(),
            level,
            category,
            message,
            job_id,
        });

        trim_diagnostic_events(&mut self.diagnostic_events);
    }

    pub(super) fn snapshot(&self) -> DesktopSnapshot {
        DesktopSnapshot {
            connection_state: self.connection_state,
            jobs: self
                .jobs
                .iter()
                .cloned()
                .map(add_artifact_existence)
                .collect(),
            settings: self.settings.clone(),
        }
    }

    pub(super) fn persisted(&self) -> PersistedState {
        PersistedState {
            jobs: self
                .jobs
                .iter()
                .cloned()
                .map(clear_transient_job_state)
                .collect(),
            settings: self.settings.clone(),
            main_window: self.main_window.clone(),
            diagnostic_events: self.diagnostic_events.clone(),
        }
    }

    pub(super) fn pause_all_jobs(&mut self) {
        for job in &mut self.jobs {
            if matches!(
                job.state,
                JobState::Queued | JobState::Starting | JobState::Downloading | JobState::Seeding
            ) {
                job.state = JobState::Paused;
                job.speed = 0;
                job.eta = 0;
            }
        }
    }

    pub(super) fn resume_all_jobs(&mut self) {
        for job in &mut self.jobs {
            if matches!(
                job.state,
                JobState::Paused | JobState::Failed | JobState::Canceled
            ) {
                job.state = JobState::Queued;
                job.error = None;
                job.failure_category = None;
                job.retry_attempts = 0;
                job.speed = 0;
                job.eta = 0;
            }
        }
    }

    pub(super) fn duplicate_enqueue_result(&self, url: &str) -> Option<EnqueueResult> {
        let existing_index = self.jobs.iter().position(|job| job.url == url)?;
        let existing_job = &self.jobs[existing_index];

        Some(EnqueueResult {
            snapshot: self.snapshot(),
            job_id: existing_job.id.clone(),
            filename: existing_job.filename.clone(),
            status: EnqueueStatus::DuplicateExistingJob,
        })
    }

    pub(super) fn queue_summary(&self) -> QueueSummary {
        QueueSummary {
            total: self.jobs.len(),
            active: self
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
                .count(),
            attention: self
                .jobs
                .iter()
                .filter(|job| job_needs_attention(job))
                .count(),
            queued: self
                .jobs
                .iter()
                .filter(|job| job.state == JobState::Queued)
                .count(),
            downloading: self
                .jobs
                .iter()
                .filter(|job| {
                    matches!(
                        job.state,
                        JobState::Starting | JobState::Downloading | JobState::Seeding
                    )
                })
                .count(),
            completed: self
                .jobs
                .iter()
                .filter(|job| matches!(job.state, JobState::Completed | JobState::Canceled))
                .count(),
            failed: self
                .jobs
                .iter()
                .filter(|job| job.state == JobState::Failed)
                .count(),
        }
    }
}

pub(super) fn job_needs_attention(job: &DownloadJob) -> bool {
    if job.state == JobState::Failed || job.failure_category.is_some() {
        return true;
    }

    let is_unfinished = !matches!(job.state, JobState::Completed | JobState::Canceled);
    let has_partial_progress = job.downloaded_bytes > 0 || job.progress > 0.0;
    is_unfinished && has_partial_progress && job.resume_support == ResumeSupport::Unsupported
}

pub(super) fn normalize_job(mut job: DownloadJob, settings: &Settings) -> DownloadJob {
    if let Some(check) = &mut job.integrity_check {
        check.expected = check.expected.trim().to_ascii_lowercase();
        check.actual = check
            .actual
            .as_ref()
            .map(|value| value.trim().to_ascii_lowercase());
    }

    if job.filename.trim().is_empty() {
        job.filename = derive_filename(&job.url);
    }

    if job.target_path.trim().is_empty() {
        let target_path = PathBuf::from(&settings.download_directory).join(&job.filename);
        job.target_path = target_path.display().to_string();
    }

    if job.temp_path.trim().is_empty() {
        job.temp_path = format!("{}.part", job.target_path);
    }

    if matches!(
        job.state,
        JobState::Starting | JobState::Downloading | JobState::Seeding
    ) {
        job.state = JobState::Queued;
        job.speed = 0;
        job.eta = 0;
    }

    if job.created_at == 0 {
        job.created_at = current_unix_timestamp_millis();
    }

    job.artifact_exists = None;

    job
}

pub(super) fn add_artifact_existence(mut job: DownloadJob) -> DownloadJob {
    job.artifact_exists = if job.state == JobState::Completed {
        Some(Path::new(&job.target_path).exists())
    } else {
        None
    };
    job
}

pub(super) fn clear_transient_job_state(mut job: DownloadJob) -> DownloadJob {
    job.artifact_exists = None;
    job
}

pub(super) fn current_unix_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

pub(super) fn next_job_number(jobs: &[DownloadJob]) -> u64 {
    jobs.iter()
        .filter_map(|job| job.id.strip_prefix("job_"))
        .filter_map(|value| value.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1
}
