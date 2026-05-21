use super::*;
use crate::archive_parts::{detect_archive_part, ArchivePartSuffix};
use crate::state::BulkArchiveEntry;
use crate::storage::BulkArchiveOutputKind;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex as StdMutex};

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
#[cfg(windows)]
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const ARCHIVE_EXTRACT_LOCK_RETRY_ATTEMPTS: usize = 8;
const BULK_FILE_OPERATION_RETRY_ATTEMPTS: usize = 8;
const BULK_COPY_CHUNK_BYTES: usize = 1024 * 1024;
const BULK_COPY_MAX_WORKERS: usize = 4;
const BULK_EXTRACT_MAX_WORKERS: usize = 2;
pub(super) const HUGE_BULK_ARCHIVE_THRESHOLD_BYTES: u64 = 100 * 1024 * 1024 * 1024;

#[cfg(test)]
const ARCHIVE_EXTRACT_LOCK_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(0);
#[cfg(not(test))]
const ARCHIVE_EXTRACT_LOCK_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(500);
#[cfg(test)]
const BULK_FILE_OPERATION_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(0);
#[cfg(not(test))]
const BULK_FILE_OPERATION_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BulkFinalizeProgressUpdate {
    pub(super) processed_bytes: u64,
    pub(super) total_bytes: u64,
}

pub(super) trait BulkFinalizeProgressListener: Send + Sync {
    fn report(&self, update: BulkFinalizeProgressUpdate);
}

#[derive(Clone)]
pub(super) struct BulkFinalizeProgressReporter {
    inner: Arc<BulkFinalizeProgressState>,
}

struct BulkFinalizeProgressState {
    processed_bytes: AtomicU64,
    total_bytes: AtomicU64,
    listener: Option<Arc<dyn BulkFinalizeProgressListener>>,
}

impl BulkFinalizeProgressReporter {
    pub(super) fn new<L>(total_bytes: u64, listener: Arc<L>) -> Self
    where
        L: BulkFinalizeProgressListener + 'static,
    {
        Self::with_listener(total_bytes, listener)
    }

    pub(super) fn with_listener(
        total_bytes: u64,
        listener: Arc<dyn BulkFinalizeProgressListener>,
    ) -> Self {
        let reporter = Self {
            inner: Arc::new(BulkFinalizeProgressState {
                processed_bytes: AtomicU64::new(0),
                total_bytes: AtomicU64::new(total_bytes),
                listener: Some(listener),
            }),
        };
        reporter.emit_current();
        reporter
    }

    #[cfg(test)]
    pub(super) fn silent(total_bytes: u64) -> Self {
        Self {
            inner: Arc::new(BulkFinalizeProgressState {
                processed_bytes: AtomicU64::new(0),
                total_bytes: AtomicU64::new(total_bytes),
                listener: None,
            }),
        }
    }

    fn add_total(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        self.inner.total_bytes.fetch_add(bytes, Ordering::SeqCst);
        self.emit_current();
    }

    fn add_processed(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let mut current = self.inner.processed_bytes.load(Ordering::SeqCst);
        loop {
            let total = self.inner.total_bytes.load(Ordering::SeqCst);
            let next = current.saturating_add(bytes);
            let bounded = if total > 0 { next.min(total) } else { next };
            if bounded == current {
                return;
            }
            match self.inner.processed_bytes.compare_exchange(
                current,
                bounded,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    self.emit_current();
                    return;
                }
                Err(next_current) => current = next_current,
            }
        }
    }

    pub(super) fn current(&self) -> BulkFinalizeProgressUpdate {
        BulkFinalizeProgressUpdate {
            processed_bytes: self.inner.processed_bytes.load(Ordering::SeqCst),
            total_bytes: self.inner.total_bytes.load(Ordering::SeqCst),
        }
    }

    fn emit_current(&self) {
        if let Some(listener) = &self.inner.listener {
            listener.report(self.current());
        }
    }
}

