use super::*;
use crate::state::BulkArchiveEntry;
use crate::storage::BulkArchiveOutputKind;
use std::collections::{BTreeSet, HashMap};
use std::process::{Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const ARCHIVE_EXTRACT_LOCK_RETRY_ATTEMPTS: usize = 8;
const BULK_FILE_OPERATION_RETRY_ATTEMPTS: usize = 8;
pub(super) const HUGE_BULK_ARCHIVE_THRESHOLD_BYTES: u64 = 100 * 1024 * 1024 * 1024;

#[cfg(test)]
const ARCHIVE_EXTRACT_LOCK_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(0);
#[cfg(not(test))]
const ARCHIVE_EXTRACT_LOCK_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(500);
#[cfg(test)]
const BULK_FILE_OPERATION_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(0);
#[cfg(not(test))]
const BULK_FILE_OPERATION_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

#[cfg(test)]
pub(super) fn create_bulk_archive_sync(archive: BulkArchiveReady) -> Result<PathBuf, String> {
    let prepared = prepare_bulk_archive_sources_without_extraction(archive)?;
    finish_prepared_bulk_archive_sync(prepared).map(|outcome| outcome.output_path)
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
    let entries = prepare_entries_for_output(plan.raw_entries, &output_path, archive.output_kind)?;
    let cleanup_paths = cleanup_paths_for_output(&entries, archive.output_kind);

    Ok(PreparedBulkArchive {
        output_kind: archive.output_kind,
        output_path,
        entries,
        cleanup_paths,
        staging_root: None,
    })
}

pub(super) fn prepare_bulk_archive_sources_with_7zip(
    archive: BulkArchiveReady,
    seven_zip_path: PathBuf,
) -> Result<PreparedBulkArchive, String> {
    let extractor = SevenZipArchiveExtractor {
        executable: seven_zip_path,
    };
    prepare_bulk_archive_sources_with_extractor(archive, &extractor)
}

pub(super) fn prepare_bulk_archive_sources_with_extractor(
    archive: BulkArchiveReady,
    extractor: &impl ArchiveExtractor,
) -> Result<PreparedBulkArchive, String> {
    let plan = build_bulk_archive_source_plan(&archive.entries)?;
    if plan.archive_sets.is_empty() {
        let output_path = archive.output_path;
        let entries =
            prepare_entries_for_output(plan.raw_entries, &output_path, archive.output_kind)?;
        let cleanup_paths = cleanup_paths_for_output(&entries, archive.output_kind);
        return Ok(PreparedBulkArchive {
            output_kind: archive.output_kind,
            output_path,
            entries,
            cleanup_paths,
            staging_root: None,
        });
    }

    let output_kind = archive.output_kind;
    let output_path = archive.output_path;
    let staging_root = archive_staging_root(&output_path)?;
    let extract_dir = staging_root.join("extracted");
    std::fs::create_dir_all(&extract_dir)
        .map_err(|error| format!("Could not create archive extraction directory: {error}"))?;

    let result: Result<PreparedBulkArchive, String> = (|| {
        let mut entries = Vec::new();
        for (index, archive_set) in plan.archive_sets.iter().enumerate() {
            let set_extract_dir = extract_dir.join(format!("set-{index}"));
            std::fs::create_dir_all(&set_extract_dir).map_err(|error| {
                format!("Could not create archive extraction directory: {error}")
            })?;
            extract_with_lock_retry(
                extractor,
                &archive_set.first_part.source_path,
                &set_extract_dir,
            )?;

            let mut set_entries = collect_zip_entries_from_directory(&set_extract_dir)?;
            if set_entries.is_empty() {
                return Err(format!(
                    "Archive extraction for {} did not produce any files.",
                    archive_part_display_name(&archive_set.first_part.source_path)
                ));
            }
            entries.append(&mut set_entries);
        }

        let raw_cleanup_paths = cleanup_paths_for_output(&plan.raw_entries, output_kind);
        entries.extend(plan.raw_entries);
        if entries.is_empty() {
            return Err("Archive extraction did not produce any files to compress.".into());
        }
        let entries = prepare_entries_for_output(entries, &output_path, output_kind)?;

        Ok(PreparedBulkArchive {
            output_kind,
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
                .chain(raw_cleanup_paths)
                .collect(),
            staging_root: Some(staging_root.clone()),
        })
    })();

    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging_root);
    }

    result
}

