use super::*;

pub(super) fn create_bulk_archive_sync(archive: BulkArchiveReady) -> Result<PathBuf, String> {
    if archive.output_path.exists() {
        return Ok(archive.output_path);
    }

    if let Some(parent) = archive.output_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create archive output directory: {error}"))?;
    }

    let temp_path = archive.output_path.with_extension(
        archive
            .output_path
            .extension()
            .and_then(|value| value.to_str())
            .map(|extension| format!("{extension}.tmp"))
            .unwrap_or_else(|| "tmp".into()),
    );
    let result = (|| {
        let mut file = std::fs::File::create(&temp_path)
            .map_err(|error| format!("Could not create archive file: {error}"))?;
        write_zip_archive(&mut file, &archive.entries)?;
        file.sync_all()
            .map_err(|error| format!("Could not flush archive file: {error}"))?;
        std::fs::rename(&temp_path, &archive.output_path)
            .map_err(|error| format!("Could not finalize archive file: {error}"))?;
        Ok(archive.output_path.clone())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }

    result
}

#[derive(Debug)]
pub(super) struct ZipCentralDirectoryEntry {
    name: String,
    crc32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    local_header_offset: u32,
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
        let size = u32::try_from(metadata.len()).map_err(|_| {
            format!(
                "Could not archive {} because it exceeds the ZIP32 size limit.",
                entry.source_path.display()
            )
        })?;
        let local_header_offset = checked_u32_position(zip_stream_position(writer)?)?;

        write_zip_local_header(writer, &name)?;
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

    let central_directory_offset = checked_u32_position(zip_stream_position(writer)?)?;
    for entry in &central_entries {
        write_zip_central_directory_entry(writer, entry)?;
    }
    let central_directory_size = checked_u32_position(
        zip_stream_position(writer)?.saturating_sub(u64::from(central_directory_offset)),
    )?;
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

pub(super) fn write_zip_local_header(writer: &mut impl Write, name: &str) -> Result<(), String> {
    let name_bytes = zip_entry_name_bytes(name)?;
    write_u32_le(writer, 0x0403_4b50)?;
    write_u16_le(writer, 20)?;
    write_u16_le(writer, ZIP_GENERAL_PURPOSE_FLAGS)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, ZIP_DOS_DATE_1980_01_01)?;
    write_u32_le(writer, 0)?;
    write_u32_le(writer, 0)?;
    write_u32_le(writer, 0)?;
    write_u16_le(
        writer,
        checked_u16_len(name_bytes.len(), "archive entry name")?,
    )?;
    write_u16_le(writer, 0)?;
    writer
        .write_all(name_bytes)
        .map_err(|error| format!("Could not write ZIP header: {error}"))
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
    size: u32,
) -> Result<(), String> {
    write_u32_le(writer, 0x0807_4b50)?;
    write_u32_le(writer, crc32)?;
    write_u32_le(writer, size)?;
    write_u32_le(writer, size)
}

pub(super) fn write_zip_central_directory_entry(
    writer: &mut impl Write,
    entry: &ZipCentralDirectoryEntry,
) -> Result<(), String> {
    let name_bytes = zip_entry_name_bytes(&entry.name)?;
    write_u32_le(writer, 0x0201_4b50)?;
    write_u16_le(writer, 20)?;
    write_u16_le(writer, 20)?;
    write_u16_le(writer, ZIP_GENERAL_PURPOSE_FLAGS)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, ZIP_DOS_DATE_1980_01_01)?;
    write_u32_le(writer, entry.crc32)?;
    write_u32_le(writer, entry.compressed_size)?;
    write_u32_le(writer, entry.uncompressed_size)?;
    write_u16_le(
        writer,
        checked_u16_len(name_bytes.len(), "archive entry name")?,
    )?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, 0)?;
    write_u32_le(writer, 0)?;
    write_u32_le(writer, entry.local_header_offset)?;
    writer
        .write_all(name_bytes)
        .map_err(|error| format!("Could not write ZIP central directory: {error}"))
}

pub(super) fn write_zip_end_of_central_directory(
    writer: &mut impl Write,
    entry_count: usize,
    central_directory_size: u32,
    central_directory_offset: u32,
) -> Result<(), String> {
    let entry_count = checked_u16_len(entry_count, "archive entry count")?;
    write_u32_le(writer, 0x0605_4b50)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, 0)?;
    write_u16_le(writer, entry_count)?;
    write_u16_le(writer, entry_count)?;
    write_u32_le(writer, central_directory_size)?;
    write_u32_le(writer, central_directory_offset)?;
    write_u16_le(writer, 0)
}

pub(super) fn zip_entry_name_bytes(name: &str) -> Result<&[u8], String> {
    let bytes = name.as_bytes();
    checked_u16_len(bytes.len(), "archive entry name")?;
    Ok(bytes)
}

pub(super) fn checked_u16_len(value: usize, label: &str) -> Result<u16, String> {
    u16::try_from(value).map_err(|_| format!("{label} exceeds the ZIP32 limit."))
}

pub(super) fn checked_u32_position(position: u64) -> Result<u32, String> {
    u32::try_from(position).map_err(|_| "Archive exceeds the ZIP32 size limit.".into())
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