#[cfg(test)]
pub(super) fn bulk_archive_needs_extraction(entries: &[BulkArchiveEntry]) -> bool {
    build_bulk_archive_source_plan(entries)
        .map(|plan| !plan.archive_sets.is_empty())
        .unwrap_or(true)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BulkFinalizationPlan {
    pub(super) total_completed_bytes: u64,
    pub(super) output_kind: BulkArchiveOutputKind,
    pub(super) requires_extraction: bool,
    pub(super) scratch_space_bytes: u64,
    pub(super) same_volume_move_capable: bool,
    pub(super) finalize_mode: BulkFinalizeMode,
    pub(super) warning: Option<String>,
}

pub(super) fn bulk_finalization_plan(
    archive: &BulkArchiveReady,
) -> Result<BulkFinalizationPlan, String> {
    let source_plan = build_bulk_archive_source_plan(&archive.entries)?;
    let total_completed_bytes = archive.entries.iter().try_fold(0_u64, |total, entry| {
        let metadata = std::fs::metadata(&entry.source_path).map_err(|error| {
            format!(
                "Could not inspect completed bulk member {}: {error}",
                entry.source_path.display()
            )
        })?;
        Ok::<u64, String>(total.saturating_add(metadata.len()))
    })?;
    let requires_extraction = !source_plan.archive_sets.is_empty();
    let finalize_mode = if requires_extraction {
        BulkFinalizeMode::Extract
    } else {
        BulkFinalizeMode::Move
    };
    let scratch_space_bytes = if requires_extraction {
        total_completed_bytes
    } else {
        0
    };
    let same_volume_move_capable = archive
        .entries
        .iter()
        .all(|entry| same_volume_for_bulk_move(&entry.source_path, &archive.output_path));
    let required_free_space = if requires_extraction {
        scratch_space_bytes
    } else if same_volume_move_capable {
        0
    } else {
        total_completed_bytes
    };
    ensure_bulk_finalization_free_space(&archive.output_path, required_free_space)?;

    Ok(BulkFinalizationPlan {
        total_completed_bytes,
        output_kind: BulkArchiveOutputKind::Folder,
        requires_extraction,
        scratch_space_bytes,
        same_volume_move_capable,
        finalize_mode,
        warning: huge_bulk_folder_warning(total_completed_bytes),
    })
}

pub(super) fn prepare_bulk_archive_sources_without_extraction(
    archive: BulkArchiveReady,
) -> Result<PreparedBulkArchive, String> {
    let plan = build_bulk_archive_source_plan(&archive.entries)?;
    if !plan.archive_sets.is_empty() {
        return Err("Archive extraction requires the bundled 7-Zip sidecar.".into());
    }
    let output_path = archive.output_path;
    let entries = prepare_entries_for_output(plan.raw_entries)?;

    Ok(PreparedBulkArchive {
        output_path,
        entries,
        cleanup_paths: Vec::new(),
        staging_root: None,
    })
}

pub(super) fn prepare_bulk_archive_sources_with_7zip_and_progress(
    archive: BulkArchiveReady,
    seven_zip_path: PathBuf,
    progress: BulkFinalizeProgressReporter,
) -> Result<PreparedBulkArchive, String> {
    let extractor = SevenZipArchiveExtractor {
        executable: seven_zip_path,
    };
    prepare_bulk_archive_sources_with_extractor_and_progress(archive, &extractor, progress)
}

#[cfg(test)]
pub(super) fn prepare_bulk_archive_sources_with_extractor(
    archive: BulkArchiveReady,
    extractor: &impl ArchiveExtractor,
) -> Result<PreparedBulkArchive, String> {
    prepare_bulk_archive_sources_with_extractor_and_progress(
        archive,
        extractor,
        BulkFinalizeProgressReporter::silent(0),
    )
}

pub(super) fn prepare_bulk_archive_sources_with_extractor_and_progress(
    archive: BulkArchiveReady,
    extractor: &impl ArchiveExtractor,
    progress: BulkFinalizeProgressReporter,
) -> Result<PreparedBulkArchive, String> {
    let plan = build_bulk_archive_source_plan(&archive.entries)?;
    if plan.archive_sets.is_empty() {
        let output_path = archive.output_path;
        let entries = prepare_entries_for_output(plan.raw_entries)?;
        return Ok(PreparedBulkArchive {
            output_path,
            entries,
            cleanup_paths: Vec::new(),
            staging_root: None,
        });
    }

    let output_path = archive.output_path;
    let staging_root = archive_staging_root(&output_path)?;
    let extract_dir = staging_root.join("extracted");
    std::fs::create_dir_all(&extract_dir)
        .map_err(|error| format!("Could not create archive extraction directory: {error}"))?;

    let result: Result<PreparedBulkArchive, String> = (|| {
        let mut entries = extract_archive_sets(
            extractor,
            &plan.archive_sets,
            &extract_dir,
            progress.clone(),
        )?;

        entries.extend(plan.raw_entries);
        if entries.is_empty() {
            return Err("Archive extraction did not produce any files to combine.".into());
        }
        let entries = prepare_entries_for_output(entries)?;

        Ok(PreparedBulkArchive {
            output_path,
            entries,
            cleanup_paths: plan
                .archive_sets
                .iter()
                .flat_map(|archive_set| {
                    archive_set
                        .members
                        .iter()
                        .map(|entry| entry.source_path.clone())
                })
                .collect(),
            staging_root: Some(staging_root.clone()),
        })
    })();

    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging_root);
    }

    result
}

#[cfg(test)]
pub(super) fn finish_prepared_bulk_archive_sync(
    prepared: PreparedBulkArchive,
) -> Result<BulkArchiveCreateOutcome, String> {
    finish_prepared_bulk_archive_sync_with_progress(
        prepared,
        BulkFinalizeProgressReporter::silent(0),
    )
}

pub(super) fn finish_prepared_bulk_archive_sync_with_progress(
    prepared: PreparedBulkArchive,
    progress: BulkFinalizeProgressReporter,
) -> Result<BulkArchiveCreateOutcome, String> {
    finish_prepared_bulk_archive_sync_with_options(
        prepared,
        progress,
        BulkFolderFinalizeOptions::default(),
    )
}