pub(super) fn finish_prepared_bulk_archive_sync(
    prepared: PreparedBulkArchive,
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

    let temp_path = temporary_output_path(&prepared.output_path, prepared.output_kind)?;
    let result: Result<PathBuf, String> = match prepared.output_kind {
        BulkArchiveOutputKind::Archive => finish_prepared_zip_output(&prepared, &temp_path),
        BulkArchiveOutputKind::Folder => finish_prepared_folder_output(&prepared, &temp_path),
    };

    if result.is_err() {
        let _ = remove_incomplete_output(&temp_path);
        if let Some(staging_root) = &prepared.staging_root {
            let _ = std::fs::remove_dir_all(staging_root);
        }
    }

    let output_path = result?;
    verify_finished_output(&output_path, prepared.output_kind)?;
    let cleanup_warnings = cleanup_original_archive_parts(&prepared.cleanup_paths);
    if let Some(staging_root) = &prepared.staging_root {
        let _ = std::fs::remove_dir_all(staging_root);
    }

    Ok(BulkArchiveCreateOutcome {
        output_path,
        cleanup_warnings,
    })
}

#[derive(Debug)]
pub(super) struct BulkArchiveCreateOutcome {
    pub(super) output_path: PathBuf,
    pub(super) cleanup_warnings: Vec<String>,
}

