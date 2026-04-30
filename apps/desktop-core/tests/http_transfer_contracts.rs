use simple_download_manager_desktop_core::storage::{DownloadPerformanceMode, ResumeSupport};
use simple_download_manager_desktop_core::transfer::{
    compute_sha256, plan_segmented_ranges_for_mode, ByteRange,
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn balanced_range_plan_uses_target_size_and_caps_at_six_segments() {
    let plan = plan_segmented_ranges_for_mode(
        512 * 1024 * 1024,
        ResumeSupport::Supported,
        None,
        DownloadPerformanceMode::Balanced,
    )
    .expect("balanced profile should segment large resumable downloads");

    assert_eq!(plan.total_bytes, 512 * 1024 * 1024);
    assert_eq!(plan.segments.len(), 6);
    assert_eq!(
        plan.segments[0],
        ByteRange {
            start: 0,
            end: 89_478_484
        }
    );
    assert_eq!(
        plan.segments[5],
        ByteRange {
            start: 447_392_425,
            end: 536_870_911,
        }
    );
}

#[test]
fn range_plan_falls_back_for_speed_limited_small_or_non_resumable_downloads() {
    assert!(plan_segmented_ranges_for_mode(
        512 * 1024 * 1024,
        ResumeSupport::Supported,
        Some(256 * 1024),
        DownloadPerformanceMode::Balanced,
    )
    .is_none());
    assert!(plan_segmented_ranges_for_mode(
        2 * 1024 * 1024,
        ResumeSupport::Supported,
        None,
        DownloadPerformanceMode::Balanced,
    )
    .is_none());
    assert!(plan_segmented_ranges_for_mode(
        512 * 1024 * 1024,
        ResumeSupport::Unsupported,
        None,
        DownloadPerformanceMode::Balanced,
    )
    .is_none());
}

#[tokio::test]
async fn sha256_digest_reads_file_contents() {
    let root = test_runtime_dir("sha256");
    let path = root.join("payload.bin");
    tokio::fs::write(&path, b"hello").await.unwrap();

    let digest = compute_sha256(&path).await.unwrap();

    assert_eq!(
        digest,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

fn test_runtime_dir(name: &str) -> std::path::PathBuf {
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!("transfer-{name}-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