fn finish_prepared_bulk_archive_sync_with_options(
    prepared: PreparedBulkArchive,
    progress: BulkFinalizeProgressReporter,
    options: BulkFolderFinalizeOptions,
) -> Result<BulkArchiveCreateOutcome, String> {
    if prepared.output_path.exists() {
        return Err(format!(
            "Bulk output already exists: {}",
            prepared.output_path.display()
        ));
    }
    cleanup_stale_archive_staging_roots(&prepared.output_path, prepared.staging_root.as_deref())?;

    if let Some(parent) = prepared.output_path.parent() {
        retry_bulk_file_operation("Could not create archive output directory", || {
            std::fs::create_dir_all(parent)
        })?;
    }

    let temp_path = temporary_output_path(&prepared.output_path)?;
    if let Some(staging_root) = prepared.staging_root.as_deref() {
        progress.add_total(source_bytes_inside_root(&prepared.entries, staging_root)?);
    }
    let result = finish_prepared_folder_output(&prepared, &temp_path, &progress, options);

    if result.is_err() {
        let _ = remove_incomplete_output(&temp_path);
        if let Some(staging_root) = &prepared.staging_root {
            let _ = std::fs::remove_dir_all(staging_root);
        }
    }

    let folder_outcome = result?;
    verify_finished_output(&folder_outcome.output_path)?;
    let cleanup_paths = prepared
        .cleanup_paths
        .iter()
        .chain(folder_outcome.copy_cleanup_paths.iter())
        .cloned()
        .collect::<Vec<_>>();
    let cleanup_warnings =
        cleanup_original_archive_parts(&cleanup_paths, &folder_outcome.moved_source_paths);
    if let Some(staging_root) = &prepared.staging_root {
        let _ = std::fs::remove_dir_all(staging_root);
    }

    Ok(BulkArchiveCreateOutcome {
        output_path: folder_outcome.output_path,
        cleanup_warnings,
    })
}

#[cfg(test)]
pub(super) fn finish_prepared_bulk_archive_sync_with_options_for_test(
    prepared: PreparedBulkArchive,
    progress: BulkFinalizeProgressReporter,
    options: BulkFolderFinalizeOptions,
) -> Result<BulkArchiveCreateOutcome, String> {
    finish_prepared_bulk_archive_sync_with_options(prepared, progress, options)
}

#[derive(Debug)]
pub(super) struct BulkArchiveCreateOutcome {
    pub(super) output_path: PathBuf,
    pub(super) cleanup_warnings: Vec<String>,
}

#[derive(Debug)]
pub(super) struct PreparedBulkArchive {
    pub(super) output_path: PathBuf,
    pub(super) entries: Vec<BulkArchiveEntry>,
    pub(super) cleanup_paths: Vec<PathBuf>,
    pub(super) staging_root: Option<PathBuf>,
}

#[derive(Debug)]
struct FolderFinalizeOutcome {
    output_path: PathBuf,
    moved_source_paths: Vec<PathBuf>,
    copy_cleanup_paths: Vec<PathBuf>,
}

#[derive(Debug)]
pub(super) struct MovedFolderEntry {
    pub(super) source_path: PathBuf,
    pub(super) destination_path: PathBuf,
    pub(super) rollback_required: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct BulkFolderFinalizeOptions {
    force_copy: bool,
}

impl BulkFolderFinalizeOptions {
    #[cfg(test)]
    pub(super) fn force_copy_for_test() -> Self {
        Self { force_copy: true }
    }
}

#[derive(Debug, Clone)]
pub(super) struct BulkArchiveSourcePlan {
    pub(super) raw_entries: Vec<BulkArchiveEntry>,
    pub(super) archive_sets: Vec<BulkArchiveSet>,
}

#[derive(Debug, Clone)]
pub(super) struct BulkArchiveSet {
    pub(super) first_part: BulkArchiveEntry,
    pub(super) members: Vec<BulkArchiveEntry>,
}

pub(super) trait ArchiveExtractor: Sync {
    fn extract(&self, first_part: &Path, output_dir: &Path) -> Result<(), String>;
}

fn extract_archive_sets(
    extractor: &impl ArchiveExtractor,
    archive_sets: &[BulkArchiveSet],
    extract_dir: &Path,
    progress: BulkFinalizeProgressReporter,
) -> Result<Vec<BulkArchiveEntry>, String> {
    if archive_sets.is_empty() {
        return Ok(Vec::new());
    }

    let worker_count = bulk_extraction_parallelism().min(archive_sets.len());
    if worker_count <= 1 {
        let mut entries = Vec::new();
        for (index, archive_set) in archive_sets.iter().enumerate() {
            let (_, mut set_entries) =
                extract_archive_set(extractor, index, archive_set, extract_dir, &progress)?;
            entries.append(&mut set_entries);
        }
        return Ok(entries);
    }

    let queue = Arc::new(StdMutex::new(
        archive_sets
            .iter()
            .cloned()
            .enumerate()
            .collect::<VecDeque<_>>(),
    ));
    let failed = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = std_mpsc::channel::<Result<(usize, Vec<BulkArchiveEntry>), String>>();

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let queue = queue.clone();
            let failed = failed.clone();
            let sender = sender.clone();
            let progress = progress.clone();
            scope.spawn(move || loop {
                if failed.load(Ordering::SeqCst) {
                    break;
                }
                let Some((index, archive_set)) = queue.lock().unwrap().pop_front() else {
                    break;
                };
                let result =
                    extract_archive_set(extractor, index, &archive_set, extract_dir, &progress);
                if result.is_err() {
                    failed.store(true, Ordering::SeqCst);
                }
                if sender.send(result).is_err() {
                    break;
                }
            });
        }
        drop(sender);

        let mut results = Vec::with_capacity(archive_sets.len());
        let mut first_error = None;
        for result in receiver {
            match result {
                Ok(extracted) => results.push(extracted),
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        results.sort_by_key(|(index, _)| *index);
        Ok(results
            .into_iter()
            .flat_map(|(_, entries)| entries)
            .collect::<Vec<_>>())
    })
}