#[derive(Debug)]
pub(super) struct PreparedBulkArchive {
    pub(super) output_kind: BulkArchiveOutputKind,
    pub(super) output_path: PathBuf,
    pub(super) entries: Vec<BulkArchiveEntry>,
    pub(super) cleanup_paths: Vec<PathBuf>,
    pub(super) staging_root: Option<PathBuf>,
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

pub(super) trait ArchiveExtractor {
    fn extract(&self, first_part: &Path, output_dir: &Path) -> Result<(), String>;
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

#[derive(Debug, Clone, Copy)]
enum ArchivePartSuffix {
    PartRar,
    Numbered,
    LegacyRar,
}

struct DetectedArchivePart {
    key: String,
    display_prefix: String,
    suffix: ArchivePartSuffix,
    part_number: u32,
    number_width: usize,
}

fn detect_archive_part(name: &str) -> Option<DetectedArchivePart> {
    let normalized = name.replace('\\', "/");
    let file_name = normalized.rsplit('/').next()?.trim();
    if file_name.is_empty() {
        return None;
    }
    let lower = file_name.to_ascii_lowercase();

    if lower.ends_with(".rar") {
        let without_rar = &file_name[..file_name.len().saturating_sub(4)];
        let lower_without_rar = &lower[..lower.len().saturating_sub(4)];
        if let Some(part_index) = lower_without_rar.rfind(".part") {
            let number_text = &lower_without_rar[part_index + 5..];
            if !number_text.is_empty() && number_text.chars().all(|value| value.is_ascii_digit()) {
                let Ok(part_number) = number_text.parse::<u32>() else {
                    return None;
                };
                let display_prefix = file_name[..part_index].to_string();
                return Some(DetectedArchivePart {
                    key: format!("part-rar:{}", display_prefix.to_ascii_lowercase()),
                    display_prefix,
                    suffix: ArchivePartSuffix::PartRar,
                    part_number,
                    number_width: number_text.len(),
                });
            }
        }

        return Some(DetectedArchivePart {
            key: format!("legacy-rar:{}", without_rar.to_ascii_lowercase()),
            display_prefix: without_rar.to_string(),
            suffix: ArchivePartSuffix::LegacyRar,
            part_number: 1,
            number_width: 1,
        });
    }

    let dot_index = file_name.rfind('.')?;
    let extension = &file_name[dot_index + 1..];
    let lower_extension = extension.to_ascii_lowercase();
    if lower_extension.len() == 3
        && lower_extension.starts_with('r')
        && lower_extension[1..]
            .chars()
            .all(|value| value.is_ascii_digit())
    {
        let Ok(part_index) = lower_extension[1..].parse::<u32>() else {
            return None;
        };
        let display_prefix = file_name[..dot_index].to_string();
        return Some(DetectedArchivePart {
            key: format!("legacy-rar:{}", display_prefix.to_ascii_lowercase()),
            display_prefix,
            suffix: ArchivePartSuffix::LegacyRar,
            part_number: part_index.saturating_add(2),
            number_width: 2,
        });
    }

    if extension.len() == 3 && extension.chars().all(|value| value.is_ascii_digit()) {
        let Ok(part_number) = extension.parse::<u32>() else {
            return None;
        };
        if part_number == 0 {
            return None;
        }
        let display_prefix = file_name[..dot_index].to_string();
        return Some(DetectedArchivePart {
            key: format!("numbered:{}", display_prefix.to_ascii_lowercase()),
            display_prefix,
            suffix: ArchivePartSuffix::Numbered,
            part_number,
            number_width: extension.len(),
        });
    }

    None
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
    output_path: &Path,
    output_kind: BulkArchiveOutputKind,
) -> Result<Vec<BulkArchiveEntry>, String> {
    let entries = match output_kind {
        BulkArchiveOutputKind::Archive => wrap_entries_in_archive_folder(entries, output_path),
        BulkArchiveOutputKind::Folder => entries
            .into_iter()
            .map(|mut entry| {
                entry.archive_name = validate_zip_entry_name(&entry.archive_name)?;
                Ok(entry)
            })
            .collect(),
    }?;
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

fn cleanup_paths_for_output(
    entries: &[BulkArchiveEntry],
    output_kind: BulkArchiveOutputKind,
) -> Vec<PathBuf> {
    match output_kind {
        BulkArchiveOutputKind::Archive => Vec::new(),
        BulkArchiveOutputKind::Folder => entries
            .iter()
            .map(|entry| entry.source_path.clone())
            .collect(),
    }
}

fn wrap_entries_in_archive_folder(
    entries: Vec<BulkArchiveEntry>,
    output_path: &Path,
) -> Result<Vec<BulkArchiveEntry>, String> {
    let folder_name = archive_folder_name_for_output_path(output_path)?;
    entries
        .into_iter()
        .map(|mut entry| {
            entry.archive_name =
                validate_zip_entry_name(&format!("{folder_name}/{}", entry.archive_name))?;
            Ok(entry)
        })
        .collect()
}

fn archive_folder_name_for_output_path(output_path: &Path) -> Result<String, String> {
    let stem = output_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("bulk-download");
    let sanitized = stem
        .chars()
        .filter(|character| {
            !character.is_control()
                && !matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();
    let folder_name = if sanitized.is_empty() {
        "bulk-download".to_string()
    } else {
        sanitized
    };
    validate_zip_entry_name(&folder_name)
}

fn huge_bulk_folder_warning(total_completed_bytes: u64) -> Option<String> {
    (total_completed_bytes >= HUGE_BULK_ARCHIVE_THRESHOLD_BYTES).then(|| {
        "Bulk output is 100 GiB or larger; finalization will use folder moves/extraction instead of ZIP creation."
            .into()
    })
}

fn same_volume_for_bulk_move(source: &Path, output_path: &Path) -> bool {
    source.components().next() == output_path.components().next()
}

fn temporary_output_path(
    output_path: &Path,
    output_kind: BulkArchiveOutputKind,
) -> Result<PathBuf, String> {
    match output_kind {
        BulkArchiveOutputKind::Archive => Ok(output_path.with_extension(
            output_path
                .extension()
                .and_then(|value| value.to_str())
                .map(|extension| format!("{extension}.tmp"))
                .unwrap_or_else(|| "tmp".into()),
        )),
        BulkArchiveOutputKind::Folder => archive_staging_root(output_path),
    }
}

fn finish_prepared_zip_output(
    prepared: &PreparedBulkArchive,
    temp_path: &Path,
) -> Result<PathBuf, String> {
    let mut file = retry_bulk_file_operation("Could not create archive file", || {
        std::fs::File::create(temp_path)
    })?;
    write_zip_archive(&mut file, &prepared.entries)?;
    retry_bulk_file_operation("Could not flush archive file", || file.sync_all())?;
    retry_bulk_file_operation("Could not finalize archive file", || {
        std::fs::rename(temp_path, &prepared.output_path)
    })?;
    Ok(prepared.output_path.clone())
}

fn finish_prepared_folder_output(
    prepared: &PreparedBulkArchive,
    temp_path: &Path,
) -> Result<PathBuf, String> {
    retry_bulk_file_operation("Could not create bulk output folder", || {
        std::fs::create_dir_all(temp_path)
    })?;
    copy_entries_to_folder_verified(&prepared.entries, temp_path)?;
    retry_bulk_file_operation("Could not finalize bulk output folder", || {
        std::fs::rename(temp_path, &prepared.output_path)
    })?;
    Ok(prepared.output_path.clone())
}

fn copy_entries_to_folder_verified(
    entries: &[BulkArchiveEntry],
    output_root: &Path,
) -> Result<(), String> {
    for entry in entries {
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

        copy_file_verified(&entry.source_path, &destination)?;
    }
    Ok(())
}

fn copy_file_verified(source: &Path, destination: &Path) -> Result<(), String> {
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

    let copied = retry_bulk_file_operation(
        &format!(
            "Could not copy {} into bulk output folder",
            source.display()
        ),
        || std::fs::copy(source, destination),
    )?;
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

    let source_crc = crc32_for_file(source)?;
    let destination_crc = crc32_for_file(destination)?;
    if source_crc != destination_crc {
        return Err(format!(
            "Could not verify copied bulk output file {}: checksum does not match source.",
            destination.display()
        ));
    }

    let file = retry_bulk_file_operation(
        &format!(
            "Could not reopen copied bulk output file {}",
            destination.display()
        ),
        || {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(destination)
        },
    )?;
    retry_bulk_file_operation(
        &format!(
            "Could not flush copied bulk output file {}",
            destination.display()
        ),
        || file.sync_all(),
    )
}

fn crc32_for_file(path: &Path) -> Result<u32, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("Could not open {} for checksum: {error}", path.display()))?;
    let mut crc = 0xffff_ffff;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Could not read {} for checksum: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        crc = update_crc32(crc, &buffer[..read]);
    }
    Ok(!crc)
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

fn cleanup_original_archive_parts(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
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

fn verify_finished_output(
    output_path: &Path,
    output_kind: BulkArchiveOutputKind,
) -> Result<(), String> {
    match output_kind {
        BulkArchiveOutputKind::Archive if output_path.is_file() => Ok(()),
        BulkArchiveOutputKind::Folder if output_path.is_dir() => Ok(()),
        BulkArchiveOutputKind::Archive => Err(format!(
            "Could not verify finalized archive file: {}",
            output_path.display()
        )),
        BulkArchiveOutputKind::Folder => Err(format!(
            "Could not verify finalized bulk output folder: {}",
            output_path.display()
        )),
    }
}

fn archive_part_display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

#[derive(Debug)]
pub(super) struct ZipCentralDirectoryEntry {
    pub(super) name: String,
    pub(super) crc32: u32,
    pub(super) compressed_size: u64,
    pub(super) uncompressed_size: u64,
    pub(super) local_header_offset: u64,
}

pub(super) fn write_zip_archive(
    writer: &mut (impl Write + Seek),
    entries: &[crate::state::BulkArchiveEntry],
) -> Result<(), String> {
    let mut central_entries = Vec::with_capacity(entries.len());

    for entry in entries {
        let name = validate_zip_entry_name(&entry.archive_name)?;
        let metadata = std::fs::metadata(&entry.source_path).map_err(|error| {
            format!(
                "Could not read {} for archiving: {error}",
                entry.source_path.display()
            )
        })?;
        if !metadata.is_file() {
            return Err(format!(
                "Could not archive {} because it is not a file.",
                entry.source_path.display()
            ));
        }
        let size = metadata.len();
        let local_header_offset = zip_stream_position(writer)?;

        write_zip_local_header(writer, &name, size)?;
        let crc32 = write_zip_file_data(writer, &entry.source_path)?;
        write_zip_data_descriptor(writer, crc32, size)?;

        central_entries.push(ZipCentralDirectoryEntry {
            name,
            crc32,
            compressed_size: size,
            uncompressed_size: size,
            local_header_offset,
        });
    }

    let central_directory_offset = zip_stream_position(writer)?;
    for entry in &central_entries {
        write_zip_central_directory_entry(writer, entry)?;
    }
    let central_directory_size =
        zip_stream_position(writer)?.saturating_sub(central_directory_offset);
    write_zip_end_of_central_directory(
        writer,
        central_entries.len(),
        central_directory_size,
        central_directory_offset,
    )
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

pub(super) fn write_zip_local_header(
    writer: &mut impl Write,
    name: &str,
    size: u64,
) -> Result<(), String> {
    let name_bytes = zip_entry_name_bytes(name)?;
    let needs_zip64 = size > ZIP32_MAX;
    let extra_len = if needs_zip64 {
        zip64_extra_field_len(2)?
    } else {
        0
    };
    write_u32_le(writer, 0x0403_4b50)?;
    write_u16_le(writer, zip_version_needed(needs_zip64))?;
    write_u16_le(writer, ZIP_GENERAL_PURPOSE_FLAGS)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, ZIP_DOS_DATE_1980_01_01)?;
    write_u32_le(writer, 0)?;
    write_u32_le(writer, if needs_zip64 { u32::MAX } else { 0 })?;
    write_u32_le(writer, if needs_zip64 { u32::MAX } else { 0 })?;
    write_u16_le(
        writer,
        checked_u16_len(name_bytes.len(), "archive entry name")?,
    )?;
    write_u16_le(writer, extra_len)?;
    writer
        .write_all(name_bytes)
        .map_err(|error| format!("Could not write ZIP header: {error}"))?;
    if needs_zip64 {
        write_zip64_extra_field(writer, &[size, size])?;
    }
    Ok(())
}

pub(super) fn write_zip_file_data(
    writer: &mut impl Write,
    source_path: &Path,
) -> Result<u32, String> {
    let mut source = std::fs::File::open(source_path).map_err(|error| {
        format!(
            "Could not open {} for archiving: {error}",
            source_path.display()
        )
    })?;
    let mut crc = 0xffff_ffff;
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = source.read(&mut buffer).map_err(|error| {
            format!(
                "Could not read {} for archiving: {error}",
                source_path.display()
            )
        })?;
        if read == 0 {
            break;
        }
        crc = update_crc32(crc, &buffer[..read]);
        writer
            .write_all(&buffer[..read])
            .map_err(|error| format!("Could not write ZIP data: {error}"))?;
    }

    Ok(!crc)
}

pub(super) fn write_zip_data_descriptor(
    writer: &mut impl Write,
    crc32: u32,
    size: u64,
) -> Result<(), String> {
    write_u32_le(writer, 0x0807_4b50)?;
    write_u32_le(writer, crc32)?;
    if size > ZIP32_MAX {
        write_u64_le(writer, size)?;
        write_u64_le(writer, size)
    } else {
        let size = size as u32;
        write_u32_le(writer, size)?;
        write_u32_le(writer, size)
    }
}

pub(super) fn write_zip_central_directory_entry(
    writer: &mut impl Write,
    entry: &ZipCentralDirectoryEntry,
) -> Result<(), String> {
    let name_bytes = zip_entry_name_bytes(&entry.name)?;
    let zip64_values = central_directory_zip64_values(entry);
    let needs_zip64 = !zip64_values.is_empty();
    write_u32_le(writer, 0x0201_4b50)?;
    write_u16_le(writer, zip_version_needed(needs_zip64))?;
    write_u16_le(writer, zip_version_needed(needs_zip64))?;
    write_u16_le(writer, ZIP_GENERAL_PURPOSE_FLAGS)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, ZIP_DOS_DATE_1980_01_01)?;
    write_u32_le(writer, entry.crc32)?;
    write_u32_le(writer, zip32_or_max(entry.compressed_size))?;
    write_u32_le(writer, zip32_or_max(entry.uncompressed_size))?;
    write_u16_le(
        writer,
        checked_u16_len(name_bytes.len(), "archive entry name")?,
    )?;
    write_u16_le(writer, zip64_extra_field_len(zip64_values.len())?)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, 0)?;
    write_u32_le(writer, 0)?;
    write_u32_le(writer, zip32_or_max(entry.local_header_offset))?;
    writer
        .write_all(name_bytes)
        .map_err(|error| format!("Could not write ZIP central directory: {error}"))?;
    if needs_zip64 {
        write_zip64_extra_field(writer, &zip64_values)?;
    }
    Ok(())
}

pub(super) fn write_zip_end_of_central_directory(
    writer: &mut (impl Write + Seek),
    entry_count: usize,
    central_directory_size: u64,
    central_directory_offset: u64,
) -> Result<(), String> {
    let needs_zip64 = entry_count > usize::from(u16::MAX)
        || central_directory_size > ZIP32_MAX
        || central_directory_offset > ZIP32_MAX;

    if needs_zip64 {
        write_zip64_end_of_central_directory(
            writer,
            entry_count,
            central_directory_size,
            central_directory_offset,
        )?;
    }

    write_u32_le(writer, 0x0605_4b50)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, 0)?;
    write_u16_le(
        writer,
        if entry_count > usize::from(u16::MAX) {
            u16::MAX
        } else {
            entry_count as u16
        },
    )?;
    write_u16_le(
        writer,
        if entry_count > usize::from(u16::MAX) {
            u16::MAX
        } else {
            entry_count as u16
        },
    )?;
    write_u32_le(writer, zip32_or_max(central_directory_size))?;
    write_u32_le(writer, zip32_or_max(central_directory_offset))?;
    write_u16_le(writer, 0)
}

