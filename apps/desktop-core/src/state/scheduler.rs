use super::*;

impl SharedState {
    pub async fn claim_schedulable_jobs(
        &self,
    ) -> Result<(DesktopSnapshot, Vec<DownloadTask>), String> {
        let auth_by_job = self.handoff_auth.read().await.clone();
        let (snapshot, persisted, tasks) = {
            let mut state = self.inner.write().await;
            let active_download_workers = state
                .active_workers
                .iter()
                .filter(|id| {
                    state
                        .jobs
                        .iter()
                        .find(|job| &job.id == *id)
                        .map(|job| job.state != JobState::Seeding)
                        .unwrap_or(false)
                })
                .count() as u32;
            let available_slots = state
                .settings
                .max_concurrent_downloads
                .max(1)
                .saturating_sub(active_download_workers) as usize;

            if available_slots == 0 {
                return Ok((state.snapshot(), Vec::new()));
            }

            let mut scheduled_ids = Vec::new();
            for job in &state.jobs {
                if scheduled_ids.len() >= available_slots {
                    break;
                }

                if job.state == JobState::Queued && !state.active_workers.contains(&job.id) {
                    scheduled_ids.push(job.id.clone());
                }
            }

            let mut tasks = Vec::new();
            for scheduled_id in scheduled_ids {
                if let Some(job) = state.jobs.iter_mut().find(|job| job.id == scheduled_id) {
                    job.state = JobState::Starting;
                    job.speed = 0;
                    job.eta = 0;
                    job.error = None;
                    let task = DownloadTask {
                        id: job.id.clone(),
                        url: job.url.clone(),
                        filename: job.filename.clone(),
                        transfer_kind: job.transfer_kind,
                        torrent: job.torrent.clone(),
                        handoff_auth: auth_by_job.get(&job.id).cloned(),
                        target_path: PathBuf::from(&job.target_path),
                        temp_path: PathBuf::from(&job.temp_path),
                    };
                    let task_id = task.id.clone();
                    let _ = job;
                    state.active_workers.insert(task_id);
                    state.push_diagnostic_event(
                        DiagnosticLevel::Info,
                        "download".into(),
                        format!("Starting {}", task.id),
                        Some(task.id.clone()),
                    );
                    tasks.push(task);
                }
            }

            (state.snapshot(), state.persisted(), tasks)
        };

        if !tasks.is_empty() {
            persist_state(&self.storage_path, &persisted)?;
        }

        Ok((snapshot, tasks))
    }

    pub async fn clear_handoff_auth(&self, id: &str) {
        self.handoff_auth.write().await.remove(id);
    }

    #[cfg(test)]
    pub(super) async fn has_handoff_auth(&self, id: &str) -> bool {
        self.handoff_auth.read().await.contains_key(id)
    }

    pub async fn worker_control(&self, id: &str) -> WorkerControl {
        let state = self.inner.read().await;
        let Some(job) = state.jobs.iter().find(|job| job.id == id) else {
            return WorkerControl::Missing;
        };

        match job.state {
            JobState::Paused => WorkerControl::Paused,
            JobState::Canceled => WorkerControl::Canceled,
            JobState::Completed | JobState::Failed => WorkerControl::Missing,
            _ => WorkerControl::Continue,
        }
    }
}
