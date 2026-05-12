use crate::storage::DiagnosticEvent;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use super::current_unix_timestamp_millis;

const DIAGNOSTIC_EVENT_LOG_FILENAME: &str = "diagnostic-events.jsonl";
const DEFAULT_DIAGNOSTIC_EVENT_HISTORY_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const DEFAULT_DIAGNOSTIC_EVENT_HISTORY_MAX_BYTES: u64 = 10 * 1024 * 1024;
const DEFAULT_DIAGNOSTIC_EVENT_COMPACT_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Debug)]
pub(super) struct DiagnosticEventStore {
    path: PathBuf,
    max_age: Duration,
    max_bytes: u64,
    compact_interval: Duration,
    state: Mutex<DiagnosticEventStoreState>,
}

#[derive(Debug, Default)]
struct DiagnosticEventStoreState {
    last_compacted_at_millis: u64,
}

impl DiagnosticEventStore {
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            path,
            max_age: DEFAULT_DIAGNOSTIC_EVENT_HISTORY_MAX_AGE,
            max_bytes: DEFAULT_DIAGNOSTIC_EVENT_HISTORY_MAX_BYTES,
            compact_interval: DEFAULT_DIAGNOSTIC_EVENT_COMPACT_INTERVAL,
            state: Mutex::new(DiagnosticEventStoreState::default()),
        }
    }

    #[cfg(test)]
    pub(super) fn new_with_limits(path: PathBuf, max_age: Duration, max_bytes: u64) -> Self {
        Self {
            path,
            max_age,
            max_bytes,
            compact_interval: Duration::from_millis(0),
            state: Mutex::new(DiagnosticEventStoreState::default()),
        }
    }

    pub(super) fn append(&self, event: &DiagnosticEvent) -> Result<(), String> {
        let line = serialize_event_line(event)?;
        let mut state = self
            .state
            .lock()
            .map_err(|error| format!("Could not lock diagnostic event store: {error}"))?;

        ensure_parent_directory(&self.path)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| format!("Could not open diagnostic event log: {error}"))?;
        file.write_all(line.as_bytes())
            .and_then(|_| file.write_all(b"\n"))
            .map_err(|error| format!("Could not append diagnostic event: {error}"))?;

        let compact_due = event
            .timestamp
            .saturating_sub(state.last_compacted_at_millis)
            >= duration_millis(self.compact_interval);
        let oversized = fs::metadata(&self.path)
            .map(|metadata| metadata.len() > self.max_bytes)
            .unwrap_or(false);

        if compact_due || oversized {
            let now = current_unix_timestamp_millis();
            self.compact_locked(now)?;
            state.last_compacted_at_millis = now;
        }

        Ok(())
    }

    pub(super) fn migrate_legacy_events(&self, events: Vec<DiagnosticEvent>) -> Result<(), String> {
        if events.is_empty() {
            return Ok(());
        }

        let mut state = self
            .state
            .lock()
            .map_err(|error| format!("Could not lock diagnostic event store: {error}"))?;
        ensure_parent_directory(&self.path)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| format!("Could not open diagnostic event log: {error}"))?;

        for event in events {
            let line = serialize_event_line(&event)?;
            file.write_all(line.as_bytes())
                .and_then(|_| file.write_all(b"\n"))
                .map_err(|error| format!("Could not migrate diagnostic event: {error}"))?;
        }

        let now = current_unix_timestamp_millis();
        self.compact_locked(now)?;
        state.last_compacted_at_millis = now;
        Ok(())
    }

    pub(super) fn retained_events(&self) -> Result<Vec<DiagnosticEvent>, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|error| format!("Could not lock diagnostic event store: {error}"))?;
        let now = current_unix_timestamp_millis();
        let events = self.compact_locked(now)?;
        state.last_compacted_at_millis = now;
        Ok(events)
    }

    pub(super) fn recent_events(&self, limit: usize) -> Result<Vec<DiagnosticEvent>, String> {
        let mut events = self.retained_events()?;
        if events.len() > limit {
            let overflow = events.len() - limit;
            events.drain(0..overflow);
        }
        Ok(events)
    }

    fn compact_locked(&self, now: u64) -> Result<Vec<DiagnosticEvent>, String> {
        let file_existed = self.path.exists();
        let events = self.load_events_locked()?;
        let events = self.apply_retention(now, events);

        if file_existed || !events.is_empty() {
            self.write_events_locked(&events)?;
        }

        Ok(events)
    }

    fn load_events_locked(&self) -> Result<Vec<DiagnosticEvent>, String> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.path)
            .map_err(|error| format!("Could not read diagnostic event log: {error}"))?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for line in reader.lines() {
            let Ok(line) = line else {
                continue;
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<DiagnosticEvent>(line) {
                events.push(event);
            }
        }

        Ok(events)
    }

    fn apply_retention(&self, now: u64, events: Vec<DiagnosticEvent>) -> Vec<DiagnosticEvent> {
        let cutoff = now.saturating_sub(duration_millis(self.max_age));
        let retained_by_age = events
            .into_iter()
            .filter(|event| event.timestamp >= cutoff)
            .collect::<Vec<_>>();

        if self.max_bytes == 0 {
            return Vec::new();
        }

        let mut retained_reversed = Vec::new();
        let mut retained_bytes = 0_u64;

        for event in retained_by_age.into_iter().rev() {
            let Ok(line) = serialize_event_line(&event) else {
                continue;
            };
            let event_bytes = line.len() as u64 + 1;
            if event_bytes > self.max_bytes || retained_bytes + event_bytes > self.max_bytes {
                continue;
            }

            retained_bytes += event_bytes;
            retained_reversed.push(event);
        }

        retained_reversed.reverse();
        retained_reversed
    }

    fn write_events_locked(&self, events: &[DiagnosticEvent]) -> Result<(), String> {
        ensure_parent_directory(&self.path)?;
        let temp_path = diagnostic_event_temp_path(&self.path);
        let mut file = File::create(&temp_path)
            .map_err(|error| format!("Could not write diagnostic event log: {error}"))?;

        for event in events {
            let line = serialize_event_line(event)?;
            file.write_all(line.as_bytes())
                .and_then(|_| file.write_all(b"\n"))
                .map_err(|error| format!("Could not write diagnostic event: {error}"))?;
        }

        drop(file);
        let backup_path = diagnostic_event_backup_path(&self.path);
        remove_file_if_exists(&backup_path, "Could not clear diagnostic event log backup")?;

        if self.path.exists() {
            fs::rename(&self.path, &backup_path)
                .map_err(|error| format!("Could not back up diagnostic event log: {error}"))?;
        }

        if let Err(error) = fs::rename(&temp_path, &self.path) {
            if !self.path.exists() && backup_path.exists() {
                let _ = fs::rename(&backup_path, &self.path);
            }

            return Err(format!("Could not finalize diagnostic event log: {error}"));
        }

        remove_file_if_exists(&backup_path, "Could not remove diagnostic event log backup")?;

        Ok(())
    }
}