fn extract_archive_set(
    extractor: &impl ArchiveExtractor,
    index: usize,
    archive_set: &BulkArchiveSet,
    extract_dir: &Path,
    progress: &BulkFinalizeProgressReporter,
) -> Result<(usize, Vec<BulkArchiveEntry>), String> {
    let set_extract_dir = extract_dir.join(format!("set-{index}"));
    std::fs::create_dir_all(&set_extract_dir)
        .map_err(|error| format!("Could not create archive extraction directory: {error}"))?;
    extract_with_lock_retry(
        extractor,
        &archive_set.first_part.source_path,
        &set_extract_dir,
    )?;

    let set_entries = collect_zip_entries_from_directory(&set_extract_dir)?;
    if set_entries.is_empty() {
        return Err(format!(
            "Archive extraction for {} did not produce any files.",
            archive_part_display_name(&archive_set.first_part.source_path)
        ));
    }
    progress.add_processed(archive_set_input_bytes(archive_set)?);
    Ok((index, set_entries))
}

fn extract_with_lock_retry(
    extractor: &impl ArchiveExtractor,
    first_part: &Path,
    output_dir: &Path,
) -> Result<(), String> {
    let mut last_lock_error = None;
    for attempt in 0..ARCHIVE_EXTRACT_LOCK_RETRY_ATTEMPTS {
        match extractor.extract(first_part, output_dir) {
            Ok(()) => return Ok(()),
            Err(error)
                if is_archive_file_lock_error(&error)
                    && attempt + 1 < ARCHIVE_EXTRACT_LOCK_RETRY_ATTEMPTS =>
            {
                last_lock_error = Some(error);
                std::thread::sleep(ARCHIVE_EXTRACT_LOCK_RETRY_DELAY);
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_lock_error.unwrap_or_else(|| archive_file_lock_message(first_part)))
}

pub(super) struct SevenZipArchiveExtractor {
    executable: PathBuf,
}

impl ArchiveExtractor for SevenZipArchiveExtractor {
    fn extract(&self, first_part: &Path, output_dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(output_dir)
            .map_err(|error| format!("Could not create archive extraction directory: {error}"))?;

        let mut command = Command::new(&self.executable);
        command
            .arg("x")
            .arg(first_part)
            .arg(format!("-o{}", output_dir.display()))
            .arg("-y")
            .arg("-mmt=on")
            .arg("-bso0")
            .arg("-bsp0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        if let Some(parent) = self.executable.parent() {
            command.current_dir(parent);
        }

        let output = command.output().map_err(|error| {
            format!(
                "Archive extraction failed for {}: could not run bundled 7-Zip: {error}",
                archive_part_display_name(first_part)
            )
        })?;

        if output.status.success() {
            return Ok(());
        }

        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Err(seven_zip_failure_message(
            first_part,
            output.status.code(),
            &combined,
        ))
    }
}

pub(super) fn build_bulk_archive_source_plan(
    entries: &[BulkArchiveEntry],
) -> Result<BulkArchiveSourcePlan, String> {
    let mut builders: Vec<ArchiveGroupBuilder> = Vec::new();
    let mut group_indexes = HashMap::new();
    let mut grouped_indexes = BTreeSet::new();

    for (index, entry) in entries.iter().enumerate() {
        let Some(part) = detect_archive_part(&entry.archive_name) else {
            continue;
        };

        grouped_indexes.insert(index);
        let group_index = if let Some(group_index) = group_indexes.get(&part.key) {
            *group_index
        } else {
            let group_index = builders.len();
            group_indexes.insert(part.key.clone(), group_index);
            builders.push(ArchiveGroupBuilder {
                display_prefix: part.display_prefix.clone(),
                suffix: part.suffix,
                number_width: part.number_width,
                parts: Vec::new(),
            });
            group_index
        };

        let builder = &mut builders[group_index];
        builder.number_width = builder.number_width.max(part.number_width);
        builder.parts.push(ArchiveGroupPart {
            part_number: part.part_number,
            entry: entry.clone(),
        });
    }

    let mut archive_sets = Vec::with_capacity(builders.len());
    for mut builder in builders {
        builder.parts.sort_by_key(|part| part.part_number);
        validate_archive_group_sequence(&builder)?;
        let first_part = builder
            .parts
            .iter()
            .find(|part| part.part_number == 1)
            .map(|part| part.entry.clone())
            .ok_or_else(|| {
                format!(
                    "Archive set is missing first part {}.",
                    builder.expected_part_name(1)
                )
            })?;
        archive_sets.push(BulkArchiveSet {
            first_part,
            members: builder.parts.into_iter().map(|part| part.entry).collect(),
        });
    }

    let raw_entries = entries
        .iter()
        .enumerate()
        .filter(|(index, _)| !grouped_indexes.contains(index))
        .map(|(_, entry)| entry.clone())
        .collect();

    Ok(BulkArchiveSourcePlan {
        raw_entries,
        archive_sets,
    })
}

pub(super) fn seven_zip_failure_message(
    first_part: &Path,
    exit_code: Option<i32>,
    output: &str,
) -> String {
    let display_name = archive_part_display_name(first_part);
    let lower = output.to_ascii_lowercase();

    if lower.contains("password")
        || lower.contains("encrypted")
        || lower.contains("can not open encrypted")
    {
        return format!(
            "Archive extraction failed for {display_name}: password is required or incorrect."
        );
    }

    if lower.contains("crc failed") || lower.contains("checksum") {
        return format!(
            "Archive extraction failed for {display_name}: archive data failed CRC validation."
        );
    }

    if lower.contains("missing volume")
        || lower.contains("missing part")
        || lower.contains("unexpected end")
        || lower.contains("cannot find archive")
        || lower.contains("can not open the file as archive")
    {
        return format!(
            "Archive extraction failed for {display_name}: one or more archive parts are missing."
        );
    }

    if is_archive_file_lock_error(output) {
        return archive_file_lock_message(first_part);
    }

    let code = exit_code
        .map(|code| format!("7-Zip exited with code {code}"))
        .unwrap_or_else(|| "7-Zip exited without a status code".into());
    let detail = output.trim();
    if detail.is_empty() {
        format!("Archive extraction failed for {display_name}: {code}.")
    } else {
        format!("Archive extraction failed for {display_name}: {code}. {detail}")
    }
}

fn archive_file_lock_message(first_part: &Path) -> String {
    format!(
        "Archive extraction failed for {}: downloaded archive part is still locked by another process. Retry archive creation in a moment.",
        archive_part_display_name(first_part)
    )
}

fn is_archive_file_lock_error(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("being used by another process")
        || lower.contains("used by another process")
        || lower.contains("another process is using")
        || lower.contains("sharing violation")
        || lower.contains("cannot access the file because it is being used")
        || lower.contains("still locked")
}

#[derive(Debug)]
struct ArchiveGroupBuilder {
    display_prefix: String,
    suffix: ArchivePartSuffix,
    number_width: usize,
    parts: Vec<ArchiveGroupPart>,
}

impl ArchiveGroupBuilder {
    fn expected_part_name(&self, number: u32) -> String {
        match self.suffix {
            ArchivePartSuffix::PartRar => {
                format!(
                    "{}.part{:0width$}.rar",
                    self.display_prefix,
                    number,
                    width = self.number_width.max(1)
                )
            }
            ArchivePartSuffix::Numbered => {
                format!(
                    "{}.{:0width$}",
                    self.display_prefix,
                    number,
                    width = self.number_width.max(3)
                )
            }
            ArchivePartSuffix::LegacyRar => {
                if number == 1 {
                    format!("{}.rar", self.display_prefix)
                } else {
                    format!("{}.r{:02}", self.display_prefix, number.saturating_sub(2))
                }
            }
        }
    }
}

#[derive(Debug)]
struct ArchiveGroupPart {
    part_number: u32,
    entry: BulkArchiveEntry,
}

fn validate_archive_group_sequence(builder: &ArchiveGroupBuilder) -> Result<(), String> {
    let Some(max_part_number) = builder.parts.iter().map(|part| part.part_number).max() else {
        return Ok(());
    };

    let present = builder
        .parts
        .iter()
        .map(|part| part.part_number)
        .collect::<BTreeSet<_>>();
    for number in 1..=max_part_number {
        if !present.contains(&number) {
            return Err(format!(
                "Archive set is missing part {}.",
                builder.expected_part_name(number)
            ));
        }
    }

    Ok(())
}

fn collect_zip_entries_from_directory(root: &Path) -> Result<Vec<BulkArchiveEntry>, String> {
    let mut entries = Vec::new();
    collect_zip_entries_from_directory_inner(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.archive_name.cmp(&right.archive_name));
    Ok(entries)
}

fn collect_zip_entries_from_directory_inner(
    root: &Path,
    current: &Path,
    entries: &mut Vec<BulkArchiveEntry>,
) -> Result<(), String> {
    let mut children = std::fs::read_dir(current)
        .map_err(|error| format!("Could not read extracted archive directory: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Could not read extracted archive directory: {error}"))?;
    children.sort_by_key(|entry| entry.path());

    for child in children {
        let path = child.path();
        let file_type = child
            .file_type()
            .map_err(|error| format!("Could not inspect extracted archive file: {error}"))?;
        if file_type.is_symlink() {
            return Err(format!(
                "Unsupported extracted archive entry: {}",
                path.display()
            ));
        }
        if file_type.is_dir() {
            collect_zip_entries_from_directory_inner(root, &path, entries)?;
            continue;
        }
        if !file_type.is_file() {
            return Err(format!(
                "Unsupported extracted archive entry: {}",
                path.display()
            ));
        }
        let relative = path.strip_prefix(root).map_err(|error| {
            format!(
                "Could not calculate archive path for {}: {error}",
                path.display()
            )
        })?;
        let archive_name = relative
            .to_string_lossy()
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_string();
        validate_zip_entry_name(&archive_name)?;
        entries.push(BulkArchiveEntry {
            source_path: path,
            archive_name,
        });
    }

    Ok(())
}

fn prepare_entries_for_output(
    entries: Vec<BulkArchiveEntry>,
) -> Result<Vec<BulkArchiveEntry>, String> {
    let entries = entries
        .into_iter()
        .map(|mut entry| {
            entry.archive_name = validate_zip_entry_name(&entry.archive_name)?;
            Ok(entry)
        })
        .collect::<Result<Vec<_>, String>>()?;
    reject_duplicate_output_paths(&entries)?;
    Ok(entries)
}

fn reject_duplicate_output_paths(entries: &[BulkArchiveEntry]) -> Result<(), String> {
    let mut seen = HashMap::new();
    for entry in entries {
        let normalized = validate_zip_entry_name(&entry.archive_name)?;
        let key = normalized.to_lowercase();
        if let Some(existing) = seen.insert(key, normalized.clone()) {
            return Err(format!(
                "Duplicate bulk output path {normalized} conflicts with {existing}."
            ));
        }
    }
    Ok(())
}

fn huge_bulk_folder_warning(total_completed_bytes: u64) -> Option<String> {
    (total_completed_bytes >= HUGE_BULK_ARCHIVE_THRESHOLD_BYTES).then(|| {
        "Bulk output is 100 GiB or larger; finalization will use folder moves/extraction.".into()
    })
}

fn archive_set_input_bytes(archive_set: &BulkArchiveSet) -> Result<u64, String> {
    archive_set.members.iter().try_fold(0_u64, |total, entry| {
        file_len_for_bulk_entry(&entry.source_path).map(|bytes| total.saturating_add(bytes))
    })
}

fn file_len_for_bulk_entry(path: &Path) -> Result<u64, String> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        format!(
            "Could not inspect completed bulk member {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!(
            "Could not finalize {} because it is not a file.",
            path.display()
        ));
    }
    Ok(metadata.len())
}

