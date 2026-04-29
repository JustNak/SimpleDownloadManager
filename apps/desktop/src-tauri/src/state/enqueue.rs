use super::*;

#[derive(Debug, Clone)]
pub(super) struct DuplicateDownloadMatch {
    pub job: Option<DownloadJob>,
    pub path: Option<String>,
    pub filename: Option<String>,
    pub reason: &'static str,
}

impl SharedState {
    pub async fn enqueue_download(
        &self,
        url: String,
        source: Option<DownloadSource>,
    ) -> Result<EnqueueResult, BackendError> {
        self.enqueue_download_with_options(
            url,
            EnqueueOptions {
                source,
                ..Default::default()
            },
        )
        .await
    }

    pub async fn enqueue_downloads(
        &self,
        urls: Vec<String>,
        source: Option<DownloadSource>,
        bulk_archive_name: Option<String>,
    ) -> Result<Vec<EnqueueResult>, BackendError> {
        if urls.is_empty() {
            return Err(BackendError {
                code: "INVALID_URL",
                message: "Add at least one download URL.".into(),
            });
        }

        let normalized_urls = urls
            .iter()
            .map(|url| normalize_download_url(url))
            .collect::<Result<Vec<_>, _>>()?;
        let bulk_archive = bulk_archive_name
            .filter(|_| normalized_urls.len() > 1)
            .map(|name| BulkArchiveInfo {
                id: format!(
                    "bulk_{}_{}",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|duration| duration.as_millis())
                        .unwrap_or_default(),
                    normalized_urls.len()
                ),
                name: normalize_archive_filename(&name),
                archive_status: BulkArchiveStatus::Pending,
                output_path: None,
                error: None,
            });

        let mut results = Vec::with_capacity(normalized_urls.len());
        for url in normalized_urls {
            results.push(
                self.enqueue_download_with_options(
                    url,
                    EnqueueOptions {
                        source: source.clone(),
                        transfer_kind: Some(TransferKind::Http),
                        bulk_archive: bulk_archive.clone(),
                        ..Default::default()
                    },
                )
                .await?,
            );
        }

        Ok(results)
    }

    pub async fn enqueue_download_with_options(
        &self,
        url: String,
        options: EnqueueOptions,
    ) -> Result<EnqueueResult, BackendError> {
        let handoff_auth = options.handoff_auth.clone();
        if let Some(auth) = handoff_auth.as_ref() {
            if options
                .source
                .as_ref()
                .map(|source| source.entry_point.as_str())
                != Some("browser_download")
            {
                return Err(BackendError {
                    code: "INVALID_PAYLOAD",
                    message: "Authenticated handoff is only supported for browser downloads."
                        .into(),
                });
            }
            self.validate_handoff_auth_for_url(&url, auth).await?;
        }

        let (result, persisted) = {
            let mut state = self.inner.write().await;
            let result = state.enqueue_download_in_memory(&url, options)?;
            let persisted = state.persisted();
            (result, persisted)
        };

        if result.status == EnqueueStatus::Queued {
            persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
            if let Some(auth) = handoff_auth {
                self.handoff_auth
                    .write()
                    .await
                    .insert(result.job_id.clone(), auth);
            }
        }

        Ok(result)
    }

    pub(super) async fn validate_handoff_auth_for_url(
        &self,
        _url: &str,
        auth: &HandoffAuth,
    ) -> Result<(), BackendError> {
        validate_handoff_auth_headers(auth)?;
        let settings = self.extension_integration_settings().await;
        if !settings.authenticated_handoff_enabled {
            return Err(BackendError {
                code: "PERMISSION_DENIED",
                message: "Protected Downloads is disabled.".into(),
            });
        }

        Ok(())
    }

    pub async fn prepare_download_prompt(
        &self,
        id: impl Into<String>,
        url: &str,
        source: Option<DownloadSource>,
        filename_hint: Option<String>,
        total_bytes: Option<u64>,
    ) -> Result<DownloadPrompt, BackendError> {
        let state = self.inner.read().await;
        state.prepare_download_prompt(id, url, source, filename_hint, total_bytes)
    }
}