fn write_zip64_end_of_central_directory(
    writer: &mut (impl Write + Seek),
    entry_count: usize,
    central_directory_size: u64,
    central_directory_offset: u64,
) -> Result<(), String> {
    let zip64_eocd_offset = zip_stream_position(writer)?;
    write_u32_le(writer, 0x0606_4b50)?;
    write_u64_le(writer, 44)?;
    write_u16_le(writer, ZIP64_VERSION_NEEDED)?;
    write_u16_le(writer, ZIP64_VERSION_NEEDED)?;
    write_u32_le(writer, 0)?;
    write_u32_le(writer, 0)?;
    write_u64_le(writer, entry_count as u64)?;
    write_u64_le(writer, entry_count as u64)?;
    write_u64_le(writer, central_directory_size)?;
    write_u64_le(writer, central_directory_offset)?;

    write_u32_le(writer, 0x0706_4b50)?;
    write_u32_le(writer, 0)?;
    write_u64_le(writer, zip64_eocd_offset)?;
    write_u32_le(writer, 1)
}

pub(super) fn zip_entry_name_bytes(name: &str) -> Result<&[u8], String> {
    let bytes = name.as_bytes();
    checked_u16_len(bytes.len(), "archive entry name")?;
    Ok(bytes)
}

pub(super) fn checked_u16_len(value: usize, label: &str) -> Result<u16, String> {
    u16::try_from(value).map_err(|_| format!("{label} exceeds the ZIP32 limit."))
}