fn bulk_copy_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(2)
        .clamp(1, BULK_COPY_MAX_WORKERS)
}

fn bulk_extraction_parallelism() -> usize {
    #[cfg(test)]
    {
        BULK_EXTRACT_MAX_WORKERS.min(1)
    }

    #[cfg(not(test))]
    {
        std::thread::available_parallelism()
            .map(|parallelism| (parallelism.get() / 2).max(1))
            .unwrap_or(1)
            .clamp(1, BULK_EXTRACT_MAX_WORKERS)
    }
}

fn ensure_bulk_finalization_free_space(
    output_path: &Path,
    required_bytes: u64,
) -> Result<(), String> {
    if required_bytes == 0 {
        return Ok(());
    }
    let Some(available_bytes) = best_effort_available_space(output_path) else {
        return Ok(());
    };
    if available_bytes < required_bytes {
        return Err(format!(
            "Not enough free space to finalize bulk output at {}: need at least {}, available {}.",
            output_path.display(),
            format_bytes_for_error(required_bytes),
            format_bytes_for_error(available_bytes)
        ));
    }
    Ok(())
}

fn format_bytes_for_error(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else {
        format!("{bytes} bytes")
    }
}

#[cfg(windows)]
fn best_effort_available_space(output_path: &Path) -> Option<u64> {
    let probe = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(output_path);
    let wide = probe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut available_bytes = 0_u64;
    let success = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available_bytes,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (success != 0).then_some(available_bytes)
}