impl RuntimeState {
    pub(super) fn enqueue_download_in_memory(
        &mut self,
        url: &str,
        mut options: EnqueueOptions,
    ) -> Result<EnqueueResult, BackendError> {
        let explicit_transfer_kind = options.transfer_kind;
        let url = normalize_download_input(url, explicit_transfer_kind)?;
        options.expected_sha256 = normalize_expected_sha256(options.expected_sha256)?;
        let inferred_transfer_kind = transfer_kind_for_url(&url);
        let transfer_kind = explicit_transfer_kind.unwrap_or(inferred_transfer_kind);
        if transfer_kind != inferred_transfer_kind {
            return Err(BackendError {
                code: "INVALID_TRANSFER_KIND",
                message:
                    "Torrent transfers require a magnet link, HTTP(S) .torrent URL, or local .torrent file."
                        .into(),
            });
        }

        if transfer_kind == TransferKind::Torrent && !self.settings.torrent.enabled {
            return Err(BackendError {
                code: "TORRENT_DISABLED",
                message: "Torrent downloads are disabled in settings.".into(),
            });
        }

        if transfer_kind == TransferKind::Torrent && options.expected_sha256.is_some() {
            return Err(BackendError {
                code: "INVALID_CHECKSUM",
                message: "SHA-256 checks are only supported for HTTP downloads.".into(),
            });
        }

        let directory = options
            .directory_override
            .as_deref()
            .unwrap_or(&self.settings.download_directory)
            .trim();
        if directory.is_empty() {
            return Err(BackendError {
                code: "DESTINATION_NOT_CONFIGURED",
                message: "Configure a download directory before adding downloads.".into(),
            });
        }

        let download_dir = PathBuf::from(directory);
        std::fs::create_dir_all(&download_dir).map_err(|error| BackendError {
            code: "DESTINATION_INVALID",
            message: format!("Could not create the download directory: {error}"),
        })?;

        let filename = if transfer_kind == TransferKind::Torrent {
            torrent_filename_from_url(&url, options.filename_hint.as_deref())
        } else {
            filename_from_hint(options.filename_hint.as_deref(), &url)
        };
        let target_dir = prepare_category_download_directory(&download_dir, &filename)?;
        verify_download_directory_writable(&target_dir)?;
        let (base_target_path, base_temp_path) = candidate_target_paths(&target_dir, &filename);
        let duplicate_match =
            self.duplicate_download_match(&url, &base_target_path, &base_temp_path);

        if options.duplicate_policy == DuplicatePolicy::ReturnExisting {
            if let Some(job) = duplicate_match
                .as_ref()
                .and_then(|duplicate| duplicate.job.as_ref())
            {
                return Ok(self.duplicate_enqueue_result_for_job(job));
            }
        }

        let duplicate_replacement_index =
            if options.duplicate_policy == DuplicatePolicy::ReplaceExisting {
                let index = duplicate_match
                    .as_ref()
                    .and_then(|duplicate| duplicate.job.as_ref())
                    .and_then(|duplicate_job| {
                        self.jobs.iter().position(|job| job.id == duplicate_job.id)
                    });
                if let Some(index) = index {
                    let job = &self.jobs[index];
                    if job_blocks_removal(job, self.active_workers.contains(&job.id)) {
                        return Err(BackendError {
                            code: "DUPLICATE_ACTIVE",
                            message: "Pause or cancel the existing duplicate before replacing it."
                                .into(),
                        });
                    }
                }
                index
            } else {
                None
            };
        let replaced_duplicate = duplicate_replacement_index.map(|index| {
            let job = self.jobs.remove(index);
            self.active_workers.remove(&job.id);
            self.external_reseed_jobs.remove(&job.id);
            if job.state != JobState::Completed {
                let _ = remove_path_if_exists(Path::new(&job.temp_path));
            }
            (job.id, job.filename)
        });
        let job_id = format!("job_{}", self.next_job_number);
        self.next_job_number += 1;
        let (target_path, temp_path) = if transfer_kind == TransferKind::Torrent {
            (
                unique_target_path(&target_dir, &filename, &self.jobs),
                torrent_state_path_for_job(&download_dir, &job_id),
            )
        } else if options.duplicate_policy == DuplicatePolicy::ReplaceExisting
            && duplicate_match.is_some()
        {
            prepare_overwrite_target(&base_target_path, &base_temp_path)?;
            (base_target_path, base_temp_path)
        } else {
            allocate_target_paths(&target_dir, &filename, &self.jobs)
        };
        let integrity_check = options.expected_sha256.map(|expected| IntegrityCheck {
            algorithm: IntegrityAlgorithm::Sha256,
            expected,
            actual: None,
            status: IntegrityStatus::Pending,
        });

        self.jobs.push(DownloadJob {
            id: job_id.clone(),
            url: url.clone(),
            filename: filename.clone(),
            source: options.source,
            transfer_kind,
            integrity_check,
            torrent: (transfer_kind == TransferKind::Torrent).then(TorrentInfo::default),
            state: JobState::Queued,
            created_at: current_unix_timestamp_millis(),
            progress: 0.0,
            total_bytes: 0,
            downloaded_bytes: 0,
            speed: 0,
            eta: 0,
            error: None,
            failure_category: None,
            resume_support: ResumeSupport::Unknown,
            retry_attempts: 0,
            target_path: target_path.display().to_string(),
            temp_path: temp_path.display().to_string(),
            artifact_exists: None,
            bulk_archive: options.bulk_archive,
        });
        self.push_diagnostic_event(
            DiagnosticLevel::Info,
            "download".into(),
            format!("Queued {filename}"),
            Some(job_id.clone()),
        );
        if let Some((replaced_id, replaced_filename)) = replaced_duplicate {
            self.push_diagnostic_event(
                DiagnosticLevel::Info,
                "download".into(),
                format!("Replaced duplicate {replaced_filename} ({replaced_id}) with {filename}"),
                Some(job_id.clone()),
            );
        }

        Ok(EnqueueResult {
            snapshot: self.snapshot(),
            job_id,
            filename,
            status: EnqueueStatus::Queued,
        })
    }