pub(super) fn diagnostic_event_log_path_for(storage_path: &Path) -> PathBuf {
    storage_path
        .parent()
        .map(|parent| parent.join(DIAGNOSTIC_EVENT_LOG_FILENAME))
        .unwrap_or_else(|| PathBuf::from(DIAGNOSTIC_EVENT_LOG_FILENAME))
}

fn serialize_event_line(event: &DiagnosticEvent) -> Result<String, String> {
    serde_json::to_string(event)
        .map_err(|error| format!("Could not serialize diagnostic event: {error}"))
}

fn ensure_parent_directory(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create diagnostic event log directory: {error}"))?;
    }
    Ok(())
}

fn diagnostic_event_temp_path(path: &Path) -> PathBuf {
    diagnostic_event_path_with_extra_extension(path, "tmp")
}

fn diagnostic_event_backup_path(path: &Path) -> PathBuf {
    diagnostic_event_path_with_extra_extension(path, "bak")
}

fn diagnostic_event_path_with_extra_extension(path: &Path, fallback: &str) -> PathBuf {
    let mut extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();

    if extension.is_empty() {
        extension = fallback.into();
    } else {
        extension.push('.');
        extension.push_str(fallback);
    }

    path.with_extension(extension)
}

fn remove_file_if_exists(path: &Path, context: &str) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{context}: {error}")),
    }
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}