#[cfg(not(windows))]
fn best_effort_available_space(_output_path: &Path) -> Option<u64> {
    None
}

fn same_volume_for_bulk_move(source: &Path, output_path: &Path) -> bool {
    source.components().next() == output_path.components().next()
}

fn temporary_output_path(output_path: &Path) -> Result<PathBuf, String> {
    archive_staging_root(output_path)
}

fn finish_prepared_folder_output(
    prepared: &PreparedBulkArchive,
    temp_path: &Path,
    progress: &BulkFinalizeProgressReporter,
    options: BulkFolderFinalizeOptions,
) -> Result<FolderFinalizeOutcome, String> {
    retry_bulk_file_operation("Could not create bulk output folder", || {
        std::fs::create_dir_all(temp_path)
    })?;
    let move_outcome = move_or_copy_entries_to_folder_with_options(
        &prepared.entries,
        temp_path,
        prepared.staging_root.as_deref(),
        progress,
        options,
    )?;
    let finalize_result =
        retry_bulk_file_operation("Could not finalize bulk output folder", || {
            std::fs::rename(temp_path, &prepared.output_path)
        });
    if let Err(error) = finalize_result {
        rollback_moved_raw_entries(&move_outcome.moved_entries);
        return Err(error);
    }

    Ok(FolderFinalizeOutcome {
        output_path: prepared.output_path.clone(),
        moved_source_paths: move_outcome
            .moved_entries
            .iter()
            .filter(|entry| entry.rollback_required)
            .map(|entry| entry.source_path.clone())
            .collect(),
        copy_cleanup_paths: move_outcome.copy_cleanup_paths,
    })
}

#[derive(Debug)]
pub(super) struct MoveOrCopyOutcome {
    pub(super) moved_entries: Vec<MovedFolderEntry>,
    pub(super) copy_cleanup_paths: Vec<PathBuf>,
}

#[cfg(test)]
pub(super) fn move_or_copy_entries_to_folder_with_progress(
    entries: &[BulkArchiveEntry],
    output_root: &Path,
    staging_root: Option<&Path>,
    progress: &BulkFinalizeProgressReporter,
) -> Result<MoveOrCopyOutcome, String> {
    move_or_copy_entries_to_folder_with_options(
        entries,
        output_root,
        staging_root,
        progress,
        BulkFolderFinalizeOptions::default(),
    )
}