    pub(super) fn prepare_download_prompt(
        &self,
        id: impl Into<String>,
        url: &str,
        source: Option<DownloadSource>,
        filename_hint: Option<String>,
        total_bytes: Option<u64>,
    ) -> Result<DownloadPrompt, BackendError> {
        let url = normalize_download_url(url)?;
        let transfer_kind = transfer_kind_for_url(&url);
        let filename = if transfer_kind == TransferKind::Torrent {
            torrent_filename_from_url(&url, filename_hint.as_deref())
        } else {
            filename_from_hint(filename_hint.as_deref(), &url)
        };
        let default_directory = self.settings.download_directory.clone();
        let target_path = if default_directory.trim().is_empty() {
            String::new()
        } else {
            let category_dir =
                category_download_directory(Path::new(&default_directory), &filename);
            let (base_target_path, base_temp_path) =
                candidate_target_paths(&category_dir, &filename);
            let duplicate_match =
                self.duplicate_download_match(&url, &base_target_path, &base_temp_path);
            let (target_path, _) = if duplicate_match.is_some() {
                (base_target_path, base_temp_path)
            } else {
                allocate_target_paths(&category_dir, &filename, &self.jobs)
            };
            target_path.display().to_string()
        };
        let duplicate_match = if default_directory.trim().is_empty() {
            self.duplicate_download_match(&url, Path::new(""), Path::new(""))
        } else {
            let category_dir =
                category_download_directory(Path::new(&default_directory), &filename);
            let (base_target_path, base_temp_path) =
                candidate_target_paths(&category_dir, &filename);
            self.duplicate_download_match(&url, &base_target_path, &base_temp_path)
        };

        Ok(DownloadPrompt {
            id: id.into(),
            url,
            filename,
            source,
            total_bytes: total_bytes.filter(|bytes| *bytes > 0),
            default_directory,
            target_path,
            duplicate_job: duplicate_match
                .as_ref()
                .and_then(|duplicate| duplicate.job.clone()),
            duplicate_path: duplicate_match
                .as_ref()
                .and_then(|duplicate| duplicate.path.clone()),
            duplicate_filename: duplicate_match
                .as_ref()
                .and_then(|duplicate| duplicate.filename.clone()),
            duplicate_reason: duplicate_match
                .as_ref()
                .map(|duplicate| duplicate.reason.to_string()),
        })
    }

    pub(super) fn duplicate_download_match(
        &self,
        url: &str,
        target_path: &Path,
        temp_path: &Path,
    ) -> Option<DuplicateDownloadMatch> {
        if let Some(job) = self.jobs.iter().find(|job| job.url == url) {
            return Some(DuplicateDownloadMatch {
                job: Some(job.clone()),
                path: non_empty_string(job.target_path.clone()),
                filename: Some(job.filename.clone()),
                reason: "url",
            });
        }

        if let Some(job) = self.jobs.iter().find(|job| {
            path_matches(&job.target_path, target_path) || path_matches(&job.temp_path, temp_path)
        }) {
            return Some(DuplicateDownloadMatch {
                job: Some(job.clone()),
                path: non_empty_string(job.target_path.clone()),
                filename: Some(job.filename.clone()),
                reason: "path",
            });
        }

        if !target_path.as_os_str().is_empty() && target_path.exists() {
            return Some(DuplicateDownloadMatch {
                job: None,
                path: Some(target_path.display().to_string()),
                filename: target_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(str::to_string),
                reason: "file",
            });
        }

        if !temp_path.as_os_str().is_empty() && temp_path.exists() {
            return Some(DuplicateDownloadMatch {
                job: None,
                path: Some(temp_path.display().to_string()),
                filename: temp_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(str::to_string),
                reason: "partial_file",
            });
        }

        None
    }
}

fn path_matches(existing_path: &str, candidate: &Path) -> bool {
    if existing_path.trim().is_empty() || candidate.as_os_str().is_empty() {
        return false;
    }

    path_key(existing_path) == path_key(&candidate.display().to_string())
}

fn path_key(path: &str) -> String {
    path.replace('/', "\\").to_ascii_lowercase()
}

fn non_empty_string(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn prepare_overwrite_target(target_path: &Path, temp_path: &Path) -> Result<(), BackendError> {
    if target_path.is_dir() {
        return Err(BackendError {
            code: "DESTINATION_INVALID",
            message: format!(
                "Cannot overwrite directory destination {}.",
                target_path.display()
            ),
        });
    }

    if target_path.exists() {
        std::fs::remove_file(target_path).map_err(|error| BackendError {
            code: "DESTINATION_INVALID",
            message: format!("Could not replace existing download file: {error}"),
        })?;
    }

    remove_file_if_exists(temp_path).map_err(internal_error)
}