pub(super) fn zip_stream_position(writer: &mut impl Seek) -> Result<u64, String> {
    writer
        .stream_position()
        .map_err(|error| format!("Could not read ZIP writer position: {error}"))
}

pub(super) fn write_u16_le(writer: &mut impl Write, value: u16) -> Result<(), String> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|error| format!("Could not write ZIP archive: {error}"))
}

pub(super) fn write_u32_le(writer: &mut impl Write, value: u32) -> Result<(), String> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|error| format!("Could not write ZIP archive: {error}"))
}

pub(super) fn write_u64_le(writer: &mut impl Write, value: u64) -> Result<(), String> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|error| format!("Could not write ZIP archive: {error}"))
}

fn write_zip64_extra_field(writer: &mut impl Write, values: &[u64]) -> Result<(), String> {
    write_u16_le(writer, 0x0001)?;
    write_u16_le(
        writer,
        checked_u16_len(values.len() * 8, "ZIP64 extra field")?,
    )?;
    for value in values {
        write_u64_le(writer, *value)?;
    }
    Ok(())
}

fn central_directory_zip64_values(entry: &ZipCentralDirectoryEntry) -> Vec<u64> {
    let mut values = Vec::with_capacity(3);
    if entry.uncompressed_size > ZIP32_MAX {
        values.push(entry.uncompressed_size);
    }
    if entry.compressed_size > ZIP32_MAX {
        values.push(entry.compressed_size);
    }
    if entry.local_header_offset > ZIP32_MAX {
        values.push(entry.local_header_offset);
    }
    values
}

fn zip64_extra_field_len(value_count: usize) -> Result<u16, String> {
    if value_count == 0 {
        return Ok(0);
    }
    checked_u16_len(4 + value_count * 8, "ZIP64 extra field")
}

fn zip32_or_max(value: u64) -> u32 {
    if value > ZIP32_MAX {
        u32::MAX
    } else {
        value as u32
    }
}

fn zip_version_needed(needs_zip64: bool) -> u16 {
    if needs_zip64 {
        ZIP64_VERSION_NEEDED
    } else {
        ZIP32_VERSION_NEEDED
    }
}

pub(super) fn update_crc32(mut crc: u32, bytes: &[u8]) -> u32 {
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    crc
}

pub(super) const ZIP_GENERAL_PURPOSE_FLAGS: u16 = 0x0808;
pub(super) const ZIP_DOS_DATE_1980_01_01: u16 = 33;
pub(super) const ZIP32_VERSION_NEEDED: u16 = 20;
pub(super) const ZIP64_VERSION_NEEDED: u16 = 45;
pub(super) const ZIP32_MAX: u64 = u32::MAX as u64;