fn move_or_copy_entries_to_folder_with_options(
    entries: &[BulkArchiveEntry],
    output_root: &Path,
    staging_root: Option<&Path>,
    progress: &BulkFinalizeProgressReporter,
    options: BulkFolderFinalizeOptions,
) -> Result<MoveOrCopyOutcome, String> {
    let mut moved_entries = Vec::new();
    let mut copy_cleanup_paths = Vec::new();
    let mut copy_jobs = Vec::new();

    for entry in entries {
        let destination = match destination_path_for_entry(output_root, entry) {
            Ok(destination) => destination,
            Err(error) => {
                rollback_moved_raw_entries(&moved_entries);
                return Err(error);
            }
        };
        let rollback_required = !is_path_inside_root(&entry.source_path, staging_root);
        if options.force_copy || !same_volume_for_bulk_move(&entry.source_path, &destination) {
            let bytes = match file_len_for_bulk_entry(&entry.source_path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    rollback_moved_raw_entries(&moved_entries);
                    return Err(error);
                }
            };
            copy_jobs.push(FolderCopyJob {
                source_path: entry.source_path.clone(),
                destination_path: destination,
                bytes,
            });
            continue;
        }

        let source_bytes = match file_len_for_bulk_entry(&entry.source_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                rollback_moved_raw_entries(&moved_entries);
                return Err(error);
            }
        };
        match move_entry_to_folder(entry, &destination, rollback_required) {
            Ok(moved) => {
                progress.add_processed(source_bytes);
                moved_entries.push(moved);
            }
            Err(_) if can_fallback_to_copy(&entry.source_path, &destination) => {
                if let Err(error) =
                    copy_file_checked_with_progress(&entry.source_path, &destination, progress)
                {
                    rollback_moved_raw_entries(&moved_entries);
                    return Err(error);
                }
                copy_cleanup_paths.push(entry.source_path.clone());
            }
            Err(error) => {
                rollback_moved_raw_entries(&moved_entries);
                return Err(error);
            }
        }
    }

    if !copy_jobs.is_empty() {
        match copy_entries_to_folder_parallel(copy_jobs, output_root, progress) {
            Ok(mut copied_paths) => copy_cleanup_paths.append(&mut copied_paths),
            Err(error) => {
                rollback_moved_raw_entries(&moved_entries);
                return Err(error);
            }
        }
    }

    Ok(MoveOrCopyOutcome {
        moved_entries,
        copy_cleanup_paths,
    })
}

fn destination_path_for_entry(
    output_root: &Path,
    entry: &BulkArchiveEntry,
) -> Result<PathBuf, String> {
    let archive_name = validate_zip_entry_name(&entry.archive_name)?;
    let mut destination = output_root.to_path_buf();
    for part in archive_name.split('/') {
        destination.push(part);
    }
    if let Some(parent) = destination.parent() {
        retry_bulk_file_operation("Could not create bulk output folder", || {
            std::fs::create_dir_all(parent)
        })?;
    }
    if destination.exists() {
        return Err(format!(
            "Bulk output file already exists while finalizing: {}",
            destination.display()
        ));
    }
    Ok(destination)
}

fn move_entry_to_folder(
    entry: &BulkArchiveEntry,
    destination: &Path,
    rollback_required: bool,
) -> Result<MovedFolderEntry, String> {
    retry_bulk_file_operation(
        &format!(
            "Could not move {} into bulk output folder",
            entry.source_path.display()
        ),
        || std::fs::rename(&entry.source_path, destination),
    )?;
    Ok(MovedFolderEntry {
        source_path: entry.source_path.clone(),
        destination_path: destination.to_path_buf(),
        rollback_required,
    })
}

fn can_fallback_to_copy(source: &Path, destination: &Path) -> bool {
    source.is_file() && !destination.exists()
}

#[derive(Debug, Clone)]
struct FolderCopyJob {
    source_path: PathBuf,
    destination_path: PathBuf,
    bytes: u64,
}

fn copy_entries_to_folder_parallel(
    copy_jobs: Vec<FolderCopyJob>,
    output_root: &Path,
    progress: &BulkFinalizeProgressReporter,
) -> Result<Vec<PathBuf>, String> {
    let required_bytes = copy_jobs
        .iter()
        .fold(0_u64, |total, job| total.saturating_add(job.bytes));
    ensure_bulk_finalization_free_space(output_root, required_bytes)?;

    let worker_count = bulk_copy_parallelism().min(copy_jobs.len());
    if worker_count <= 1 {
        let mut cleanup_paths = Vec::with_capacity(copy_jobs.len());
        for job in copy_jobs {
            copy_file_checked_with_progress(&job.source_path, &job.destination_path, progress)?;
            cleanup_paths.push(job.source_path);
        }
        return Ok(cleanup_paths);
    }

    let queue = Arc::new(StdMutex::new(
        copy_jobs.into_iter().collect::<VecDeque<_>>(),
    ));
    let failed = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = std_mpsc::channel::<Result<PathBuf, String>>();

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let queue = queue.clone();
            let failed = failed.clone();
            let sender = sender.clone();
            let progress = progress.clone();
            scope.spawn(move || loop {
                if failed.load(Ordering::SeqCst) {
                    break;
                }
                let Some(job) = queue.lock().unwrap().pop_front() else {
                    break;
                };
                let result = copy_file_checked_with_progress(
                    &job.source_path,
                    &job.destination_path,
                    &progress,
                )
                .map(|_| job.source_path);
                if result.is_err() {
                    failed.store(true, Ordering::SeqCst);
                }
                if sender.send(result).is_err() {
                    break;
                }
            });
        }
        drop(sender);

        let mut cleanup_paths = Vec::new();
        let mut first_error = None;
        for result in receiver {
            match result {
                Ok(path) => cleanup_paths.push(path),
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }

        if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(cleanup_paths)
        }
    })
}

