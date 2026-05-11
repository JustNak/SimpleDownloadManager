#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchivePartSuffix {
    PartRar,
    Numbered,
    LegacyRar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetectedArchivePart {
    pub(crate) key: String,
    pub(crate) display_prefix: String,
    pub(crate) suffix: ArchivePartSuffix,
    pub(crate) part_number: u32,
    pub(crate) number_width: usize,
}

pub(crate) fn detect_archive_part(name: &str) -> Option<DetectedArchivePart> {
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
