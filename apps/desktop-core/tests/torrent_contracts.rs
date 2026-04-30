use simple_download_manager_desktop_core::torrent::{
    pending_torrent_cleanup_info_hash, prepare_torrent_source, TorrentSourceKind,
    TORRENT_TRACKER_FIRST_METADATA_TIMEOUT,
};

#[test]
fn core_exposes_torrent_source_preparation_contracts() {
    let prepared =
        prepare_torrent_source("magnet:?xt=urn:btih:a634dc946d49989526058626caa3bbabba4607b6");

    assert_eq!(prepared.source_kind, TorrentSourceKind::Magnet);
    assert!(prepared.fallback_trackers_added > 0);
    assert!(prepared.tracker_first_metadata);
    assert_eq!(
        pending_torrent_cleanup_info_hash(&prepared).as_deref(),
        Some("a634dc946d49989526058626caa3bbabba4607b6")
    );
}

#[test]
fn core_exposes_tracker_first_metadata_timeout_contract() {
    assert_eq!(TORRENT_TRACKER_FIRST_METADATA_TIMEOUT.as_secs(), 15);
}
