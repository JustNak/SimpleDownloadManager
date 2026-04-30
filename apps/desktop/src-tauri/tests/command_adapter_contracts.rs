use simple_download_manager_desktop_backend::commands::{
    add_job_request_from_tauri_args, confirm_prompt_request_from_tauri_args,
    progress_batch_context_from_tauri_payload, ProgressBatchKind,
};
use simple_download_manager_desktop_backend::desktop_core::contracts::{
    ProgressBatchContext, PromptDuplicateAction,
};
use simple_download_manager_desktop_backend::storage::TransferKind;

#[test]
fn add_job_adapter_preserves_expected_sha_and_transfer_kind() {
    let request = add_job_request_from_tauri_args(
        "https://example.com/file.zip".into(),
        Some("f".repeat(64)),
        Some(TransferKind::Http),
    );

    assert_eq!(request.url, "https://example.com/file.zip");
    assert_eq!(
        request.expected_sha256.as_deref(),
        Some("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")
    );
    assert_eq!(request.transfer_kind, Some(TransferKind::Http));
}

#[test]
fn prompt_adapter_preserves_legacy_allow_duplicate_behavior() {
    let request =
        confirm_prompt_request_from_tauri_args("prompt_1".into(), None, Some(true), None, None);

    assert_eq!(
        request.duplicate_action,
        PromptDuplicateAction::DownloadAnyway
    );

    let explicit = confirm_prompt_request_from_tauri_args(
        "prompt_1".into(),
        None,
        Some(false),
        Some(PromptDuplicateAction::Overwrite),
        Some("renamed.zip".into()),
    );

    assert_eq!(explicit.duplicate_action, PromptDuplicateAction::Overwrite);
    assert_eq!(explicit.renamed_filename.as_deref(), Some("renamed.zip"));
}

#[test]
fn progress_batch_adapter_preserves_payload_shape() {
    let context = progress_batch_context_from_tauri_payload(ProgressBatchContext {
        batch_id: "batch_123".into(),
        kind: ProgressBatchKind::Bulk,
        job_ids: vec!["job_1".into(), "job_2".into()],
        title: "Archive progress".into(),
        archive_name: Some("bundle.zip".into()),
    });

    assert_eq!(context.batch_id, "batch_123");
    assert_eq!(context.kind, ProgressBatchKind::Bulk);
    assert_eq!(context.job_ids, ["job_1", "job_2"]);
    assert_eq!(context.title, "Archive progress");
    assert_eq!(context.archive_name.as_deref(), Some("bundle.zip"));
}
