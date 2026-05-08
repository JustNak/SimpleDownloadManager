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
        self.enqueue_downloads_with_options(urls, source, bulk_archive_name, false)
            .await
    }

    pub async fn enqueue_downloads_with_options(
        &self,
        urls: Vec<String>,
        source: Option<DownloadSource>,
        bulk_archive_name: Option<String>,
        start_paused: bool,
    ) -> Result<Vec<EnqueueResult>, BackendError> {
        self.enqueue_downloads_with_bulk_options(
            urls,
            source,
            bulk_archive_name,
            start_paused,
            BulkArchiveOutputKind::Folder,
        )
        .await
    }

    pub async fn enqueue_downloads_with_bulk_options(
        &self,
        urls: Vec<String>,
        source: Option<DownloadSource>,
        bulk_archive_name: Option<String>,
        start_paused: bool,
        bulk_output_kind: BulkArchiveOutputKind,
    ) -> Result<Vec<EnqueueResult>, BackendError> {
        let entries = urls
            .into_iter()
            .map(|url| BatchDownloadEntry {
                url,
                filename_hint: None,
                resolved_from_url: None,
            })
            .collect();
        self.enqueue_download_entries_with_bulk_options(
            entries,
            source,
            bulk_archive_name,
            start_paused,
            bulk_output_kind,
        )
        .await
    }

    pub async fn enqueue_download_entries(
        &self,
        entries: Vec<BatchDownloadEntry>,
        source: Option<DownloadSource>,
        bulk_archive_name: Option<String>,
    ) -> Result<Vec<EnqueueResult>, BackendError> {
        self.enqueue_download_entries_with_options(entries, source, bulk_archive_name, false)
            .await
    }

    pub async fn enqueue_download_entries_with_options(
        &self,
        entries: Vec<BatchDownloadEntry>,
        source: Option<DownloadSource>,
        bulk_archive_name: Option<String>,
        start_paused: bool,
    ) -> Result<Vec<EnqueueResult>, BackendError> {
        self.enqueue_download_entries_with_bulk_options(
            entries,
            source,
            bulk_archive_name,
            start_paused,
            BulkArchiveOutputKind::Folder,
        )
        .await
    }

    pub async fn enqueue_download_entries_with_bulk_options(
        &self,
        entries: Vec<BatchDownloadEntry>,
        source: Option<DownloadSource>,
        bulk_archive_name: Option<String>,
        start_paused: bool,
        _bulk_output_kind: BulkArchiveOutputKind,
    ) -> Result<Vec<EnqueueResult>, BackendError> {
        let bulk_output_kind = BulkArchiveOutputKind::Folder;
        if entries.is_empty() {
            return Err(BackendError {
                code: "INVALID_URL",
                message: "Add at least one download URL.".into(),
            });
        }

        let normalized_entries = entries
            .into_iter()
            .map(|entry| {
                normalize_download_url(&entry.url).map(|url| BatchDownloadEntry {
                    url,
                    filename_hint: entry.filename_hint,
                    resolved_from_url: normalize_optional_resolved_from_url(
                        entry.resolved_from_url,
                    ),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let bulk_archive =
            if let Some(name) = bulk_archive_name.filter(|_| normalized_entries.len() > 1) {
                let name = normalize_bulk_output_name(&name, bulk_output_kind);
                let download_dir = {
                    let state = self.inner.read().await;
                    PathBuf::from(&state.settings.download_directory)
                };
                let output_path =
                    prepare_bulk_output_directory(&download_dir, bulk_output_kind)?.join(&name);
                if output_path.exists() {
                    return Err(BackendError {
                        code: "DESTINATION_EXISTS",
                        message: format!("Bulk output already exists: {}", output_path.display()),
                    });
                }
                Some(BulkArchiveInfo {
                    id: format!(
                        "bulk_{}_{}",
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|duration| duration.as_millis())
                            .unwrap_or_default(),
                        normalized_entries.len()
                    ),
                    name,
                    output_kind: bulk_output_kind,
                    archive_status: BulkArchiveStatus::Pending,
                    requires_extraction: None,
                    output_path: None,
                    error: None,
                    warning: None,
                    finalize_total_bytes: None,
                    finalize_processed_bytes: None,
                    finalize_mode: None,
                })
            } else {
                None
            };

        let mut results = Vec::with_capacity(normalized_entries.len());
        for entry in normalized_entries {
            results.push(
                self.enqueue_download_with_options(
                    entry.url,
                    EnqueueOptions {
                        source: source.clone(),
                        filename_hint: entry.filename_hint,
                        transfer_kind: Some(TransferKind::Http),
                        bulk_archive: bulk_archive.clone(),
                        start_paused,
                        resolved_from_url: entry.resolved_from_url,
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
        url: &str,
        auth: &HandoffAuth,
    ) -> Result<(), BackendError> {
        validate_handoff_auth_headers(auth)?;
        let settings = self.extension_integration_settings().await;
        if !protected_download_auth_allowed_for_url(url, &settings) {
            return Err(BackendError {
                code: "PERMISSION_DENIED",
                message:
                    "Protected Downloads browser session headers are not allowed for this site."
                        .into(),
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

fn normalize_optional_resolved_from_url(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
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

        let torrent_download_directory =
            if self.settings.torrent.download_directory.trim().is_empty() {
                default_torrent_download_directory_for(&self.settings.download_directory)
            } else {
                self.settings.torrent.download_directory.clone()
            };
        let default_directory = if transfer_kind == TransferKind::Torrent {
            &torrent_download_directory
        } else {
            &self.settings.download_directory
        };
        let directory = options
            .directory_override
            .as_deref()
            .unwrap_or(default_directory)
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
        let target_dir = if transfer_kind == TransferKind::Torrent {
            download_dir.clone()
        } else {
            prepare_category_download_directory(&download_dir, &filename)?
        };
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
            let job = self.remove_job_at_index(index);
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

        let initial_state = if options.start_paused {
            JobState::Paused
        } else {
            JobState::Queued
        };

        self.push_job(DownloadJob {
            id: job_id.clone(),
            url: url.clone(),
            filename: filename.clone(),
            source: options.source,
            transfer_kind,
            integrity_check,
            torrent: (transfer_kind == TransferKind::Torrent).then(TorrentInfo::default),
            state: initial_state,
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
            auto_restart_attempts: 0,
            resolved_from_url: options.resolved_from_url,
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
        let default_directory = if transfer_kind == TransferKind::Torrent {
            if self.settings.torrent.download_directory.trim().is_empty() {
                default_torrent_download_directory_for(&self.settings.download_directory)
            } else {
                self.settings.torrent.download_directory.clone()
            }
        } else {
            self.settings.download_directory.clone()
        };
        let target_path = if default_directory.trim().is_empty() {
            String::new()
        } else {
            let category_dir = if transfer_kind == TransferKind::Torrent {
                PathBuf::from(&default_directory)
            } else {
                category_download_directory(Path::new(&default_directory), &filename)
            };
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
            let category_dir = if transfer_kind == TransferKind::Torrent {
                PathBuf::from(&default_directory)
            } else {
                category_download_directory(Path::new(&default_directory), &filename)
            };
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

pub(super) fn protected_download_auth_allowed_for_url(
    url: &str,
    settings: &ExtensionIntegrationSettings,
) -> bool {
    if !settings.authenticated_handoff_enabled {
        return false;
    }

    match settings.protected_download_auth_scope {
        ProtectedDownloadAuthScope::LegacyGlobal => true,
        ProtectedDownloadAuthScope::Allowlist => {
            url_host_matches_patterns(url, &settings.authenticated_handoff_hosts)
        }
        ProtectedDownloadAuthScope::Off => false,
    }
}

fn url_host_matches_patterns(url: &str, patterns: &[String]) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    patterns
        .iter()
        .any(|pattern| host_matches_pattern(&host, pattern))
}

fn host_matches_pattern(host: &str, pattern: &str) -> bool {
    let pattern = pattern.trim().to_ascii_lowercase();
    if pattern.is_empty() {
        return false;
    }

    if let Some(suffix) = pattern.strip_prefix("*.") {
        return host
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('.') && prefix.len() > 1);
    }

    host == pattern || host.ends_with(&format!(".{pattern}"))
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