fn copy_file_checked_with_progress(
    source: &Path,
    destination: &Path,
    progress: &BulkFinalizeProgressReporter,
) -> Result<(), String> {
    let source_metadata = std::fs::metadata(source).map_err(|error| {
        format!(
            "Could not inspect {} before copying into bulk output folder: {error}",
            source.display()
        )
    })?;
    if !source_metadata.is_file() {
        return Err(format!(
            "Could not copy {} into bulk output folder because it is not a file.",
            source.display()
        ));
    }

    let copied = copy_file_chunked(source, destination, progress).map_err(|error| {
        format!(
            "Could not copy {} into bulk output folder: {error}",
            source.display()
        )
    })?;
    if copied != source_metadata.len() {
        return Err(format!(
            "Could not verify copied bulk output file {}: copied {copied} bytes but expected {} bytes.",
            destination.display(),
            source_metadata.len()
        ));
    }

    let destination_metadata = std::fs::metadata(destination).map_err(|error| {
        format!(
            "Could not inspect copied bulk output file {}: {error}",
            destination.display()
        )
    })?;
    if !destination_metadata.is_file() || destination_metadata.len() != source_metadata.len() {
        return Err(format!(
            "Could not verify copied bulk output file {}: destination size does not match source.",
            destination.display()
        ));
    }
    Ok(())
}

fn copy_file_chunked(
    source: &Path,
    destination: &Path,
    progress: &BulkFinalizeProgressReporter,
) -> Result<u64, std::io::Error> {
    if destination.exists() {
        std::fs::remove_file(destination)?;
    }
    let mut input = std::fs::File::open(source)?;
    let mut output = std::fs::File::create(destination)?;
    let mut buffer = vec![0_u8; BULK_COPY_CHUNK_BYTES];
    let mut copied = 0_u64;

    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        copied = copied.saturating_add(read as u64);
        progress.add_processed(read as u64);
    }
    output.flush()?;
    Ok(copied)
}

fn rollback_moved_raw_entries(moved_entries: &[MovedFolderEntry]) {
    for entry in moved_entries.iter().rev() {
        if !entry.rollback_required {
            continue;
        }
        if entry.source_path.exists() || !entry.destination_path.exists() {
            continue;
        }
        let _ = std::fs::rename(&entry.destination_path, &entry.source_path);
    }
}

fn is_path_inside_root(path: &Path, root: Option<&Path>) -> bool {
    root.is_some_and(|root| path.starts_with(root))
}

fn source_bytes_inside_root(entries: &[BulkArchiveEntry], root: &Path) -> Result<u64, String> {
    entries.iter().try_fold(0_u64, |total, entry| {
        if entry.source_path.starts_with(root) {
            file_len_for_bulk_entry(&entry.source_path).map(|bytes| total.saturating_add(bytes))
        } else {
            Ok(total)
        }
    })
}

fn remove_incomplete_output(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
            .map_err(|error| format!("Could not remove incomplete bulk output folder: {error}"))
    } else {
        std::fs::remove_file(path)
            .map_err(|error| format!("Could not remove incomplete archive file: {error}"))
    }
}

fn cleanup_original_archive_parts(
    paths: &[PathBuf],
    already_moved_paths: &[PathBuf],
) -> Vec<String> {
    paths
        .iter()
        .filter(|path| !already_moved_paths.iter().any(|moved| moved == *path))
        .filter_map(|path| match std::fs::remove_file(path) {
            Ok(()) => None,
            Err(error) => Some(format!(
                "Could not delete downloaded archive part {} after bulk finalization: {error}",
                path.display()
            )),
        })
        .collect()
}

fn archive_staging_root(output_path: &Path) -> Result<PathBuf, String> {
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = output_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("bulk-download.zip");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("Could not create archive staging name: {error}"))?
        .as_nanos();
    Ok(parent.join(format!(
        ".{file_name}.extracting-{}-{timestamp}",
        std::process::id()
    )))
}

pub(super) fn retry_bulk_file_operation<T>(
    context: &str,
    mut operation: impl FnMut() -> Result<T, std::io::Error>,
) -> Result<T, String> {
    let mut last_error = None;
    for attempt in 0..BULK_FILE_OPERATION_RETRY_ATTEMPTS {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error)
                if is_retryable_file_operation_error(&error)
                    && attempt + 1 < BULK_FILE_OPERATION_RETRY_ATTEMPTS =>
            {
                last_error = Some(error);
                std::thread::sleep(BULK_FILE_OPERATION_RETRY_DELAY);
            }
            Err(error) => return Err(format!("{context}: {error}")),
        }
    }

    Err(format!(
        "{context}: {}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "operation did not complete".into())
    ))
}

fn is_retryable_file_operation_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::PermissionDenied
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::WouldBlock
    ) || is_archive_file_lock_error(&error.to_string())
}

fn cleanup_stale_archive_staging_roots(
    output_path: &Path,
    keep_staging_root: Option<&Path>,
) -> Result<(), String> {
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = output_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("bulk-download.zip");
    let prefix = format!(".{file_name}.extracting-");
    let children = match std::fs::read_dir(parent) {
        Ok(children) => children,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Could not inspect bulk staging folders: {error}")),
    };

    for child in children {
        let child =
            child.map_err(|error| format!("Could not inspect bulk staging folder: {error}"))?;
        let path = child.path();
        if keep_staging_root.is_some_and(|keep| keep == path) {
            continue;
        }
        if path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with(&prefix))
            && path.is_dir()
        {
            retry_bulk_file_operation("Could not remove stale bulk staging folder", || {
                std::fs::remove_dir_all(&path)
            })?;
        }
    }

    Ok(())
}

fn verify_finished_output(output_path: &Path) -> Result<(), String> {
    if output_path.is_dir() {
        Ok(())
    } else {
        Err(format!(
            "Could not verify finalized bulk output folder: {}",
            output_path.display()
        ))
    }
}

fn archive_part_display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

pub(super) fn validate_zip_entry_name(name: &str) -> Result<String, String> {
    let normalized = name.trim().replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!("Archive entry name is not supported: {name}"));
    }

    Ok(normalized)
}
