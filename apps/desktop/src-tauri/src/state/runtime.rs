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
            jobs: self.jobs.clone(),
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
        self.bulk_hoster_worker_health.clear();
        self.bulk_hoster_fairness.clear();
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
                job.auto_restart_attempts = 0;
                job.speed = 0;
                job.eta = 0;
            }
        }
        self.bulk_hoster_worker_health.clear();
        self.bulk_hoster_fairness.clear();
    }

    #[cfg(test)]
    pub(super) fn duplicate_enqueue_result(&self, url: &str) -> Option<EnqueueResult> {
        let existing_index = self.jobs.iter().position(|job| job.url == url)?;
        let existing_job = &self.jobs[existing_index];

        Some(self.duplicate_enqueue_result_for_job(existing_job))
    }

    pub(super) fn duplicate_enqueue_result_for_job(
        &self,
        existing_job: &DownloadJob,
    ) -> EnqueueResult {
        EnqueueResult {
            snapshot: self.snapshot(),
            job_id: existing_job.id.clone(),
            filename: existing_job.filename.clone(),
            status: EnqueueStatus::DuplicateExistingJob,
        }
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

    pub(super) fn torrent_diagnostics_snapshot(&self) -> Vec<TorrentJobDiagnostics> {
        self.jobs
            .iter()
            .filter_map(|job| {
                let torrent = job.torrent.as_ref()?;
                let diagnostics = torrent.diagnostics.clone()?;
                Some(TorrentJobDiagnostics {
                    job_id: job.id.clone(),
                    filename: job.filename.clone(),
                    info_hash: torrent.info_hash.clone(),
                    diagnostics,
                })
            })
            .collect()
    }

    pub(super) fn rebuild_job_indexes(&mut self) {
        self.job_indexes = job_indexes_for(&self.jobs);
    }

    pub(super) fn job_index(&self, id: &str) -> Option<usize> {
        self.job_indexes
            .get(id)
            .copied()
            .filter(|index| self.jobs.get(*index).is_some_and(|job| job.id == id))
    }

    pub(super) fn job(&self, id: &str) -> Option<&DownloadJob> {
        self.job_index(id).and_then(|index| self.jobs.get(index))
    }

    pub(super) fn job_mut(&mut self, id: &str) -> Option<&mut DownloadJob> {
        let index = self.job_index(id)?;
        self.jobs.get_mut(index)
    }

    pub(super) fn push_job(&mut self, job: DownloadJob) {
        self.job_indexes.insert(job.id.clone(), self.jobs.len());
        self.jobs.push(job);
    }

    pub(super) fn remove_job_at_index(&mut self, index: usize) -> DownloadJob {
        let job = self.jobs.remove(index);
        self.rebuild_job_indexes();
        job
    }

    pub(super) fn retain_jobs(&mut self, mut keep: impl FnMut(&DownloadJob) -> bool) {
        self.jobs.retain(|job| keep(job));
        self.rebuild_job_indexes();
    }

    pub(super) fn should_persist_progress_at(&mut self, now: Instant) -> bool {
        if self.last_progress_persist_at.is_some_and(|last| {
            now.saturating_duration_since(last) < PROGRESS_PERSIST_COALESCE_WINDOW
        }) {
            return false;
        }

        self.last_progress_persist_at = Some(now);
        true
    }

    pub(super) fn remove_active_worker(&mut self, id: &str) {
        self.active_workers.remove(id);
        self.clear_bulk_hoster_worker_health(id);
    }

    pub(super) fn clear_bulk_hoster_worker_health(&mut self, id: &str) {
        self.bulk_hoster_worker_health.remove(id);
        if self.bulk_hoster_worker_health.is_empty() {
            self.bulk_hoster_fairness.clear();
        }
    }

    pub(super) fn update_bulk_hoster_worker_health(
        &mut self,
        id: &str,
        downloaded_bytes: u64,
        speed: u64,
        now: Instant,
    ) {
        if let Some(health) = self.bulk_hoster_worker_health.get_mut(id) {
            health.update(downloaded_bytes, speed, now);
        }
    }

    pub(super) fn mark_bulk_hoster_worker_resolving(&mut self, id: &str, now: Instant) {
        if let Some(health) = self.bulk_hoster_worker_health.get_mut(id) {
            health.mark_resolving(now);
        }
    }

    pub(super) fn mark_bulk_hoster_worker_transferring(
        &mut self,
        id: &str,
        downloaded_bytes: u64,
        now: Instant,
    ) {
        if let Some(health) = self.bulk_hoster_worker_health.get_mut(id) {
            if health.phase != BulkHosterWorkerPhase::Transferring {
                health.mark_transferring(downloaded_bytes, now);
            }
        }
    }
}

pub(super) fn job_indexes_for(jobs: &[DownloadJob]) -> HashMap<String, usize> {
    jobs.iter()
        .enumerate()
        .map(|(index, job)| (job.id.clone(), index))
        .collect()
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

    job.resolved_from_url = job.resolved_from_url.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    });

    if matches!(
        job.state,
        JobState::Starting | JobState::Downloading | JobState::Seeding
    ) {
        job.state = JobState::Queued;
        job.speed = 0;
        job.eta = 0;
    }

    mark_stale_bulk_archive_finalization_failed(&mut job);

    if job.created_at == 0 {
        job.created_at = current_unix_timestamp_millis();
    }

    job.artifact_exists = None;

    job
}

fn mark_stale_bulk_archive_finalization_failed(job: &mut DownloadJob) {
    let Some(archive) = &mut job.bulk_archive else {
        return;
    };
    if !archive.archive_status.is_finalizing() {
        return;
    }

    archive.archive_status = BulkArchiveStatus::Failed;
    archive.error = Some(
        "Bulk archive finalization was interrupted by app shutdown. Use Fix archive to finish it."
            .into(),
    );
}

pub(super) fn clear_transient_job_state(mut job: DownloadJob) -> DownloadJob {
    job.artifact_exists = None;
    if let Some(torrent) = &mut job.torrent {
        torrent.diagnostics = None;
    }
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
