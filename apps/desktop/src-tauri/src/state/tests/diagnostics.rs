use super::*;

fn myapp_data_dir_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

struct MyappDataDirGuard {
    previous: Option<std::ffi::OsString>,
}

impl MyappDataDirGuard {
    fn set(path: &Path) -> Self {
        let previous = std::env::var_os("MYAPP_DATA_DIR");
        std::env::set_var("MYAPP_DATA_DIR", path);
        Self { previous }
    }
}

impl Drop for MyappDataDirGuard {
    fn drop(&mut self) {
        if let Some(value) = self.previous.take() {
            std::env::set_var("MYAPP_DATA_DIR", value);
        } else {
            std::env::remove_var("MYAPP_DATA_DIR");
        }
    }
}

#[tokio::test]
async fn diagnostics_keep_newest_five_hundred_events() {
    let download_dir = test_runtime_dir("diagnostic-events");
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);

    for index in 0..600 {
        state
            .record_diagnostic_event(
                DiagnosticLevel::Info,
                "test",
                format!("event {index}"),
                None,
            )
            .await
            .unwrap();
    }

    let host_registration = HostRegistrationDiagnostics {
        status: HostRegistrationStatus::Configured,
        entries: Vec::new(),
    };
    let snapshot = state.diagnostics_snapshot(host_registration.clone()).await;

    assert_eq!(snapshot.recent_events.len(), 500);
    assert_eq!(snapshot.recent_events[0].message, "event 100");
    assert_eq!(snapshot.recent_events[499].message, "event 599");

    let history = state
        .diagnostic_event_history()
        .await
        .expect("diagnostic event history should load");
    assert_eq!(history.len(), 600);

    let export = state.diagnostics_export(host_registration).await;
    assert_eq!(export.snapshot.recent_events.len(), 500);
    assert_eq!(export.event_history.len(), 600);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn startup_recovers_from_corrupt_state_json_with_diagnostic() {
    let _lock = myapp_data_dir_test_lock().lock().unwrap();
    let data_dir = test_runtime_dir("startup-corrupt-state");
    let state_path = data_dir.join("state.json");
    std::fs::write(&state_path, "{not-json").unwrap();
    let _env = MyappDataDirGuard::set(&data_dir);

    let state = SharedState::new().expect("corrupt persisted state should not abort startup");
    let runtime = state.inner.blocking_read();

    assert!(
        !state_path.exists(),
        "startup should not overwrite a corrupt state file with defaults"
    );
    assert!(
        runtime.diagnostic_events.iter().any(|event| {
            event.category == "startup" && event.message.contains("Could not parse persisted state")
        }),
        "startup diagnostics should include the persisted-state parse failure"
    );
    assert_eq!(
        runtime
            .snapshot()
            .startup_recovery
            .as_ref()
            .map(|recovery| recovery.status),
        Some(StartupRecoveryStatus::NeedsLocalRecovery)
    );
    drop(runtime);
    let quarantined = std::fs::read_dir(&data_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("state.invalid-") && name.ends_with(".json"))
        })
        .expect("corrupt state should be preserved as a quarantine file");
    assert_eq!(std::fs::read_to_string(quarantined).unwrap(), "{not-json");

    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn startup_restores_zero_filled_state_from_last_good_snapshot() {
    let _lock = myapp_data_dir_test_lock().lock().unwrap();
    let data_dir = test_runtime_dir("startup-zero-filled-last-good");
    let state_path = data_dir.join("state.json");
    std::fs::write(&state_path, vec![0; 256]).unwrap();
    let mut persisted = PersistedState::default();
    persisted.jobs = vec![download_job(
        "job_recovered",
        JobState::Completed,
        ResumeSupport::Supported,
        100,
    )];
    std::fs::write(
        data_dir.join("state.last-good.json"),
        serde_json::to_string_pretty(&persisted).unwrap(),
    )
    .unwrap();
    let _env = MyappDataDirGuard::set(&data_dir);

    let state = SharedState::new().expect("last-good state should recover startup");
    let snapshot = state.inner.blocking_read().snapshot();

    assert_eq!(snapshot.jobs.len(), 1);
    assert_eq!(snapshot.jobs[0].id, "job_recovered");
    let recovery = snapshot
        .startup_recovery
        .expect("startup recovery should be visible in the app snapshot");
    assert_eq!(recovery.status, StartupRecoveryStatus::Recovered);
    assert!(recovery.quarantined_path.is_some());

    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn startup_warns_for_unrecoverable_zero_filled_state_without_overwriting_it() {
    let _lock = myapp_data_dir_test_lock().lock().unwrap();
    let data_dir = test_runtime_dir("startup-zero-filled-needs-local-recovery");
    let state_path = data_dir.join("state.json");
    std::fs::write(&state_path, vec![0; 64]).unwrap();
    let _env = MyappDataDirGuard::set(&data_dir);

    let state = SharedState::new().expect("unrecoverable state should not abort startup");
    let snapshot = state.inner.blocking_read().snapshot();

    assert!(snapshot.jobs.is_empty());
    let recovery = snapshot
        .startup_recovery
        .expect("startup recovery warning should be visible in the app snapshot");
    assert_eq!(recovery.status, StartupRecoveryStatus::NeedsLocalRecovery);
    assert!(
        !state_path.exists(),
        "unrecoverable state.json should stay quarantined instead of being overwritten by defaults"
    );

    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn startup_keeps_unavailable_saved_directories_nonfatal_with_diagnostic() {
    let _lock = myapp_data_dir_test_lock().lock().unwrap();
    let data_dir = test_runtime_dir("startup-unavailable-directories");
    let blocking_file = data_dir.join("blocked-parent");
    std::fs::write(&blocking_file, "not a directory").unwrap();
    let download_dir = blocking_file.join("Downloads");
    let torrent_dir = blocking_file.join("Torrent");
    let bulk_dir = blocking_file.join("Bulk");
    let mut persisted = PersistedState::default();
    persisted.settings.download_directory = download_dir.display().to_string();
    persisted.settings.torrent.download_directory = torrent_dir.display().to_string();
    persisted.settings.bulk.output_directory = bulk_dir.display().to_string();
    persist_state(&data_dir.join("state.json"), &persisted).unwrap();
    let _env = MyappDataDirGuard::set(&data_dir);

    let state = SharedState::new().expect("unavailable saved directories should not abort startup");
    let settings = state.settings_sync();
    let runtime = state.inner.blocking_read();

    assert_eq!(PathBuf::from(settings.download_directory), download_dir);
    assert_eq!(
        PathBuf::from(settings.torrent.download_directory),
        torrent_dir
    );
    assert_eq!(PathBuf::from(settings.bulk.output_directory), bulk_dir);
    assert!(
        runtime.diagnostic_events.iter().any(|event| {
            event.category == "startup" && event.message.contains("saved download directory")
        }),
        "startup diagnostics should explain the unavailable saved download directory"
    );

    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn diagnostic_event_store_migrates_legacy_state_events() {
    let download_dir = test_runtime_dir("diagnostic-events-migrate");
    let store = DiagnosticEventStore::new(download_dir.join("diagnostic-events.jsonl"));
    let legacy_event = diagnostic_test_event("legacy event", current_unix_timestamp_millis());

    store
        .migrate_legacy_events(vec![legacy_event.clone()])
        .expect("legacy diagnostics should migrate");

    let history = store
        .retained_events()
        .expect("migrated diagnostic history should load");
    assert_eq!(history, vec![legacy_event]);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn diagnostic_event_store_skips_malformed_jsonl_records() {
    let download_dir = test_runtime_dir("diagnostic-events-malformed");
    let log_path = download_dir.join("diagnostic-events.jsonl");
    let good_event = diagnostic_test_event("valid event", current_unix_timestamp_millis());
    let serialized = serde_json::to_string(&good_event).unwrap();
    std::fs::write(
        &log_path,
        format!("{serialized}\n{{not-json}}\n{{\"timestamp\":"),
    )
    .unwrap();
    let store = DiagnosticEventStore::new(log_path);

    let history = store
        .retained_events()
        .expect("malformed diagnostic records should not abort loading");

    assert_eq!(history, vec![good_event]);
    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn diagnostic_event_store_compacts_old_events_and_byte_budget() {
    let download_dir = test_runtime_dir("diagnostic-events-compact");
    let log_path = download_dir.join("diagnostic-events.jsonl");
    let store =
        DiagnosticEventStore::new_with_limits(log_path.clone(), Duration::from_millis(60_000), 260);
    let now = current_unix_timestamp_millis();

    store
        .append(&diagnostic_test_event(
            "old event",
            now.saturating_sub(120_000),
        ))
        .unwrap();
    for index in 0..6 {
        store
            .append(&diagnostic_test_event(
                &format!("new event {index} with enough text to exercise byte trimming"),
                now + index,
            ))
            .unwrap();
    }

    let history = store
        .retained_events()
        .expect("compacted diagnostic history should load");
    let compacted = std::fs::metadata(&log_path).unwrap().len();

    assert!(compacted <= 260);
    assert!(history.iter().all(|event| event.message != "old event"));
    assert_eq!(
        history.last().map(|event| event.message.as_str()),
        Some("new event 5 with enough text to exercise byte trimming")
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn record_diagnostic_event_survives_unwritable_log_path() {
    let download_dir = test_runtime_dir("diagnostic-events-unwritable");
    let blocked_parent = download_dir.join("not-a-directory");
    std::fs::write(&blocked_parent, "blocks diagnostic log parent creation").unwrap();
    let state = shared_state_with_jobs(blocked_parent.join("state.json"), vec![]);

    let result = state
        .record_diagnostic_event(
            DiagnosticLevel::Warning,
            "test",
            "event survives log append failure",
            None,
        )
        .await;

    let snapshot = state
        .diagnostics_snapshot(HostRegistrationDiagnostics {
            status: HostRegistrationStatus::Configured,
            entries: Vec::new(),
        })
        .await;

    assert!(result.is_ok());
    assert_eq!(snapshot.recent_events.len(), 1);
    assert_eq!(
        snapshot.recent_events[0].message,
        "event survives log append failure"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn diagnostic_event_push_is_memory_only() {
    let source = include_str!("../runtime.rs");
    let body = source
        .split("pub(super) fn push_diagnostic_event")
        .nth(1)
        .expect("push_diagnostic_event should exist")
        .split("pub(super) fn snapshot")
        .next()
        .expect("push_diagnostic_event body should end before snapshot");

    assert!(
        body.contains("-> DiagnosticEvent"),
        "push_diagnostic_event should return the event for append outside the state lock"
    );
    assert!(
        !body.contains("diagnostic_event_store")
            && !body.contains(".append(")
            && !body.contains("spawn_blocking"),
        "push_diagnostic_event must only update in-memory diagnostics"
    );
}

#[test]
fn hot_diagnostic_state_paths_use_blocking_append_helper() {
    for (name, source) in [
        ("scheduler", include_str!("../scheduler.rs")),
        ("progress", include_str!("../progress.rs")),
        ("jobs", include_str!("../jobs.rs")),
        ("torrent", include_str!("../torrent.rs")),
        ("enqueue", include_str!("../enqueue.rs")),
    ] {
        assert!(
            !source.contains("diagnostic_event_store.append"),
            "{name} should not append diagnostic events directly"
        );
        assert!(
            source.contains("append_diagnostic_events_in_background")
                || source.contains("append_diagnostic_events_blocking"),
            "{name} should flush diagnostic events through a blocking-safe helper"
        );
    }
}

#[tokio::test]
async fn blocking_diagnostic_append_helper_persists_event_history() {
    let download_dir = test_runtime_dir("diagnostic-events-blocking-helper");
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);
    let event = diagnostic_test_event(
        "blocking helper persisted event",
        current_unix_timestamp_millis(),
    );

    state
        .append_diagnostic_events_blocking(vec![event.clone()])
        .await;

    let history = state
        .diagnostic_event_history()
        .await
        .expect("diagnostic event history should load");

    assert_eq!(history, vec![event]);
    let _ = std::fs::remove_dir_all(download_dir);
}
