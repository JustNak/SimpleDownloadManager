use simple_download_manager_desktop_core::contracts::{
    AddJobResult, AddJobStatus, AddJobsResult, ConfirmPromptRequest, ProgressBatchContext,
    ProgressBatchKind,
};
use simple_download_manager_desktop_core::prompts::PromptDuplicateAction;
use simple_download_manager_desktop_core::storage::{
    BulkArchiveStatus, ConnectionState, DesktopSnapshot, DiagnosticEvent, DiagnosticLevel,
    DiagnosticsSnapshot, DownloadHandoffMode, DownloadJob, DownloadPerformanceMode, DownloadPrompt,
    HostRegistrationEntry, HostRegistrationStatus, JobState, QueueRowSize, ResumeSupport, Settings,
    StartupLaunchMode, Theme, TorrentPeerConnectionWatchdogMode, TorrentRuntimeDiagnostics,
    TorrentSeedMode, TorrentSettings, TransferKind,
};
use slint::{ModelRc, SharedString, VecModel};
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

#[derive(Debug, Clone, PartialEq)]
pub struct JobRow {
    pub id: String,
    pub filename: String,
    pub state: String,
    pub progress: f64,
    pub bytes_text: String,
    pub selected: bool,
    pub transfer_kind: String,
    pub created_text: String,
    pub size_text: String,
    pub speed_text: String,
    pub status_detail: String,
    pub status_tone: String,
    pub can_pause: bool,
    pub can_resume: bool,
    pub can_cancel: bool,
    pub can_retry: bool,
    pub can_restart: bool,
    pub can_remove: bool,
    pub can_show_progress: bool,
    pub target_path: String,
    pub delete_label: String,
    pub can_open: bool,
    pub can_reveal: bool,
    pub can_swap_to_browser: bool,
    pub can_rename: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePromptContent {
    pub title: String,
    pub description: String,
    pub checkbox_label: String,
    pub confirm_label: String,
    pub context_menu_label: String,
    pub selected_summary: String,
    pub missing_path_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePromptJob {
    pub id: String,
    pub filename: String,
    pub target_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePromptDetails {
    pub content: DeletePromptContent,
    pub jobs: Vec<DeletePromptJob>,
    pub delete_from_disk: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitFilename {
    pub base_name: String,
    pub extension: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadMode {
    Single,
    Torrent,
    Multi,
    Bulk,
}

impl DownloadMode {
    pub fn id(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Torrent => "torrent",
            Self::Multi => "multi",
            Self::Bulk => "bulk",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "single" => Some(Self::Single),
            "torrent" => Some(Self::Torrent),
            "multi" => Some(Self::Multi),
            "bulk" => Some(Self::Bulk),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddDownloadFormState {
    pub mode: DownloadMode,
    pub single_url: String,
    pub torrent_url: String,
    pub single_sha256: String,
    pub multi_urls: String,
    pub bulk_urls: String,
    pub archive_name: String,
    pub combine_bulk: bool,
    pub error_text: String,
}

impl Default for AddDownloadFormState {
    fn default() -> Self {
        Self {
            mode: DownloadMode::Single,
            single_url: String::new(),
            torrent_url: String::new(),
            single_sha256: String::new(),
            multi_urls: String::new(),
            bulk_urls: String::new(),
            archive_name: "bulk-download.zip".into(),
            combine_bulk: true,
            error_text: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddDownloadFormModel {
    pub mode_id: String,
    pub active_urls: Vec<String>,
    pub submit_label: String,
    pub ready_label: String,
    pub ready_detail: String,
    pub can_submit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddDownloadProgressIntent {
    Single { job_id: String },
    Batch { context: ProgressBatchContext },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddDownloadResult {
    Single(AddJobResult),
    Batch(AddJobsResult),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddDownloadOutcome {
    pub primary_job_id: Option<String>,
    pub primary_filename: Option<String>,
    pub progress_intent: Option<AddDownloadProgressIntent>,
    pub view_id: &'static str,
    pub mode: DownloadMode,
    pub total_count: usize,
    pub queued_count: usize,
    pub duplicate_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DownloadCategory {
    Document,
    Program,
    Picture,
    Video,
    Compressed,
    Music,
    Other,
}

impl DownloadCategory {
    pub const ALL: [DownloadCategory; 7] = [
        DownloadCategory::Document,
        DownloadCategory::Program,
        DownloadCategory::Picture,
        DownloadCategory::Video,
        DownloadCategory::Compressed,
        DownloadCategory::Music,
        DownloadCategory::Other,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Program => "program",
            Self::Picture => "picture",
            Self::Video => "video",
            Self::Compressed => "compressed",
            Self::Music => "music",
            Self::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Document => "Document",
            Self::Program => "Program",
            Self::Picture => "Picture",
            Self::Video => "Video",
            Self::Compressed => "Compressed",
            Self::Music => "Music",
            Self::Other => "Other",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|category| category.id() == id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CategoryCounts {
    document: usize,
    program: usize,
    picture: usize,
    video: usize,
    compressed: usize,
    music: usize,
    other: usize,
}

impl CategoryCounts {
    pub fn get(&self, category: DownloadCategory) -> usize {
        match category {
            DownloadCategory::Document => self.document,
            DownloadCategory::Program => self.program,
            DownloadCategory::Picture => self.picture,
            DownloadCategory::Video => self.video,
            DownloadCategory::Compressed => self.compressed,
            DownloadCategory::Music => self.music,
            DownloadCategory::Other => self.other,
        }
    }

    fn increment(&mut self, category: DownloadCategory) {
        match category {
            DownloadCategory::Document => self.document += 1,
            DownloadCategory::Program => self.program += 1,
            DownloadCategory::Picture => self.picture += 1,
            DownloadCategory::Video => self.video += 1,
            DownloadCategory::Compressed => self.compressed += 1,
            DownloadCategory::Music => self.music += 1,
            DownloadCategory::Other => self.other += 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TorrentCounts {
    pub all: usize,
    pub active: usize,
    pub seeding: usize,
    pub attention: usize,
    pub queued: usize,
    pub completed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QueueCounts {
    pub all: usize,
    pub active: usize,
    pub attention: usize,
    pub queued: usize,
    pub completed: usize,
    pub categories: CategoryCounts,
    pub torrents: TorrentCounts,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TorrentFooterStats {
    pub all: usize,
    pub active: usize,
    pub seeding: usize,
    pub uploaded_bytes: u64,
    pub total_ratio: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ViewFilter {
    #[default]
    All,
    Attention,
    Active,
    Queued,
    Completed,
    Category(DownloadCategory),
    Torrents,
    TorrentActive,
    TorrentSeeding,
    TorrentAttention,
    TorrentQueued,
    TorrentCompleted,
}

impl ViewFilter {
    pub fn id(self) -> String {
        match self {
            Self::All => "all".into(),
            Self::Attention => "attention".into(),
            Self::Active => "active".into(),
            Self::Queued => "queued".into(),
            Self::Completed => "completed".into(),
            Self::Category(category) => format!("category:{}", category.id()),
            Self::Torrents => "torrents".into(),
            Self::TorrentActive => "torrent-active".into(),
            Self::TorrentSeeding => "torrent-seeding".into(),
            Self::TorrentAttention => "torrent-attention".into(),
            Self::TorrentQueued => "torrent-queued".into(),
            Self::TorrentCompleted => "torrent-completed".into(),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All downloads",
            Self::Attention => "Attention",
            Self::Active => "Active",
            Self::Queued => "Queued",
            Self::Completed => "Completed",
            Self::Category(category) => category.label(),
            Self::Torrents => "Torrents",
            Self::TorrentActive => "Torrent active",
            Self::TorrentSeeding => "Seeding",
            Self::TorrentAttention => "Torrent attention",
            Self::TorrentQueued => "Torrent queued",
            Self::TorrentCompleted => "Torrent completed",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "all" => Some(Self::All),
            "attention" => Some(Self::Attention),
            "active" => Some(Self::Active),
            "queued" => Some(Self::Queued),
            "completed" => Some(Self::Completed),
            "torrents" => Some(Self::Torrents),
            "torrent-active" => Some(Self::TorrentActive),
            "torrent-seeding" => Some(Self::TorrentSeeding),
            "torrent-attention" => Some(Self::TorrentAttention),
            "torrent-queued" => Some(Self::TorrentQueued),
            "torrent-completed" => Some(Self::TorrentCompleted),
            _ => id
                .strip_prefix("category:")
                .and_then(DownloadCategory::from_id)
                .map(Self::Category),
        }
    }

    pub fn is_torrent(self) -> bool {
        matches!(
            self,
            Self::Torrents
                | Self::TorrentActive
                | Self::TorrentSeeding
                | Self::TorrentAttention
                | Self::TorrentQueued
                | Self::TorrentCompleted
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Date,
    Size,
}

impl SortColumn {
    pub fn id(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Date => "date",
            Self::Size => "size",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "name" => Some(Self::Name),
            "date" => Some(Self::Date),
            "size" => Some(Self::Size),
            _ => None,
        }
    }

    fn default_direction(self) -> SortDirection {
        if self == Self::Name {
            SortDirection::Asc
        } else {
            SortDirection::Desc
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    pub fn id(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }

    fn toggled(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortMode {
    pub column: SortColumn,
    pub direction: SortDirection,
}

impl SortMode {
    pub fn new(column: SortColumn, direction: SortDirection) -> Self {
        Self { column, direction }
    }

    pub fn next_for_column(self, column: SortColumn) -> Self {
        if self.column == column {
            Self::new(column, self.direction.toggled())
        } else {
            Self::new(column, column.default_direction())
        }
    }
}

impl Default for SortMode {
    fn default() -> Self {
        Self::new(SortColumn::Date, SortDirection::Asc)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionState {
    selected_ids: BTreeSet<String>,
    anchor_id: Option<String>,
}

impl SelectionState {
    pub fn select_single(&mut self, id: &str) {
        self.selected_ids.clear();
        self.selected_ids.insert(id.into());
        self.anchor_id = Some(id.into());
    }

    pub fn toggle(&mut self, id: &str, selected: bool) {
        if selected {
            self.selected_ids.insert(id.into());
            self.anchor_id = Some(id.into());
        } else {
            self.selected_ids.remove(id);
            if self.anchor_id.as_deref() == Some(id) {
                self.anchor_id = self.selected_ids.iter().next().cloned();
            }
        }
    }

    pub fn select_all(&mut self, ids: &[String]) {
        self.selected_ids = ids.iter().cloned().collect();
        self.anchor_id = ids.first().cloned();
    }

    pub fn clear(&mut self) {
        self.selected_ids.clear();
        self.anchor_id = None;
    }

    pub fn prune_to_visible(&mut self, visible_ids: &[String]) {
        let visible: BTreeSet<&String> = visible_ids.iter().collect();
        self.selected_ids.retain(|id| visible.contains(id));
        if self
            .anchor_id
            .as_ref()
            .is_some_and(|id| !self.selected_ids.contains(id))
        {
            self.anchor_id = self.selected_ids.iter().next().cloned();
        }
    }

    pub fn selected_ids(&self) -> Vec<String> {
        self.selected_ids.iter().cloned().collect()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.selected_ids.contains(id)
    }

    pub fn len(&self) -> usize {
        self.selected_ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.selected_ids.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueUiState {
    pub view: ViewFilter,
    pub search_query: String,
    pub sort_mode: SortMode,
    pub selection: SelectionState,
}

impl Default for QueueUiState {
    fn default() -> Self {
        Self {
            view: ViewFilter::All,
            search_query: String::new(),
            sort_mode: SortMode::default(),
            selection: SelectionState::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueNavItem {
    pub id: String,
    pub label: String,
    pub count: usize,
    pub active: bool,
    pub section: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueueViewModel {
    pub rows: Vec<JobRow>,
    pub nav_items: Vec<QueueNavItem>,
    pub counts: QueueCounts,
    pub title: String,
    pub subtitle: String,
    pub footer_text: String,
    pub empty_text: String,
    pub selected_count: usize,
    pub all_visible_selected: bool,
    pub has_visible_selection: bool,
    pub sort_mode: SortMode,
    pub view_id: String,
    pub search_query: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SettingsSection {
    #[default]
    General,
    Updates,
    Torrenting,
    Appearance,
    Extension,
    NativeHost,
}

impl SettingsSection {
    pub const ALL: [SettingsSection; 6] = [
        SettingsSection::General,
        SettingsSection::Updates,
        SettingsSection::Torrenting,
        SettingsSection::Appearance,
        SettingsSection::Extension,
        SettingsSection::NativeHost,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Updates => "updates",
            Self::Torrenting => "torrenting",
            Self::Appearance => "appearance",
            Self::Extension => "extension",
            Self::NativeHost => "native_host",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Updates => "App Updates",
            Self::Torrenting => "Torrenting",
            Self::Appearance => "Appearance/Behavior",
            Self::Extension => "Web Extension",
            Self::NativeHost => "Native Host",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|section| section.id() == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsNavItem {
    pub id: String,
    pub label: String,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct SettingsDraftState {
    pub saved: Settings,
    pub draft: Settings,
    pub active_section: SettingsSection,
    pub visible: bool,
    pub unsaved_prompt_visible: bool,
    pub error_text: String,
    pub excluded_host_input: String,
    pub saving: bool,
    pub cache_clearing: bool,
}

impl SettingsDraftState {
    pub fn new(settings: Settings) -> Self {
        Self {
            saved: settings.clone(),
            draft: settings,
            active_section: SettingsSection::General,
            visible: false,
            unsaved_prompt_visible: false,
            error_text: String::new(),
            excluded_host_input: String::new(),
            saving: false,
            cache_clearing: false,
        }
    }

    pub fn dirty(&self) -> bool {
        !settings_equal(&self.saved, &self.draft)
    }

    pub fn adopt_incoming_settings(&mut self, next_settings: Settings) {
        if should_adopt_incoming_settings_draft(&self.draft, &self.saved, &next_settings) {
            self.saved = next_settings.clone();
            self.draft = next_settings;
            self.error_text.clear();
        }
    }

    pub fn discard(&mut self) {
        self.draft = self.saved.clone();
        self.unsaved_prompt_visible = false;
        self.error_text.clear();
    }
}

impl Default for SettingsDraftState {
    fn default() -> Self {
        Self::new(Settings::default())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsViewModel {
    pub sections: Vec<SettingsNavItem>,
    pub active_section_id: String,
    pub dirty: bool,
    pub visible: bool,
    pub unsaved_prompt_visible: bool,
    pub error_text: String,
    pub saving: bool,
    pub cache_clearing: bool,
    pub download_directory: String,
    pub max_concurrent_downloads: String,
    pub auto_retry_attempts: String,
    pub speed_limit_kib_per_second: String,
    pub download_performance_mode_id: String,
    pub notifications_enabled: bool,
    pub show_details_on_click: bool,
    pub queue_row_size_id: String,
    pub start_on_startup: bool,
    pub startup_launch_mode_id: String,
    pub theme_id: String,
    pub accent_color: String,
    pub torrent_enabled: bool,
    pub torrent_download_directory: String,
    pub torrent_seed_mode_id: String,
    pub torrent_seed_ratio_limit: String,
    pub torrent_seed_time_limit_minutes: String,
    pub torrent_upload_limit_kib_per_second: String,
    pub torrent_port_forwarding_enabled: bool,
    pub torrent_port_forwarding_port: String,
    pub torrent_peer_watchdog_mode_id: String,
    pub extension_enabled: bool,
    pub extension_handoff_mode_id: String,
    pub extension_listen_port: String,
    pub extension_context_menu_enabled: bool,
    pub extension_show_progress_after_handoff: bool,
    pub extension_show_badge_status: bool,
    pub extension_authenticated_handoff_enabled: bool,
    pub extension_excluded_host_input: String,
    pub extension_excluded_hosts_summary: String,
    pub extension_excluded_hosts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRegistrationEntryRow {
    pub browser: String,
    pub registry_path: String,
    pub manifest_path: String,
    pub host_binary_path: String,
    pub manifest_exists: bool,
    pub host_binary_exists: bool,
    pub status_label: String,
    pub status_tone: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticEventRow {
    pub level: String,
    pub level_tone: String,
    pub timestamp_text: String,
    pub message: String,
    pub category: String,
    pub job_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentDiagnosticRow {
    pub job_id: String,
    pub filename: String,
    pub info_hash: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsViewModel {
    pub has_snapshot: bool,
    pub loading: bool,
    pub status_label: String,
    pub status_message: String,
    pub status_tone: String,
    pub last_host_contact_text: String,
    pub queue_summary_text: String,
    pub host_entries: Vec<HostRegistrationEntryRow>,
    pub recent_events: Vec<DiagnosticEventRow>,
    pub torrent_diagnostics: Vec<TorrentDiagnosticRow>,
    pub action_status_text: String,
    pub error_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddExcludedHostsResult {
    pub hosts: Vec<String>,
    pub added_hosts: Vec<String>,
    pub duplicate_hosts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PromptWindowDetails {
    pub id: String,
    pub title: String,
    pub filename: String,
    pub url: String,
    pub destination: String,
    pub size_text: String,
    pub duplicate_text: String,
    pub is_duplicate: bool,
    pub duplicate_label: String,
    pub duplicate_message: String,
    pub overwrite_label: String,
    pub source_label: String,
    pub can_swap_to_browser: bool,
    pub duplicate_menu_open: bool,
    pub renaming_duplicate: bool,
    pub renamed_filename: String,
    pub rename_can_confirm: bool,
    pub busy: bool,
    pub error_text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptWindowInteractionState {
    pub directory_override: Option<String>,
    pub duplicate_menu_open: bool,
    pub renaming_duplicate: bool,
    pub renamed_filename: String,
    pub busy: bool,
    pub error_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptConfirmAction {
    DefaultDownload,
    DownloadAnyway,
    Overwrite,
    Rename,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProgressPopupInteractionState {
    pub busy: bool,
    pub cancel_confirming: bool,
    pub error_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressSample {
    pub job_id: String,
    pub timestamp: u64,
    pub downloaded_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DownloadProgressMetrics {
    pub average_speed: u64,
    pub time_remaining: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProgressWindowDetails {
    pub id: String,
    pub title: String,
    pub filename: String,
    pub state: String,
    pub bytes_text: String,
    pub progress: f64,
    pub error_text: String,
    pub subtitle: String,
    pub destination: String,
    pub status_tone: String,
    pub progress_label: String,
    pub speed_text: String,
    pub eta_text: String,
    pub size_text: String,
    pub source_summary: String,
    pub info_hash: String,
    pub remaining_text: String,
    pub upload_speed_text: String,
    pub peers_text: String,
    pub seeds_text: String,
    pub ratio_text: String,
    pub files_text: String,
    pub peer_health_tones: Vec<String>,
    pub can_open: bool,
    pub can_reveal: bool,
    pub can_pause: bool,
    pub can_resume: bool,
    pub can_retry: bool,
    pub can_cancel: bool,
    pub can_swap_to_browser: bool,
    pub can_close: bool,
    pub busy: bool,
    pub cancel_confirming: bool,
    pub action_error_text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatchProgressJobRow {
    pub id: String,
    pub filename: String,
    pub subtitle: String,
    pub status_text: String,
    pub status_tone: String,
    pub progress: f64,
    pub progress_label: String,
    pub speed_text: String,
    pub size_text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatchProgressDetails {
    pub batch_id: String,
    pub title: String,
    pub display_title: String,
    pub summary: String,
    pub bytes_text: String,
    pub progress: f64,
    pub completed_count: usize,
    pub failed_count: usize,
    pub active_count: usize,
    pub total_count: usize,
    pub phase_id: String,
    pub phase_label: String,
    pub phase_tone: String,
    pub archive_error: String,
    pub rows: Vec<BatchProgressJobRow>,
    pub can_pause: bool,
    pub can_resume: bool,
    pub can_cancel: bool,
    pub can_reveal_completed: bool,
    pub reveal_completed_job_id: String,
    pub can_close: bool,
    pub busy: bool,
    pub action_error_text: String,
}

pub const TOAST_AUTO_CLOSE_MS: u64 = 3_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastType {
    Info,
    Success,
    Warning,
    Error,
}

impl ToastType {
    pub fn id(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    pub fn tone(self) -> &'static str {
        self.id()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastMessage {
    pub id: String,
    pub toast_type: ToastType,
    pub title: String,
    pub message: String,
    pub auto_close: bool,
}

impl ToastMessage {
    pub fn new(
        toast_type: ToastType,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id: String::new(),
            toast_type,
            title: title.into(),
            message: message.into(),
            auto_close: true,
        }
    }

    pub fn persistent(
        toast_type: ToastType,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            auto_close: false,
            ..Self::new(toast_type, title, message)
        }
    }
}

pub fn job_row_from_job(job: &DownloadJob) -> JobRow {
    job_row_from_job_with_selection(job, false)
}

pub fn job_row_from_job_with_selection(job: &DownloadJob, selected: bool) -> JobRow {
    JobRow {
        id: job.id.clone(),
        filename: job.filename.clone(),
        state: job_state_label(job.state).into(),
        progress: job.progress,
        bytes_text: bytes_text(job.downloaded_bytes, job.total_bytes),
        selected,
        transfer_kind: transfer_kind_id(job.transfer_kind).into(),
        created_text: created_text(job.created_at),
        size_text: if job.total_bytes > 0 {
            format_bytes(job.total_bytes)
        } else {
            "Unknown".into()
        },
        speed_text: if job.speed > 0 {
            format!("{}/s", format_bytes(job.speed))
        } else {
            String::new()
        },
        status_detail: status_detail(job),
        status_tone: status_tone(job).into(),
        can_pause: matches!(
            job.state,
            JobState::Queued | JobState::Starting | JobState::Downloading | JobState::Seeding
        ),
        can_resume: job.state == JobState::Paused,
        can_cancel: !matches!(
            job.state,
            JobState::Completed | JobState::Canceled | JobState::Failed
        ),
        can_retry: matches!(job.state, JobState::Failed | JobState::Canceled),
        can_restart: !matches!(
            job.state,
            JobState::Starting | JobState::Downloading | JobState::Seeding
        ),
        can_remove: can_remove_download_immediately(job),
        can_show_progress: can_show_progress_popup(job),
        target_path: job.target_path.clone(),
        delete_label: delete_action_label_for_job(job),
        can_open: !job.target_path.trim().is_empty(),
        can_reveal: !job.target_path.trim().is_empty(),
        can_swap_to_browser: can_swap_failed_download_to_browser(job),
        can_rename: can_remove_download_immediately(job),
    }
}

pub fn status_text_from_snapshot(snapshot: &DesktopSnapshot) -> String {
    let connection = match snapshot.connection_state {
        ConnectionState::Checking => "Checking browser handoff",
        ConnectionState::Connected => "Connected to browser handoff",
        ConnectionState::HostMissing => "Browser host is missing",
        ConnectionState::AppMissing => "Desktop app is missing",
        ConnectionState::AppUnreachable => "Desktop app is unreachable",
        ConnectionState::Error => "Browser handoff error",
    };

    let count = snapshot.jobs.len();
    let suffix = if count == 1 { "download" } else { "downloads" };
    format!("{connection} | {count} {suffix}")
}

pub fn rows_from_snapshot(snapshot: &DesktopSnapshot) -> Vec<JobRow> {
    snapshot.jobs.iter().map(job_row_from_job).collect()
}

pub fn slint_rows_from_snapshot(snapshot: &DesktopSnapshot) -> Vec<crate::JobRow> {
    let state = QueueUiState::default();
    queue_view_model_from_snapshot(snapshot, &state)
        .rows
        .into_iter()
        .map(slint_job_row_from_row)
        .collect()
}

pub fn slint_job_row_from_job(job: &DownloadJob) -> crate::JobRow {
    slint_job_row_from_row(job_row_from_job(job))
}

pub fn slint_job_row_from_row(row: JobRow) -> crate::JobRow {
    crate::JobRow {
        id: row.id.into(),
        filename: row.filename.into(),
        state: row.state.into(),
        progress: row.progress as f32,
        bytes_text: row.bytes_text.into(),
        selected: row.selected,
        transfer_kind: row.transfer_kind.into(),
        created_text: row.created_text.into(),
        size_text: row.size_text.into(),
        speed_text: row.speed_text.into(),
        status_detail: row.status_detail.into(),
        status_tone: row.status_tone.into(),
        can_pause: row.can_pause,
        can_resume: row.can_resume,
        can_cancel: row.can_cancel,
        can_retry: row.can_retry,
        can_restart: row.can_restart,
        can_remove: row.can_remove,
        can_show_progress: row.can_show_progress,
        target_path: row.target_path.into(),
        delete_label: row.delete_label.into(),
        can_open: row.can_open,
        can_reveal: row.can_reveal,
        can_swap_to_browser: row.can_swap_to_browser,
        can_rename: row.can_rename,
    }
}

pub fn delete_prompt_content(selected_count: usize) -> DeletePromptContent {
    let count = selected_count.max(1);
    if count == 1 {
        return DeletePromptContent {
            title: "Delete Download".into(),
            description:
                "Remove this download from the list. Disk deletion requires explicit confirmation below."
                    .into(),
            checkbox_label: "Delete file from disk".into(),
            confirm_label: "Delete".into(),
            context_menu_label: "Delete".into(),
            selected_summary: "1 download selected".into(),
            missing_path_label: "No file path is recorded for this download.".into(),
        };
    }

    DeletePromptContent {
        title: format!("Delete {count} Downloads"),
        description:
            "Remove these downloads from the list. Disk deletion requires explicit confirmation below."
                .into(),
        checkbox_label: "Delete selected files from disk".into(),
        confirm_label: "Delete All".into(),
        context_menu_label: "Delete All".into(),
        selected_summary: format!("{count} downloads selected"),
        missing_path_label: "No file path is recorded for this download.".into(),
    }
}

pub fn delete_context_menu_label(selected_count: usize) -> String {
    delete_prompt_content(selected_count).context_menu_label
}

pub fn delete_action_label_for_job(job: &DownloadJob) -> String {
    if is_paused_seeding_torrent_delete_candidate(job) {
        "Delete from disk...".into()
    } else {
        "Delete".into()
    }
}

pub fn default_delete_from_disk_for_jobs(jobs: &[DownloadJob]) -> bool {
    jobs.len() == 1 && is_paused_seeding_torrent_delete_candidate(&jobs[0])
}

pub fn delete_prompt_from_jobs(jobs: &[DownloadJob]) -> Option<DeletePromptDetails> {
    let removable_jobs: Vec<DownloadJob> = jobs
        .iter()
        .filter(|job| can_remove_download_immediately(job))
        .cloned()
        .collect();
    if removable_jobs.is_empty() {
        return None;
    }

    Some(DeletePromptDetails {
        content: delete_prompt_content(removable_jobs.len()),
        delete_from_disk: default_delete_from_disk_for_jobs(&removable_jobs),
        jobs: removable_jobs
            .iter()
            .map(|job| DeletePromptJob {
                id: job.id.clone(),
                filename: job.filename.clone(),
                target_path: job.target_path.clone(),
            })
            .collect(),
    })
}

pub fn split_filename(filename: &str) -> SplitFilename {
    let Some(dot_index) = filename.rfind('.') else {
        return SplitFilename {
            base_name: filename.into(),
            extension: String::new(),
        };
    };

    if dot_index == 0 || dot_index == filename.len() - 1 {
        return SplitFilename {
            base_name: filename.into(),
            extension: String::new(),
        };
    }

    SplitFilename {
        base_name: filename[..dot_index].into(),
        extension: filename[dot_index + 1..].into(),
    }
}

pub fn normalize_extension_input(value: &str) -> String {
    value
        .trim_start_matches('.')
        .chars()
        .filter(|ch| {
            !matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
                && !ch.is_control()
                && !ch.is_whitespace()
        })
        .collect::<String>()
        .trim()
        .into()
}

pub fn build_filename(base_name: &str, extension: &str) -> Option<String> {
    let name = base_name.trim();
    if name.is_empty() {
        return None;
    }
    let normalized_extension = normalize_extension_input(extension);
    if normalized_extension.is_empty() {
        Some(name.into())
    } else {
        Some(format!("{name}.{normalized_extension}"))
    }
}

pub fn parse_download_url_lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub fn ensure_trailing_editable_line(value: &str) -> String {
    let normalized = value.replace("\r\n", "\n");
    if normalized.trim().is_empty() {
        return String::new();
    }
    if normalized.ends_with('\n') {
        normalized
    } else {
        format!("{normalized}\n")
    }
}

pub fn download_submit_label(mode: DownloadMode, link_count: usize, combine_bulk: bool) -> String {
    match mode {
        DownloadMode::Single => "Start Download".into(),
        DownloadMode::Torrent => "Add Torrent".into(),
        DownloadMode::Bulk if combine_bulk => {
            if link_count > 0 {
                format!("Queue {} and Combine", download_count_label(link_count))
            } else {
                "Queue and Combine".into()
            }
        }
        DownloadMode::Multi | DownloadMode::Bulk => {
            if link_count > 0 {
                format!("Queue {}", download_count_label(link_count))
            } else {
                "Queue Downloads".into()
            }
        }
    }
}

pub fn download_ready_label(mode: DownloadMode, link_count: usize) -> String {
    if mode == DownloadMode::Torrent {
        format!(
            "{} {} ready",
            link_count,
            if link_count == 1 {
                "torrent"
            } else {
                "torrents"
            }
        )
    } else {
        format!(
            "{} {} ready",
            link_count,
            if link_count == 1 { "link" } else { "links" }
        )
    }
}

pub fn download_ready_detail(mode: DownloadMode, combine_bulk: bool, archive_name: &str) -> String {
    match mode {
        DownloadMode::Torrent => "Torrent".into(),
        DownloadMode::Bulk if combine_bulk => archive_name.into(),
        _ => "Queue only".into(),
    }
}

pub fn validate_optional_sha256(value: &str) -> Result<Option<String>, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Ok(None);
    }
    let valid = normalized.len() == 64
        && normalized
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase());
    if valid {
        Ok(Some(normalized))
    } else {
        Err("SHA-256 checksum must be 64 hexadecimal characters.".into())
    }
}

pub fn normalize_archive_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .filter(|ch| {
            !matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') && !ch.is_control()
        })
        .collect::<String>();
    let sanitized = sanitized.trim_start();
    if sanitized.is_empty() {
        String::new()
    } else if sanitized.to_ascii_lowercase().ends_with(".zip") {
        sanitized.into()
    } else {
        format!("{sanitized}.zip")
    }
}

pub fn infer_transfer_kind_for_url(url: &str) -> TransferKind {
    let normalized = url.trim().to_ascii_lowercase();
    if normalized.starts_with("magnet:") {
        return TransferKind::Torrent;
    }
    let without_fragment = normalized.split('#').next().unwrap_or(normalized.as_str());
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    if without_query.contains("://") && without_query.ends_with(".torrent") {
        TransferKind::Torrent
    } else {
        TransferKind::Http
    }
}

pub fn active_download_urls(state: &AddDownloadFormState) -> Vec<String> {
    match state.mode {
        DownloadMode::Single => trimmed_single_url(&state.single_url),
        DownloadMode::Torrent => trimmed_single_url(&state.torrent_url),
        DownloadMode::Multi => parse_download_url_lines(&state.multi_urls),
        DownloadMode::Bulk => parse_download_url_lines(&state.bulk_urls),
    }
}

pub fn add_download_form_model(state: &AddDownloadFormState) -> AddDownloadFormModel {
    let active_urls = active_download_urls(state);
    AddDownloadFormModel {
        mode_id: state.mode.id().into(),
        submit_label: download_submit_label(state.mode, active_urls.len(), state.combine_bulk),
        ready_label: download_ready_label(state.mode, active_urls.len()),
        ready_detail: download_ready_detail(state.mode, state.combine_bulk, &state.archive_name),
        can_submit: !active_urls.is_empty(),
        active_urls,
    }
}

pub fn add_download_outcome_for_result(
    mode: DownloadMode,
    result: AddDownloadResult,
    archive_name: Option<&str>,
) -> AddDownloadOutcome {
    let view_id = if mode == DownloadMode::Torrent {
        "torrents"
    } else {
        "all"
    };

    match result {
        AddDownloadResult::Single(result) => {
            let queued = result.status == AddJobStatus::Queued;
            let duplicate = result.status == AddJobStatus::DuplicateExistingJob;
            let progress_intent = if queued {
                Some(AddDownloadProgressIntent::Single {
                    job_id: result.job_id.clone(),
                })
            } else {
                None
            };
            AddDownloadOutcome {
                primary_job_id: Some(result.job_id),
                primary_filename: Some(result.filename),
                progress_intent,
                view_id,
                mode,
                total_count: 1,
                queued_count: usize::from(queued),
                duplicate_count: usize::from(duplicate),
            }
        }
        AddDownloadResult::Batch(result) => {
            let total_count = result.results.len();
            let duplicate_count = result
                .results
                .iter()
                .filter(|item| item.status == AddJobStatus::DuplicateExistingJob)
                .count();
            let queued_ids = result
                .results
                .iter()
                .filter(|item| item.status == AddJobStatus::Queued)
                .map(|item| item.job_id.clone())
                .collect::<Vec<_>>();
            let queued_count = queued_ids.len();
            let primary_job_id = result
                .results
                .iter()
                .find(|item| item.status == AddJobStatus::Queued)
                .or_else(|| result.results.first())
                .map(|item| item.job_id.clone());
            let primary_filename = result
                .results
                .iter()
                .find(|item| item.status == AddJobStatus::Queued)
                .or_else(|| result.results.first())
                .map(|item| item.filename.clone());
            let progress_intent = if queued_ids.is_empty() {
                None
            } else {
                let bulk_archive = mode == DownloadMode::Bulk && archive_name.is_some();
                Some(AddDownloadProgressIntent::Batch {
                    context: ProgressBatchContext {
                        batch_id: next_add_download_batch_id(),
                        kind: if bulk_archive {
                            ProgressBatchKind::Bulk
                        } else {
                            ProgressBatchKind::Multi
                        },
                        job_ids: queued_ids,
                        title: if bulk_archive {
                            "Bulk download progress".into()
                        } else {
                            "Multi-download progress".into()
                        },
                        archive_name: archive_name.map(ToOwned::to_owned),
                    },
                })
            };

            AddDownloadOutcome {
                primary_job_id,
                primary_filename,
                progress_intent,
                view_id,
                mode,
                total_count,
                queued_count,
                duplicate_count,
            }
        }
    }
}

pub fn settings_equal(left: &Settings, right: &Settings) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

pub fn should_adopt_incoming_settings_draft(
    current_draft: &Settings,
    previous_settings: &Settings,
    next_settings: &Settings,
) -> bool {
    settings_equal(current_draft, previous_settings) || settings_equal(current_draft, next_settings)
}

pub fn settings_view_model_from_state(state: &SettingsDraftState) -> SettingsViewModel {
    let settings = &state.draft;
    SettingsViewModel {
        sections: SettingsSection::ALL
            .into_iter()
            .map(|section| SettingsNavItem {
                id: section.id().into(),
                label: section.label().into(),
                active: section == state.active_section,
            })
            .collect(),
        active_section_id: state.active_section.id().into(),
        dirty: state.dirty(),
        visible: state.visible,
        unsaved_prompt_visible: state.unsaved_prompt_visible,
        error_text: state.error_text.clone(),
        saving: state.saving,
        cache_clearing: state.cache_clearing,
        download_directory: settings.download_directory.clone(),
        max_concurrent_downloads: settings.max_concurrent_downloads.to_string(),
        auto_retry_attempts: settings.auto_retry_attempts.to_string(),
        speed_limit_kib_per_second: settings.speed_limit_kib_per_second.to_string(),
        download_performance_mode_id: performance_mode_id(settings.download_performance_mode)
            .into(),
        notifications_enabled: settings.notifications_enabled,
        show_details_on_click: settings.show_details_on_click,
        queue_row_size_id: queue_row_size_id(settings.queue_row_size).into(),
        start_on_startup: settings.start_on_startup,
        startup_launch_mode_id: startup_launch_mode_id(settings.startup_launch_mode).into(),
        theme_id: theme_id(&settings.theme).into(),
        accent_color: normalize_accent_color(&settings.accent_color),
        torrent_enabled: settings.torrent.enabled,
        torrent_download_directory: settings.torrent.download_directory.clone(),
        torrent_seed_mode_id: torrent_seed_mode_id(settings.torrent.seed_mode).into(),
        torrent_seed_ratio_limit: trim_float(settings.torrent.seed_ratio_limit),
        torrent_seed_time_limit_minutes: settings.torrent.seed_time_limit_minutes.to_string(),
        torrent_upload_limit_kib_per_second: settings
            .torrent
            .upload_limit_kib_per_second
            .to_string(),
        torrent_port_forwarding_enabled: settings.torrent.port_forwarding_enabled,
        torrent_port_forwarding_port: settings.torrent.port_forwarding_port.to_string(),
        torrent_peer_watchdog_mode_id: torrent_peer_watchdog_mode_id(
            settings.torrent.peer_connection_watchdog_mode,
        )
        .into(),
        extension_enabled: settings.extension_integration.enabled,
        extension_handoff_mode_id: handoff_mode_id(
            settings.extension_integration.download_handoff_mode,
        )
        .into(),
        extension_listen_port: settings.extension_integration.listen_port.to_string(),
        extension_context_menu_enabled: settings.extension_integration.context_menu_enabled,
        extension_show_progress_after_handoff: settings
            .extension_integration
            .show_progress_after_handoff,
        extension_show_badge_status: settings.extension_integration.show_badge_status,
        extension_authenticated_handoff_enabled: settings
            .extension_integration
            .authenticated_handoff_enabled,
        extension_excluded_host_input: state.excluded_host_input.clone(),
        extension_excluded_hosts_summary: format_excluded_sites_summary(
            &settings.extension_integration.excluded_hosts,
        ),
        extension_excluded_hosts: settings.extension_integration.excluded_hosts.clone(),
    }
}

pub fn registration_status_label(status: Option<HostRegistrationStatus>) -> &'static str {
    match status {
        Some(HostRegistrationStatus::Configured) => "Ready",
        Some(HostRegistrationStatus::Broken) => "Repair",
        Some(HostRegistrationStatus::Missing) => "Missing",
        None => "Checking",
    }
}

pub fn registration_status_message(status: Option<HostRegistrationStatus>) -> &'static str {
    match status {
        Some(HostRegistrationStatus::Configured) => {
            "At least one browser has a valid native host registration and host binary path."
        }
        Some(HostRegistrationStatus::Broken) => {
            "A browser registration exists, but the manifest or native host binary path is broken."
        }
        Some(HostRegistrationStatus::Missing) => {
            "No browser registration was detected for the native messaging host."
        }
        None => "Diagnostics are still loading.",
    }
}

pub fn registration_status_tone(status: Option<HostRegistrationStatus>) -> &'static str {
    match status {
        Some(HostRegistrationStatus::Configured) => "success",
        Some(HostRegistrationStatus::Broken) => "warning",
        Some(HostRegistrationStatus::Missing) => "error",
        None => "neutral",
    }
}

pub fn diagnostics_view_model_from_snapshot(
    diagnostics: Option<&DiagnosticsSnapshot>,
    loading: bool,
    action_status_text: &str,
    error_text: &str,
) -> DiagnosticsViewModel {
    let status = diagnostics.map(|snapshot| snapshot.host_registration.status);
    DiagnosticsViewModel {
        has_snapshot: diagnostics.is_some(),
        loading,
        status_label: registration_status_label(status).into(),
        status_message: registration_status_message(status).into(),
        status_tone: registration_status_tone(status).into(),
        last_host_contact_text: diagnostics
            .map(last_host_contact_text)
            .unwrap_or_else(|| "Never".into()),
        queue_summary_text: diagnostics
            .map(queue_summary_text)
            .unwrap_or_else(|| "No diagnostics loaded".into()),
        host_entries: diagnostics
            .map(|snapshot| {
                snapshot
                    .host_registration
                    .entries
                    .iter()
                    .map(host_registration_entry_row)
                    .collect()
            })
            .unwrap_or_default(),
        recent_events: diagnostics
            .map(|snapshot| {
                snapshot
                    .recent_events
                    .iter()
                    .rev()
                    .map(diagnostic_event_row)
                    .collect()
            })
            .unwrap_or_default(),
        torrent_diagnostics: diagnostics
            .map(|snapshot| {
                snapshot
                    .torrent_diagnostics
                    .iter()
                    .map(|torrent| TorrentDiagnosticRow {
                        job_id: torrent.job_id.clone(),
                        filename: torrent.filename.clone(),
                        info_hash: torrent
                            .info_hash
                            .clone()
                            .unwrap_or_else(|| "Unknown".into()),
                        summary: format!(
                            "{} live peers, {} B/s down, {} B/s up",
                            torrent.diagnostics.live_peers,
                            torrent.diagnostics.session_download_speed,
                            torrent.diagnostics.session_upload_speed
                        ),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        action_status_text: action_status_text.into(),
        error_text: error_text.into(),
    }
}

pub fn format_diagnostics_report(diagnostics: &DiagnosticsSnapshot) -> String {
    let mut lines = vec![
        "Simple Download Manager Diagnostics".to_string(),
        format!(
            "Connection State: {}",
            connection_state_id(diagnostics.connection_state)
        ),
        format!(
            "Last Host Contact: {} seconds ago",
            diagnostics
                .last_host_contact_seconds_ago
                .map(|seconds| seconds.to_string())
                .unwrap_or_else(|| "never".into())
        ),
        format!("Queue Total: {}", diagnostics.queue_summary.total),
        format!("Queue Active: {}", diagnostics.queue_summary.active),
        format!(
            "Queue Needs Attention: {}",
            diagnostics.queue_summary.attention
        ),
        format!("Queue Queued: {}", diagnostics.queue_summary.queued),
        format!(
            "Queue Downloading: {}",
            diagnostics.queue_summary.downloading
        ),
        format!("Queue Completed: {}", diagnostics.queue_summary.completed),
        format!("Queue Failed: {}", diagnostics.queue_summary.failed),
        format!(
            "Host Registration Status: {}",
            host_registration_status_id(diagnostics.host_registration.status)
        ),
        String::new(),
        "Host Registration Entries:".into(),
    ];

    for entry in &diagnostics.host_registration.entries {
        lines.push(format!("- {}", entry.browser));
        lines.push(format!("  Registry: {}", entry.registry_path));
        lines.push(format!(
            "  Manifest: {}",
            entry.manifest_path.as_deref().unwrap_or("missing")
        ));
        lines.push(format!("  Manifest Exists: {}", entry.manifest_exists));
        lines.push(format!(
            "  Host Binary: {}",
            entry.host_binary_path.as_deref().unwrap_or("missing")
        ));
        lines.push(format!(
            "  Host Binary Exists: {}",
            entry.host_binary_exists
        ));
    }

    lines.push(String::new());
    lines.push("Torrent Diagnostics:".into());
    if diagnostics.torrent_diagnostics.is_empty() {
        lines.push("- none".into());
    } else {
        for torrent in &diagnostics.torrent_diagnostics {
            let torrent_diagnostics = &torrent.diagnostics;
            lines.push(format!("- {} {}", torrent.job_id, torrent.filename));
            if let Some(info_hash) = &torrent.info_hash {
                lines.push(format!("  Info Hash: {info_hash}"));
            }
            lines.push(format!("  Live Peers: {}", torrent_diagnostics.live_peers));
            lines.push(format!("  Seen Peers: {}", torrent_diagnostics.seen_peers));
            lines.push(format!(
                "  Contributing Peers: {}",
                torrent_diagnostics.contributing_peers
            ));
            lines.push(format!(
                "  Peer Error Events: {}",
                torrent_diagnostics.peer_errors
            ));
            lines.push(format!(
                "  Peers With Errors: {}",
                torrent_diagnostics.peers_with_errors
            ));
            lines.push(format!(
                "  Peer Connection Attempts: {}",
                torrent_diagnostics.peer_connection_attempts
            ));
            lines.push(format!(
                "  Queued Peers: {}",
                torrent_diagnostics.queued_peers
            ));
            lines.push(format!(
                "  Connecting Peers: {}",
                torrent_diagnostics.connecting_peers
            ));
            lines.push(format!("  Dead Peers: {}", torrent_diagnostics.dead_peers));
            lines.push(format!(
                "  Not Needed Peers: {}",
                torrent_diagnostics.not_needed_peers
            ));
            lines.push(format!(
                "  Session Download Speed: {} B/s",
                torrent_diagnostics.session_download_speed
            ));
            lines.push(format!(
                "  Session Upload Speed: {} B/s",
                torrent_diagnostics.session_upload_speed
            ));
            if let Some(average) = torrent_diagnostics.average_piece_download_millis {
                lines.push(format!("  Average Piece Download: {average} ms"));
            }
            lines.push(format!(
                "  Listen Port: {}",
                format_torrent_listen_port(torrent_diagnostics)
            ));
            if !torrent_diagnostics.peer_samples.is_empty() {
                lines.push("  Peer Samples:".into());
                for sample in &torrent_diagnostics.peer_samples {
                    lines.push(format!(
                        "  - {} fetched {} bytes, errors {}, pieces {}, attempts {}",
                        sample.state,
                        sample.fetched_bytes,
                        sample.errors,
                        sample.downloaded_pieces,
                        sample.connection_attempts
                    ));
                }
            }
        }
    }

    lines.push(String::new());
    lines.push("Recent Events:".into());
    if diagnostics.recent_events.is_empty() {
        lines.push("- none".into());
    } else {
        for event in &diagnostics.recent_events {
            let job = event
                .job_id
                .as_ref()
                .map(|id| format!(" {id}"))
                .unwrap_or_default();
            lines.push(format!(
                "- {} {} {}{} {}",
                format_diagnostic_report_timestamp(event.timestamp),
                diagnostic_level_id(event.level),
                event.category,
                job,
                event.message
            ));
        }
    }

    lines.join("\n")
}

fn last_host_contact_text(diagnostics: &DiagnosticsSnapshot) -> String {
    diagnostics
        .last_host_contact_seconds_ago
        .map(|seconds| format!("{seconds} seconds ago"))
        .unwrap_or_else(|| "Never".into())
}

fn queue_summary_text(diagnostics: &DiagnosticsSnapshot) -> String {
    format!(
        "{} total | {} active | {} needs attention",
        diagnostics.queue_summary.total,
        diagnostics.queue_summary.active,
        diagnostics.queue_summary.attention
    )
}

fn host_registration_entry_row(entry: &HostRegistrationEntry) -> HostRegistrationEntryRow {
    let (status_label, status_tone) = if entry.host_binary_exists {
        ("Ready", "success")
    } else if entry.manifest_path.is_some() {
        ("Broken", "warning")
    } else {
        ("Missing", "neutral")
    };
    HostRegistrationEntryRow {
        browser: entry.browser.clone(),
        registry_path: entry.registry_path.clone(),
        manifest_path: entry
            .manifest_path
            .clone()
            .unwrap_or_else(|| "Not registered".into()),
        host_binary_path: entry
            .host_binary_path
            .clone()
            .unwrap_or_else(|| "Missing from manifest".into()),
        manifest_exists: entry.manifest_exists,
        host_binary_exists: entry.host_binary_exists,
        status_label: status_label.into(),
        status_tone: status_tone.into(),
    }
}

fn diagnostic_event_row(event: &DiagnosticEvent) -> DiagnosticEventRow {
    DiagnosticEventRow {
        level: diagnostic_level_id(event.level).into(),
        level_tone: diagnostic_level_tone(event.level).into(),
        timestamp_text: format_diagnostic_event_timestamp(event.timestamp),
        message: event.message.clone(),
        category: event.category.clone(),
        job_id: event.job_id.clone().unwrap_or_default(),
    }
}

fn connection_state_id(state: ConnectionState) -> &'static str {
    match state {
        ConnectionState::Checking => "checking",
        ConnectionState::Connected => "connected",
        ConnectionState::HostMissing => "host_missing",
        ConnectionState::AppMissing => "app_missing",
        ConnectionState::AppUnreachable => "app_unreachable",
        ConnectionState::Error => "error",
    }
}

fn host_registration_status_id(status: HostRegistrationStatus) -> &'static str {
    match status {
        HostRegistrationStatus::Configured => "configured",
        HostRegistrationStatus::Missing => "missing",
        HostRegistrationStatus::Broken => "broken",
    }
}

fn diagnostic_level_id(level: DiagnosticLevel) -> &'static str {
    match level {
        DiagnosticLevel::Info => "info",
        DiagnosticLevel::Warning => "warning",
        DiagnosticLevel::Error => "error",
    }
}

fn diagnostic_level_tone(level: DiagnosticLevel) -> &'static str {
    match level {
        DiagnosticLevel::Info => "success",
        DiagnosticLevel::Warning => "warning",
        DiagnosticLevel::Error => "error",
    }
}

fn format_diagnostic_event_timestamp(timestamp: u64) -> String {
    if timestamp == 0 {
        "Unknown time".into()
    } else {
        format_diagnostic_report_timestamp(timestamp)
    }
}

fn format_diagnostic_report_timestamp(timestamp: u64) -> String {
    if timestamp == 0 {
        return "unknown-time".into();
    }
    format_unix_millis_iso(timestamp)
}

fn format_unix_millis_iso(timestamp: u64) -> String {
    let seconds = timestamp / 1000;
    let millis = timestamp % 1000;
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as u32, d as u32)
}

fn format_torrent_listen_port(diagnostics: &TorrentRuntimeDiagnostics) -> String {
    match (diagnostics.listen_port, diagnostics.listener_fallback) {
        (Some(port), true) => format!("{port} (fallback active)"),
        (Some(port), false) => port.to_string(),
        (None, true) => "unavailable (fallback active)".into(),
        (None, false) => "unavailable".into(),
    }
}

pub fn default_torrent_download_directory(download_directory: &str) -> String {
    let trimmed = download_directory.trim().trim_end_matches(['\\', '/']);
    if trimmed.is_empty() {
        return String::new();
    }
    let separator = if trimmed.contains('\\') { '\\' } else { '/' };
    format!("{trimmed}{separator}Torrent")
}

pub fn normalize_torrent_settings(
    settings: TorrentSettings,
    download_directory: &str,
) -> TorrentSettings {
    TorrentSettings {
        enabled: settings.enabled,
        download_directory: if settings.download_directory.trim().is_empty() {
            default_torrent_download_directory(download_directory)
        } else {
            settings.download_directory.trim().into()
        },
        seed_mode: settings.seed_mode,
        seed_ratio_limit: clamp_f64(settings.seed_ratio_limit, 0.1, 100.0, 1.0),
        seed_time_limit_minutes: clamp_u32(settings.seed_time_limit_minutes, 1, 525_600, 60),
        upload_limit_kib_per_second: clamp_u32(
            settings.upload_limit_kib_per_second,
            0,
            1_048_576,
            0,
        ),
        port_forwarding_enabled: settings.port_forwarding_enabled,
        port_forwarding_port: normalize_forwarding_port(settings.port_forwarding_port),
        peer_connection_watchdog_mode: settings.peer_connection_watchdog_mode,
    }
}

pub fn should_stop_seeding(settings: &TorrentSettings, ratio: f64, elapsed_seconds: u64) -> bool {
    match settings.seed_mode {
        TorrentSeedMode::Forever => false,
        TorrentSeedMode::Ratio => ratio >= settings.seed_ratio_limit,
        TorrentSeedMode::Time => {
            elapsed_seconds >= u64::from(settings.seed_time_limit_minutes) * 60
        }
        TorrentSeedMode::RatioOrTime => {
            ratio >= settings.seed_ratio_limit
                || elapsed_seconds >= u64::from(settings.seed_time_limit_minutes) * 60
        }
    }
}

pub fn normalize_accent_color(value: &str) -> String {
    let trimmed = value.trim();
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if hex.len() == 6 && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        format!("#{}", hex.to_ascii_lowercase())
    } else {
        "#3b82f6".into()
    }
}

pub fn normalize_host_input(value: &str) -> String {
    let mut pattern = value.trim().to_string();
    let lower = pattern.to_ascii_lowercase();
    if lower.starts_with("http://") {
        pattern = pattern[7..].to_string();
    } else if lower.starts_with("https://") {
        pattern = pattern[8..].to_string();
    }

    if let Some(at_index) = pattern.find('@') {
        let slash_index = pattern.find('/').unwrap_or(usize::MAX);
        if at_index < slash_index {
            pattern = pattern[at_index + 1..].to_string();
        }
    }

    if let Some(cut_index) = pattern.find(['/', '?', '#']) {
        pattern.truncate(cut_index);
    }

    pattern = pattern.to_ascii_lowercase();

    if let Some((host, port)) = pattern.rsplit_once(':') {
        if !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) {
            pattern = host.into();
        }
    }

    let pattern = pattern.trim_matches('.').to_string();
    if pattern.is_empty()
        || pattern.contains('/')
        || pattern.contains('\\')
        || pattern.chars().any(char::is_whitespace)
        || !pattern.chars().any(|ch| ch.is_ascii_alphanumeric())
        || !pattern
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '*' | '-'))
    {
        String::new()
    } else {
        pattern
    }
}

pub fn parse_excluded_host_input(value: &str) -> Vec<String> {
    value
        .split(['\r', '\n', ','])
        .map(normalize_host_input)
        .filter(|host| !host.is_empty())
        .collect()
}

pub fn add_excluded_hosts(
    current_hosts: Vec<String>,
    candidates: Vec<String>,
) -> AddExcludedHostsResult {
    let mut hosts = current_hosts;
    let mut existing: BTreeSet<String> = hosts.iter().cloned().collect();
    let mut added_hosts = Vec::new();
    let mut duplicate_hosts = Vec::new();

    for candidate in candidates {
        let normalized = normalize_host_input(&candidate);
        if normalized.is_empty() {
            continue;
        }
        if existing.contains(&normalized) {
            if !duplicate_hosts.contains(&normalized) {
                duplicate_hosts.push(normalized);
            }
            continue;
        }
        existing.insert(normalized.clone());
        hosts.push(normalized.clone());
        added_hosts.push(normalized);
    }

    AddExcludedHostsResult {
        hosts,
        added_hosts,
        duplicate_hosts,
    }
}

pub fn remove_excluded_host(hosts: &[String], host: &str) -> Vec<String> {
    hosts
        .iter()
        .filter(|candidate| candidate.as_str() != host)
        .cloned()
        .collect()
}

pub fn filter_excluded_hosts(hosts: &[String], query: &str) -> Vec<String> {
    let normalized_query = query.trim().to_ascii_lowercase();
    if normalized_query.is_empty() {
        return hosts.to_vec();
    }
    hosts
        .iter()
        .filter(|host| host.contains(&normalized_query))
        .cloned()
        .collect()
}

pub fn format_excluded_sites_summary(hosts: &[String]) -> String {
    match hosts.len() {
        0 => "No excluded sites".into(),
        1 => "1 excluded site".into(),
        count => format!("{count} excluded sites"),
    }
}

pub fn queue_view_model_from_snapshot(
    snapshot: &DesktopSnapshot,
    state: &QueueUiState,
) -> QueueViewModel {
    let counts = get_queue_counts(&snapshot.jobs);
    let mut state = state.clone();
    let mut jobs = filter_jobs_for_view(&snapshot.jobs, state.view, &state.search_query);
    jobs.sort_by(|a, b| compare_downloads_for_sort(a, b, state.sort_mode));
    let visible_ids: Vec<String> = jobs.iter().map(|job| job.id.clone()).collect();
    state.selection.prune_to_visible(&visible_ids);
    let rows: Vec<JobRow> = jobs
        .iter()
        .map(|job| job_row_from_job_with_selection(job, state.selection.contains(&job.id)))
        .collect();
    let selected_count = state.selection.len();
    let all_visible_selected = !visible_ids.is_empty()
        && visible_ids
            .iter()
            .all(|id| state.selection.selected_ids.contains(id));
    let has_visible_selection = visible_ids
        .iter()
        .any(|id| state.selection.selected_ids.contains(id));
    let footer_text = footer_text_for_view(state.view, &snapshot.jobs);

    QueueViewModel {
        rows,
        nav_items: queue_nav_items(&counts, state.view),
        counts,
        title: state.view.label().into(),
        subtitle: queue_subtitle(visible_ids.len(), &state.search_query),
        footer_text,
        empty_text: empty_text_for_view(state.view, &state.search_query),
        selected_count,
        all_visible_selected,
        has_visible_selection,
        sort_mode: state.sort_mode,
        view_id: state.view.id(),
        search_query: state.search_query,
    }
}

pub fn select_job_range(job_ids: &[String], anchor_id: &str, current_id: &str) -> Vec<String> {
    let Some(anchor_index) = job_ids.iter().position(|id| id == anchor_id) else {
        return Vec::new();
    };
    let Some(current_index) = job_ids.iter().position(|id| id == current_id) else {
        return Vec::new();
    };
    let start = anchor_index.min(current_index);
    let end = anchor_index.max(current_index);
    job_ids[start..=end].to_vec()
}

pub fn view_for_job(job: &DownloadJob) -> ViewFilter {
    if job.transfer_kind == TransferKind::Torrent {
        ViewFilter::Torrents
    } else {
        ViewFilter::All
    }
}

pub fn slint_nav_item_from_item(item: QueueNavItem) -> crate::QueueNavItem {
    crate::QueueNavItem {
        id: item.id.into(),
        label: item.label.into(),
        count: item.count as i32,
        active: item.active,
        section: item.section.into(),
    }
}

pub fn slint_settings_nav_item_from_item(item: SettingsNavItem) -> crate::SettingsNavItem {
    crate::SettingsNavItem {
        id: item.id.into(),
        label: item.label.into(),
        active: item.active,
    }
}

pub fn slint_host_registration_entry_from_row(
    row: HostRegistrationEntryRow,
) -> crate::HostRegistrationEntryRow {
    crate::HostRegistrationEntryRow {
        browser: row.browser.into(),
        registry_path: row.registry_path.into(),
        manifest_path: row.manifest_path.into(),
        host_binary_path: row.host_binary_path.into(),
        manifest_exists: row.manifest_exists,
        host_binary_exists: row.host_binary_exists,
        status_label: row.status_label.into(),
        status_tone: row.status_tone.into(),
    }
}

pub fn slint_diagnostic_event_from_row(row: DiagnosticEventRow) -> crate::DiagnosticEventRow {
    crate::DiagnosticEventRow {
        level: row.level.into(),
        level_tone: row.level_tone.into(),
        timestamp_text: row.timestamp_text.into(),
        message: row.message.into(),
        category: row.category.into(),
        job_id: row.job_id.into(),
    }
}

pub fn slint_torrent_diagnostic_from_row(row: TorrentDiagnosticRow) -> crate::TorrentDiagnosticRow {
    crate::TorrentDiagnosticRow {
        job_id: row.job_id.into(),
        filename: row.filename.into(),
        info_hash: row.info_hash.into(),
        summary: row.summary.into(),
    }
}

pub fn theme_from_id(id: &str) -> Option<Theme> {
    match id {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        "oled_dark" => Some(Theme::OledDark),
        "system" => Some(Theme::System),
        _ => None,
    }
}

pub fn theme_id(theme: &Theme) -> &'static str {
    match theme {
        Theme::Light => "light",
        Theme::Dark => "dark",
        Theme::OledDark => "oled_dark",
        Theme::System => "system",
    }
}

pub fn performance_mode_from_id(id: &str) -> Option<DownloadPerformanceMode> {
    match id {
        "stable" => Some(DownloadPerformanceMode::Stable),
        "balanced" => Some(DownloadPerformanceMode::Balanced),
        "fast" => Some(DownloadPerformanceMode::Fast),
        _ => None,
    }
}

pub fn performance_mode_id(mode: DownloadPerformanceMode) -> &'static str {
    match mode {
        DownloadPerformanceMode::Stable => "stable",
        DownloadPerformanceMode::Balanced => "balanced",
        DownloadPerformanceMode::Fast => "fast",
    }
}

pub fn startup_launch_mode_from_id(id: &str) -> Option<StartupLaunchMode> {
    match id {
        "open" => Some(StartupLaunchMode::Open),
        "tray" => Some(StartupLaunchMode::Tray),
        _ => None,
    }
}

pub fn startup_launch_mode_id(mode: StartupLaunchMode) -> &'static str {
    match mode {
        StartupLaunchMode::Open => "open",
        StartupLaunchMode::Tray => "tray",
    }
}

pub fn queue_row_size_from_id(id: &str) -> Option<QueueRowSize> {
    match id {
        "compact" => Some(QueueRowSize::Compact),
        "small" => Some(QueueRowSize::Small),
        "medium" => Some(QueueRowSize::Medium),
        "large" => Some(QueueRowSize::Large),
        "damn" => Some(QueueRowSize::Damn),
        _ => None,
    }
}

pub fn queue_row_size_id(size: QueueRowSize) -> &'static str {
    match size {
        QueueRowSize::Compact => "compact",
        QueueRowSize::Small => "small",
        QueueRowSize::Medium => "medium",
        QueueRowSize::Large => "large",
        QueueRowSize::Damn => "damn",
    }
}

pub fn handoff_mode_from_id(id: &str) -> Option<DownloadHandoffMode> {
    match id {
        "off" => Some(DownloadHandoffMode::Off),
        "ask" => Some(DownloadHandoffMode::Ask),
        "auto" => Some(DownloadHandoffMode::Auto),
        _ => None,
    }
}

pub fn handoff_mode_id(mode: DownloadHandoffMode) -> &'static str {
    match mode {
        DownloadHandoffMode::Off => "off",
        DownloadHandoffMode::Ask => "ask",
        DownloadHandoffMode::Auto => "auto",
    }
}

pub fn torrent_seed_mode_from_id(id: &str) -> Option<TorrentSeedMode> {
    match id {
        "forever" => Some(TorrentSeedMode::Forever),
        "ratio" => Some(TorrentSeedMode::Ratio),
        "time" => Some(TorrentSeedMode::Time),
        "ratio_or_time" => Some(TorrentSeedMode::RatioOrTime),
        _ => None,
    }
}

pub fn torrent_seed_mode_id(mode: TorrentSeedMode) -> &'static str {
    match mode {
        TorrentSeedMode::Forever => "forever",
        TorrentSeedMode::Ratio => "ratio",
        TorrentSeedMode::Time => "time",
        TorrentSeedMode::RatioOrTime => "ratio_or_time",
    }
}

pub fn torrent_peer_watchdog_mode_from_id(id: &str) -> Option<TorrentPeerConnectionWatchdogMode> {
    match id {
        "diagnose" => Some(TorrentPeerConnectionWatchdogMode::Diagnose),
        "experimental" => Some(TorrentPeerConnectionWatchdogMode::Experimental),
        _ => None,
    }
}

pub fn torrent_peer_watchdog_mode_id(mode: TorrentPeerConnectionWatchdogMode) -> &'static str {
    match mode {
        TorrentPeerConnectionWatchdogMode::Diagnose => "diagnose",
        TorrentPeerConnectionWatchdogMode::Experimental => "experimental",
    }
}

pub fn reset_prompt_interaction_state(
    state: &mut PromptWindowInteractionState,
    prompt: Option<&DownloadPrompt>,
) {
    *state = PromptWindowInteractionState::default();
    if let Some(prompt) = prompt {
        state.renamed_filename = prompt.filename.clone();
    }
}

pub fn category_folder_for_filename(filename: &str) -> &'static str {
    category_for_filename(filename).label()
}

pub fn prompt_details_from_prompt(prompt: &DownloadPrompt) -> PromptWindowDetails {
    prompt_details_from_prompt_with_state(prompt, &PromptWindowInteractionState::default())
}

pub fn prompt_details_from_prompt_with_state(
    prompt: &DownloadPrompt,
    state: &PromptWindowInteractionState,
) -> PromptWindowDetails {
    let duplicate_text = prompt
        .duplicate_reason
        .as_ref()
        .map(|reason| {
            let duplicate_name = prompt
                .duplicate_filename
                .as_deref()
                .unwrap_or(prompt.filename.as_str());
            format!("Duplicate {reason}: {duplicate_name}")
        })
        .unwrap_or_default();
    let is_duplicate = prompt.duplicate_job.is_some() || prompt.duplicate_path.is_some();
    let title = if is_duplicate || !duplicate_text.is_empty() {
        "Duplicate download detected"
    } else {
        "New download detected"
    };
    let duplicate_label = prompt_duplicate_label(prompt);
    let duplicate_message = if prompt.duplicate_job.is_some() {
        "Already in queue: "
    } else if prompt.duplicate_path.is_some() {
        "Destination exists: "
    } else {
        ""
    };
    let overwrite_label = if prompt.duplicate_job.is_some() {
        "replace queue"
    } else {
        "replace file"
    };
    let destination = state
        .directory_override
        .as_deref()
        .map(|directory| prompt_destination_with_directory_override(directory, &prompt.filename))
        .unwrap_or_else(|| prompt.target_path.clone());

    PromptWindowDetails {
        id: prompt.id.clone(),
        title: title.into(),
        filename: prompt.filename.clone(),
        url: prompt.url.clone(),
        destination,
        size_text: prompt
            .total_bytes
            .map(format_bytes)
            .unwrap_or_else(|| "Unknown".into()),
        duplicate_text,
        is_duplicate,
        duplicate_label,
        duplicate_message: duplicate_message.into(),
        overwrite_label: overwrite_label.into(),
        source_label: prompt_source_label(prompt),
        can_swap_to_browser: prompt_can_swap_to_browser(prompt, is_duplicate),
        duplicate_menu_open: state.duplicate_menu_open,
        renaming_duplicate: state.renaming_duplicate,
        renamed_filename: state.renamed_filename.clone(),
        rename_can_confirm: !state.renamed_filename.trim().is_empty(),
        busy: state.busy,
        error_text: state.error_text.clone(),
    }
}

pub fn slint_prompt_details_from_prompt(prompt: &DownloadPrompt) -> crate::PromptDetails {
    slint_prompt_details_from_details(prompt_details_from_prompt(prompt))
}

pub fn slint_prompt_details_from_prompt_with_state(
    prompt: &DownloadPrompt,
    state: &PromptWindowInteractionState,
) -> crate::PromptDetails {
    slint_prompt_details_from_details(prompt_details_from_prompt_with_state(prompt, state))
}

pub fn prompt_confirm_request(
    prompt: &DownloadPrompt,
    state: &PromptWindowInteractionState,
    action: PromptConfirmAction,
) -> Result<ConfirmPromptRequest, String> {
    let (duplicate_action, renamed_filename) = match action {
        PromptConfirmAction::DefaultDownload => (PromptDuplicateAction::ReturnExisting, None),
        PromptConfirmAction::DownloadAnyway => (PromptDuplicateAction::DownloadAnyway, None),
        PromptConfirmAction::Overwrite => (PromptDuplicateAction::Overwrite, None),
        PromptConfirmAction::Rename => {
            let renamed = state.renamed_filename.trim();
            if renamed.is_empty() {
                return Err("Enter a file name before renaming the duplicate download.".into());
            }
            (PromptDuplicateAction::Rename, Some(renamed.into()))
        }
    };

    Ok(ConfirmPromptRequest {
        id: prompt.id.clone(),
        directory_override: state.directory_override.clone(),
        duplicate_action,
        renamed_filename,
    })
}

pub fn waiting_prompt_details() -> crate::PromptDetails {
    slint_prompt_details_from_details(PromptWindowDetails {
        id: String::new(),
        title: "Download prompt".into(),
        filename: "Waiting for a download...".into(),
        url: String::new(),
        destination: String::new(),
        size_text: "Unknown".into(),
        duplicate_text: String::new(),
        is_duplicate: false,
        duplicate_label: String::new(),
        duplicate_message: String::new(),
        overwrite_label: "replace file".into(),
        source_label: String::new(),
        can_swap_to_browser: false,
        duplicate_menu_open: false,
        renaming_duplicate: false,
        renamed_filename: String::new(),
        rename_can_confirm: false,
        busy: false,
        error_text: String::new(),
    })
}

pub fn progress_details_from_job(job: &DownloadJob, title: &str) -> ProgressWindowDetails {
    let metrics = download_progress_metrics(job, &[], unix_timestamp_millis() as u64);
    progress_details_from_job_with_state(
        job,
        title,
        &metrics,
        &ProgressPopupInteractionState::default(),
    )
}

pub fn progress_details_from_job_with_state(
    job: &DownloadJob,
    title: &str,
    metrics: &DownloadProgressMetrics,
    interaction: &ProgressPopupInteractionState,
) -> ProgressWindowDetails {
    let is_torrent = job.transfer_kind == TransferKind::Torrent;
    let state = progress_state_label(job);
    let (progress_label, bytes_text) = if is_torrent {
        torrent_progress_strip_text(job)
    } else {
        (
            format!("{:.0}%", clamp_progress(job.progress)),
            bytes_text(job.downloaded_bytes, job.total_bytes),
        )
    };
    let destination = if job.target_path.trim().is_empty() {
        "No destination recorded yet.".into()
    } else {
        job.target_path.clone()
    };
    let speed_text = if job.state == JobState::Downloading && metrics.average_speed > 0 {
        format!("{}/s", format_bytes(metrics.average_speed))
    } else {
        "--".into()
    };
    let eta_text = if job.state == JobState::Downloading && metrics.time_remaining > 0 {
        format_duration(metrics.time_remaining)
    } else {
        "--".into()
    };
    let size_text = if job.total_bytes > 0 {
        format_bytes(job.total_bytes)
    } else {
        "Unknown".into()
    };
    let completed_or_seeding_with_path =
        matches!(job.state, JobState::Completed | JobState::Seeding)
            && !job.target_path.trim().is_empty();

    ProgressWindowDetails {
        id: job.id.clone(),
        title: title.into(),
        filename: if is_torrent {
            torrent_display_name(job)
        } else {
            job.filename.clone()
        },
        state,
        bytes_text,
        progress: clamp_progress(job.progress),
        error_text: job.error.clone().unwrap_or_default(),
        subtitle: if is_torrent {
            torrent_subtitle(job)
        } else {
            host_from_url(&job.url)
        },
        destination,
        status_tone: progress_status_tone(job).into(),
        progress_label,
        speed_text,
        eta_text,
        size_text,
        source_summary: if is_torrent {
            torrent_source_summary(job)
        } else {
            job.url.clone()
        },
        info_hash: if is_torrent {
            torrent_info_hash(job)
        } else {
            String::new()
        },
        remaining_text: if is_torrent {
            torrent_remaining_text(job)
        } else {
            String::new()
        },
        upload_speed_text: if is_torrent {
            format_speed(torrent_upload_speed(job))
        } else {
            String::new()
        },
        peers_text: optional_u32_text(job.torrent.as_ref().and_then(|torrent| torrent.peers)),
        seeds_text: optional_u32_text(job.torrent.as_ref().and_then(|torrent| torrent.seeds)),
        ratio_text: job
            .torrent
            .as_ref()
            .map(|torrent| format!("{:.2}", torrent.ratio))
            .unwrap_or_else(|| "--".into()),
        files_text: if is_torrent {
            torrent_files_text(job)
        } else {
            String::new()
        },
        peer_health_tones: if is_torrent {
            torrent_peer_health_dots(job)
        } else {
            Vec::new()
        },
        can_open: job.state == JobState::Completed,
        can_reveal: completed_or_seeding_with_path,
        can_pause: matches!(
            job.state,
            JobState::Queued | JobState::Starting | JobState::Downloading | JobState::Seeding
        ),
        can_resume: job.state == JobState::Paused,
        can_retry: job.state == JobState::Failed,
        can_cancel: matches!(
            job.state,
            JobState::Queued | JobState::Starting | JobState::Downloading | JobState::Paused
        ),
        can_swap_to_browser: can_swap_failed_download_to_browser(job),
        can_close: matches!(
            job.state,
            JobState::Completed | JobState::Failed | JobState::Canceled
        ),
        busy: interaction.busy,
        cancel_confirming: interaction.cancel_confirming,
        action_error_text: if interaction.error_text.is_empty() {
            job.error.clone().unwrap_or_default()
        } else {
            interaction.error_text.clone()
        },
    }
}

pub fn slint_progress_details_from_job(job: &DownloadJob, title: &str) -> crate::ProgressDetails {
    slint_progress_details_from_details(progress_details_from_job(job, title))
}

pub fn slint_progress_details_from_job_with_state(
    job: &DownloadJob,
    title: &str,
    metrics: &DownloadProgressMetrics,
    interaction: &ProgressPopupInteractionState,
) -> crate::ProgressDetails {
    slint_progress_details_from_details(progress_details_from_job_with_state(
        job,
        title,
        metrics,
        interaction,
    ))
}

pub fn empty_progress_details(id: &str, title: &str) -> crate::ProgressDetails {
    slint_progress_details_from_details(ProgressWindowDetails {
        id: id.into(),
        title: title.into(),
        filename: id.into(),
        state: "Unknown".into(),
        bytes_text: "0 B / Unknown".into(),
        progress: 0.0,
        error_text: String::new(),
        subtitle: String::new(),
        destination: String::new(),
        status_tone: "neutral".into(),
        progress_label: "0%".into(),
        speed_text: "--".into(),
        eta_text: "--".into(),
        size_text: "Unknown".into(),
        source_summary: String::new(),
        info_hash: String::new(),
        remaining_text: String::new(),
        upload_speed_text: "--".into(),
        peers_text: "--".into(),
        seeds_text: "--".into(),
        ratio_text: "--".into(),
        files_text: String::new(),
        peer_health_tones: Vec::new(),
        can_open: false,
        can_reveal: false,
        can_pause: false,
        can_resume: false,
        can_retry: false,
        can_cancel: false,
        can_swap_to_browser: false,
        can_close: true,
        busy: false,
        cancel_confirming: false,
        action_error_text: String::new(),
    })
}

pub fn batch_details_from_context(
    context: &ProgressBatchContext,
    snapshot: &DesktopSnapshot,
) -> BatchProgressDetails {
    batch_details_from_context_with_state(
        context,
        snapshot,
        &ProgressPopupInteractionState::default(),
    )
}

pub fn batch_details_from_context_with_state(
    context: &ProgressBatchContext,
    snapshot: &DesktopSnapshot,
    interaction: &ProgressPopupInteractionState,
) -> BatchProgressDetails {
    let jobs: Vec<&DownloadJob> = context
        .job_ids
        .iter()
        .filter_map(|id| snapshot.jobs.iter().find(|job| job.id == *id))
        .collect();
    let completed = jobs
        .iter()
        .filter(|job| job.state == JobState::Completed)
        .count();
    let total = context.job_ids.len();
    let downloaded_bytes: u64 = jobs.iter().map(|job| job.downloaded_bytes).sum();
    let total_bytes: u64 = jobs.iter().map(|job| job.total_bytes).sum();
    let progress = if total_bytes > 0 {
        (downloaded_bytes as f64 / total_bytes as f64) * 100.0
    } else if total > 0 {
        jobs.iter().map(|job| job.progress).sum::<f64>() / total as f64
    } else {
        0.0
    };
    let failed = jobs
        .iter()
        .filter(|job| job.state == JobState::Failed)
        .count();
    let active = jobs.iter().filter(|job| batch_job_is_active(job)).count();
    let rows = jobs
        .iter()
        .map(|job| batch_progress_row(job))
        .collect::<Vec<_>>();
    let archive = jobs.iter().find_map(|job| job.bulk_archive.as_ref());
    let (phase_id, phase_label, phase_tone, archive_error) =
        batch_phase_details(context, archive, completed, failed, active, total);
    let reveal_completed_job_id = jobs
        .iter()
        .find(|job| job.state == JobState::Completed && !job.target_path.trim().is_empty())
        .map(|job| job.id.clone())
        .unwrap_or_default();

    BatchProgressDetails {
        batch_id: context.batch_id.clone(),
        title: context.title.clone(),
        display_title: context
            .archive_name
            .clone()
            .or_else(|| archive.map(|archive| archive.name.clone()))
            .unwrap_or_else(|| context.title.clone()),
        summary: format!("{completed} of {total} completed"),
        bytes_text: bytes_text(downloaded_bytes, total_bytes),
        progress: clamp_progress(progress),
        completed_count: completed,
        failed_count: failed,
        active_count: active,
        total_count: total,
        phase_id,
        phase_label,
        phase_tone,
        archive_error,
        rows,
        can_pause: jobs.iter().any(|job| batch_job_can_pause(job)),
        can_resume: jobs.iter().any(|job| batch_job_can_resume(job)),
        can_cancel: jobs.iter().any(|job| batch_job_can_cancel(job)),
        can_reveal_completed: !reveal_completed_job_id.is_empty(),
        reveal_completed_job_id,
        can_close: active == 0,
        busy: interaction.busy,
        action_error_text: interaction.error_text.clone(),
    }
}

pub fn slint_batch_details_from_context(
    context: &ProgressBatchContext,
    snapshot: &DesktopSnapshot,
) -> crate::BatchDetails {
    slint_batch_details_from_details(batch_details_from_context(context, snapshot))
}

pub fn slint_batch_details_from_context_with_state(
    context: &ProgressBatchContext,
    snapshot: &DesktopSnapshot,
    interaction: &ProgressPopupInteractionState,
) -> crate::BatchDetails {
    slint_batch_details_from_details(batch_details_from_context_with_state(
        context,
        snapshot,
        interaction,
    ))
}

pub fn empty_batch_details(batch_id: &str) -> crate::BatchDetails {
    slint_batch_details_from_details(BatchProgressDetails {
        batch_id: batch_id.into(),
        title: "Batch progress".into(),
        display_title: "Batch progress".into(),
        summary: "Waiting for batch details".into(),
        bytes_text: "0 B / Unknown".into(),
        progress: 0.0,
        completed_count: 0,
        failed_count: 0,
        active_count: 0,
        total_count: 0,
        phase_id: "waiting".into(),
        phase_label: "Waiting for jobs".into(),
        phase_tone: "neutral".into(),
        archive_error: String::new(),
        rows: Vec::new(),
        can_pause: false,
        can_resume: false,
        can_cancel: false,
        can_reveal_completed: false,
        reveal_completed_job_id: String::new(),
        can_close: true,
        busy: false,
        action_error_text: String::new(),
    })
}

fn slint_prompt_details_from_details(details: PromptWindowDetails) -> crate::PromptDetails {
    crate::PromptDetails {
        id: details.id.into(),
        title: details.title.into(),
        filename: details.filename.into(),
        url: details.url.into(),
        destination: details.destination.into(),
        size_text: details.size_text.into(),
        duplicate_text: details.duplicate_text.into(),
        is_duplicate: details.is_duplicate,
        duplicate_label: details.duplicate_label.into(),
        duplicate_message: details.duplicate_message.into(),
        overwrite_label: details.overwrite_label.into(),
        source_label: details.source_label.into(),
        can_swap_to_browser: details.can_swap_to_browser,
        duplicate_menu_open: details.duplicate_menu_open,
        renaming_duplicate: details.renaming_duplicate,
        renamed_filename: details.renamed_filename.into(),
        rename_can_confirm: details.rename_can_confirm,
        busy: details.busy,
        error_text: details.error_text.into(),
    }
}

fn slint_progress_details_from_details(details: ProgressWindowDetails) -> crate::ProgressDetails {
    crate::ProgressDetails {
        id: details.id.into(),
        title: details.title.into(),
        filename: details.filename.into(),
        state: details.state.into(),
        bytes_text: details.bytes_text.into(),
        progress: details.progress as f32,
        error_text: details.error_text.into(),
        subtitle: details.subtitle.into(),
        destination: details.destination.into(),
        status_tone: details.status_tone.into(),
        progress_label: details.progress_label.into(),
        speed_text: details.speed_text.into(),
        eta_text: details.eta_text.into(),
        size_text: details.size_text.into(),
        source_summary: details.source_summary.into(),
        info_hash: details.info_hash.into(),
        remaining_text: details.remaining_text.into(),
        upload_speed_text: details.upload_speed_text.into(),
        peers_text: details.peers_text.into(),
        seeds_text: details.seeds_text.into(),
        ratio_text: details.ratio_text.into(),
        files_text: details.files_text.into(),
        peer_health_tones: ModelRc::from(Rc::new(VecModel::from(
            details
                .peer_health_tones
                .into_iter()
                .map(SharedString::from)
                .collect::<Vec<_>>(),
        ))),
        can_open: details.can_open,
        can_reveal: details.can_reveal,
        can_pause: details.can_pause,
        can_resume: details.can_resume,
        can_retry: details.can_retry,
        can_cancel: details.can_cancel,
        can_swap_to_browser: details.can_swap_to_browser,
        can_close: details.can_close,
        busy: details.busy,
        cancel_confirming: details.cancel_confirming,
        action_error_text: details.action_error_text.into(),
    }
}

fn slint_batch_details_from_details(details: BatchProgressDetails) -> crate::BatchDetails {
    crate::BatchDetails {
        batch_id: details.batch_id.into(),
        title: details.title.into(),
        display_title: details.display_title.into(),
        summary: details.summary.into(),
        bytes_text: details.bytes_text.into(),
        progress: details.progress as f32,
        completed_count: details.completed_count as i32,
        failed_count: details.failed_count as i32,
        active_count: details.active_count as i32,
        total_count: details.total_count as i32,
        phase_id: details.phase_id.into(),
        phase_label: details.phase_label.into(),
        phase_tone: details.phase_tone.into(),
        archive_error: details.archive_error.into(),
        rows: ModelRc::from(Rc::new(VecModel::from(
            details
                .rows
                .into_iter()
                .map(|row| crate::BatchProgressJobRow {
                    id: row.id.into(),
                    filename: row.filename.into(),
                    subtitle: row.subtitle.into(),
                    status_text: row.status_text.into(),
                    status_tone: row.status_tone.into(),
                    progress: row.progress as f32,
                    progress_label: row.progress_label.into(),
                    speed_text: row.speed_text.into(),
                    size_text: row.size_text.into(),
                })
                .collect::<Vec<_>>(),
        ))),
        can_pause: details.can_pause,
        can_resume: details.can_resume,
        can_cancel: details.can_cancel,
        can_reveal_completed: details.can_reveal_completed,
        reveal_completed_job_id: details.reveal_completed_job_id.into(),
        can_close: details.can_close,
        busy: details.busy,
        action_error_text: details.action_error_text.into(),
    }
}

pub fn toast_message(
    toast_type: ToastType,
    title: impl Into<String>,
    message: impl Into<String>,
) -> ToastMessage {
    ToastMessage::new(toast_type, title, message)
}

pub fn toast_for_shell_error(operation: &str, message: &str) -> ToastMessage {
    ToastMessage::new(
        ToastType::Error,
        "Shell Error",
        format!("{operation} failed: {message}"),
    )
}

pub fn external_use_auto_reseed_message(target: &str, retry_seconds: u64) -> String {
    format!(
        "Windows can use the {target} now. Simple Download Manager will try to resume seeding every {retry_seconds}s."
    )
}

pub fn slint_toast_from_message(message: ToastMessage) -> crate::ToastMessage {
    crate::ToastMessage {
        id: message.id.into(),
        toast_type: message.toast_type.id().into(),
        tone: message.toast_type.tone().into(),
        title: message.title.into(),
        message: message.message.into(),
        auto_close: message.auto_close,
    }
}

fn job_state_label(state: JobState) -> &'static str {
    match state {
        JobState::Queued => "Queued",
        JobState::Starting => "Starting",
        JobState::Downloading => "Downloading",
        JobState::Seeding => "Seeding",
        JobState::Paused => "Paused",
        JobState::Completed => "Completed",
        JobState::Failed => "Failed",
        JobState::Canceled => "Canceled",
    }
}

fn trimmed_single_url(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Vec::new()
    } else {
        vec![trimmed.into()]
    }
}

fn prompt_duplicate_label(prompt: &DownloadPrompt) -> String {
    prompt
        .duplicate_job
        .as_ref()
        .map(|job| job.filename.clone())
        .or_else(|| prompt.duplicate_filename.clone())
        .or_else(|| prompt.duplicate_path.clone())
        .unwrap_or_default()
}

fn prompt_source_label(prompt: &DownloadPrompt) -> String {
    prompt
        .source
        .as_ref()
        .map(|source| {
            let entry = source.entry_point.replace('_', " ");
            if source.browser.trim().is_empty() {
                entry
            } else {
                format!("{} {entry}", source.browser)
            }
        })
        .unwrap_or_else(|| "Browser download".into())
}

fn prompt_can_swap_to_browser(prompt: &DownloadPrompt, is_duplicate: bool) -> bool {
    !is_duplicate
        && prompt
            .source
            .as_ref()
            .is_some_and(|source| source.entry_point == "browser_download")
}

fn prompt_destination_with_directory_override(directory: &str, filename: &str) -> String {
    let base = directory.trim().trim_end_matches(['\\', '/']);
    if base.is_empty() {
        return filename.into();
    }
    format!(
        "{}/{}/{}",
        base,
        category_folder_for_filename(filename),
        filename
    )
}

fn download_count_label(count: usize) -> String {
    format!(
        "{} {}",
        count,
        if count == 1 { "Download" } else { "Downloads" }
    )
}

fn next_add_download_batch_id() -> String {
    static NEXT_BATCH_ID: AtomicU64 = AtomicU64::new(1);
    format!(
        "batch_{}_{}",
        unix_timestamp_millis(),
        NEXT_BATCH_ID.fetch_add(1, AtomicOrdering::Relaxed)
    )
}

fn unix_timestamp_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn clamp_f64(value: f64, min: f64, max: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.max(min).min(max)
    } else {
        fallback
    }
}

fn clamp_u32(value: u32, min: u32, max: u32, _fallback: u32) -> u32 {
    value.max(min).min(max)
}

fn normalize_forwarding_port(port: u32) -> u32 {
    if (1024..=65534).contains(&port) {
        port
    } else {
        42_000
    }
}

fn trim_float(value: f64) -> String {
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn get_queue_counts(jobs: &[DownloadJob]) -> QueueCounts {
    let mut counts = QueueCounts::default();

    for job in jobs {
        if is_torrent_download(job) {
            counts.torrents.all += 1;
            if is_active_download_state(job.state) {
                counts.torrents.active += 1;
            }
            if job.state == JobState::Seeding {
                counts.torrents.seeding += 1;
            }
            if job_needs_attention(job) {
                counts.torrents.attention += 1;
            }
            if job.state == JobState::Queued {
                counts.torrents.queued += 1;
            }
            if is_finished_state(job.state) {
                counts.torrents.completed += 1;
            }
            continue;
        }

        counts.all += 1;
        if is_active_download_state(job.state) {
            counts.active += 1;
        }
        if job_needs_attention(job) {
            counts.attention += 1;
        }
        if job.state == JobState::Queued {
            counts.queued += 1;
        }
        if is_finished_state(job.state) {
            counts.completed += 1;
        }
        counts
            .categories
            .increment(category_for_filename(&job.filename));
    }

    counts
}

fn filter_jobs_for_view<'a>(
    jobs: &'a [DownloadJob],
    view: ViewFilter,
    query: &str,
) -> Vec<&'a DownloadJob> {
    let normalized_query = query.trim().to_lowercase();
    jobs.iter()
        .filter(|job| {
            if let ViewFilter::Category(category) = view {
                return !is_torrent_download(job)
                    && category_for_filename(&job.filename) == category
                    && matches_search_query(job, &normalized_query);
            }

            if view.is_torrent() {
                if !is_torrent_download(job) {
                    return false;
                }
                if view == ViewFilter::TorrentActive && !is_active_download_state(job.state) {
                    return false;
                }
                if view == ViewFilter::TorrentSeeding && job.state != JobState::Seeding {
                    return false;
                }
                if view == ViewFilter::TorrentAttention && !job_needs_attention(job) {
                    return false;
                }
                if view == ViewFilter::TorrentQueued && job.state != JobState::Queued {
                    return false;
                }
                if view == ViewFilter::TorrentCompleted && !is_finished_state(job.state) {
                    return false;
                }
                return matches_search_query(job, &normalized_query);
            }

            if is_torrent_download(job) {
                return false;
            }
            if view == ViewFilter::Attention && !job_needs_attention(job) {
                return false;
            }
            if view == ViewFilter::Active && !is_active_download_state(job.state) {
                return false;
            }
            if view == ViewFilter::Queued && job.state != JobState::Queued {
                return false;
            }
            if view == ViewFilter::Completed && !is_finished_state(job.state) {
                return false;
            }
            matches_search_query(job, &normalized_query)
        })
        .collect()
}

fn compare_downloads_for_sort(a: &&DownloadJob, b: &&DownloadJob, sort_mode: SortMode) -> Ordering {
    match sort_mode.column {
        SortColumn::Name => compare_ordering(a.filename.cmp(&b.filename), sort_mode.direction),
        SortColumn::Size => compare_numbers(a.total_bytes, b.total_bytes, sort_mode.direction)
            .then_with(|| a.filename.cmp(&b.filename)),
        SortColumn::Date => {
            compare_created_at(a, b, sort_mode.direction).then_with(|| a.filename.cmp(&b.filename))
        }
    }
}

fn compare_ordering(ordering: Ordering, direction: SortDirection) -> Ordering {
    match direction {
        SortDirection::Asc => ordering,
        SortDirection::Desc => ordering.reverse(),
    }
}

fn compare_numbers(a: u64, b: u64, direction: SortDirection) -> Ordering {
    compare_ordering(a.cmp(&b), direction)
}

fn compare_created_at(a: &DownloadJob, b: &DownloadJob, direction: SortDirection) -> Ordering {
    match (created_at_rank(a), created_at_rank(b)) {
        (0, 0) => Ordering::Equal,
        (0, _) => Ordering::Greater,
        (_, 0) => Ordering::Less,
        (left, right) => compare_numbers(left, right, direction),
    }
}

fn created_at_rank(job: &DownloadJob) -> u64 {
    job.created_at
}

fn queue_nav_items(counts: &QueueCounts, active_view: ViewFilter) -> Vec<QueueNavItem> {
    let mut items = vec![
        nav_item(ViewFilter::All, "Queue", counts.all, active_view),
        nav_item(
            ViewFilter::Attention,
            "Queue",
            counts.attention,
            active_view,
        ),
        nav_item(ViewFilter::Active, "Queue", counts.active, active_view),
        nav_item(ViewFilter::Queued, "Queue", counts.queued, active_view),
        nav_item(
            ViewFilter::Completed,
            "Queue",
            counts.completed,
            active_view,
        ),
    ];

    for category in DownloadCategory::ALL {
        items.push(nav_item(
            ViewFilter::Category(category),
            "Categories",
            counts.categories.get(category),
            active_view,
        ));
    }

    items.extend([
        nav_item(
            ViewFilter::Torrents,
            "Torrents",
            counts.torrents.all,
            active_view,
        ),
        nav_item(
            ViewFilter::TorrentActive,
            "Torrents",
            counts.torrents.active,
            active_view,
        ),
        nav_item(
            ViewFilter::TorrentSeeding,
            "Torrents",
            counts.torrents.seeding,
            active_view,
        ),
        nav_item(
            ViewFilter::TorrentAttention,
            "Torrents",
            counts.torrents.attention,
            active_view,
        ),
        nav_item(
            ViewFilter::TorrentQueued,
            "Torrents",
            counts.torrents.queued,
            active_view,
        ),
        nav_item(
            ViewFilter::TorrentCompleted,
            "Torrents",
            counts.torrents.completed,
            active_view,
        ),
    ]);

    items
}

fn nav_item(
    view: ViewFilter,
    section: &str,
    count: usize,
    active_view: ViewFilter,
) -> QueueNavItem {
    QueueNavItem {
        id: view.id(),
        label: view.label().into(),
        count,
        active: view == active_view,
        section: section.into(),
    }
}

fn queue_subtitle(visible_count: usize, query: &str) -> String {
    let suffix = if visible_count == 1 { "item" } else { "items" };
    if query.trim().is_empty() {
        format!("{visible_count} {suffix}")
    } else {
        format!("{visible_count} {suffix} matching \"{}\"", query.trim())
    }
}

fn footer_text_for_view(view: ViewFilter, jobs: &[DownloadJob]) -> String {
    if view.is_torrent() {
        let stats = torrent_footer_stats(jobs);
        return format!(
            "{} torrents | {} active | {} seeding | uploaded {} | ratio {:.2}",
            stats.all,
            stats.active,
            stats.seeding,
            format_bytes(stats.uploaded_bytes),
            stats.total_ratio
        );
    }

    let counts = get_queue_counts(jobs);
    format!(
        "{} downloads | {} active | {} need attention",
        counts.all, counts.active, counts.attention
    )
}

fn empty_text_for_view(view: ViewFilter, query: &str) -> String {
    if !query.trim().is_empty() {
        return "No downloads match the current search.".into();
    }
    if view.is_torrent() {
        "No torrent downloads in this view.".into()
    } else {
        "No downloads in this view.".into()
    }
}

fn torrent_footer_stats(jobs: &[DownloadJob]) -> TorrentFooterStats {
    let torrent_jobs: Vec<&DownloadJob> =
        jobs.iter().filter(|job| is_torrent_download(job)).collect();
    let uploaded_bytes = torrent_jobs
        .iter()
        .map(|job| {
            job.torrent
                .as_ref()
                .map(|torrent| torrent.uploaded_bytes)
                .unwrap_or(0)
        })
        .sum();
    let verified_bytes: u64 = torrent_jobs.iter().map(|job| job.downloaded_bytes).sum();

    TorrentFooterStats {
        all: torrent_jobs.len(),
        active: torrent_jobs
            .iter()
            .filter(|job| is_active_download_state(job.state))
            .count(),
        seeding: torrent_jobs
            .iter()
            .filter(|job| job.state == JobState::Seeding)
            .count(),
        uploaded_bytes,
        total_ratio: if verified_bytes > 0 {
            uploaded_bytes as f64 / verified_bytes as f64
        } else {
            0.0
        },
    }
}

fn category_for_filename(filename: &str) -> DownloadCategory {
    let basename = filename
        .trim()
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default();
    let Some((_, extension)) = basename.rsplit_once('.') else {
        return DownloadCategory::Other;
    };
    if extension.is_empty() || basename.starts_with('.') {
        return DownloadCategory::Other;
    }
    match extension.to_lowercase().as_str() {
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "rtf" | "csv" | "md"
        | "epub" => DownloadCategory::Document,
        "exe" | "msi" | "apk" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" => {
            DownloadCategory::Program
        }
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "tif" | "tiff" | "heic" => {
            DownloadCategory::Picture
        }
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "m4v" | "wmv" | "flv" => DownloadCategory::Video,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "tgz" => DownloadCategory::Compressed,
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "opus" | "wma" => DownloadCategory::Music,
        _ => DownloadCategory::Other,
    }
}

fn matches_search_query(job: &DownloadJob, normalized_query: &str) -> bool {
    if normalized_query.is_empty() {
        return true;
    }
    let archive_name = job
        .bulk_archive
        .as_ref()
        .map(|archive| archive.name.as_str())
        .unwrap_or_default();
    format!(
        "{} {} {} {}",
        job.filename, job.url, job.target_path, archive_name
    )
    .to_lowercase()
    .contains(normalized_query)
}

fn job_needs_attention(job: &DownloadJob) -> bool {
    if job.state == JobState::Failed || job.failure_category.is_some() {
        return true;
    }
    let is_unfinished = !is_finished_state(job.state);
    let has_partial_progress = job.downloaded_bytes > 0 || job.progress > 0.0;
    is_unfinished && has_partial_progress && job.resume_support == ResumeSupport::Unsupported
}

fn is_torrent_download(job: &DownloadJob) -> bool {
    job.transfer_kind == TransferKind::Torrent
}

fn is_active_download_state(state: JobState) -> bool {
    matches!(
        state,
        JobState::Starting | JobState::Downloading | JobState::Paused
    )
}

fn is_finished_state(state: JobState) -> bool {
    matches!(state, JobState::Completed | JobState::Canceled)
}

fn can_remove_download_immediately(job: &DownloadJob) -> bool {
    !matches!(
        job.state,
        JobState::Starting | JobState::Downloading | JobState::Seeding
    )
}

fn is_paused_seeding_torrent_delete_candidate(job: &DownloadJob) -> bool {
    job.transfer_kind == TransferKind::Torrent
        && job.state == JobState::Paused
        && job
            .torrent
            .as_ref()
            .and_then(|torrent| torrent.seeding_started_at)
            .is_some()
}

fn can_swap_failed_download_to_browser(job: &DownloadJob) -> bool {
    job.state == JobState::Failed
        && job.transfer_kind == TransferKind::Http
        && job
            .source
            .as_ref()
            .is_some_and(|source| source.entry_point == "browser_download")
        && is_http_url(&job.url)
}

fn is_http_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn can_show_progress_popup(job: &DownloadJob) -> bool {
    matches!(
        job.state,
        JobState::Queued | JobState::Starting | JobState::Downloading | JobState::Seeding
    )
}

fn transfer_kind_id(transfer_kind: TransferKind) -> &'static str {
    match transfer_kind {
        TransferKind::Http => "http",
        TransferKind::Torrent => "torrent",
    }
}

fn created_text(created_at: u64) -> String {
    if created_at == 0 {
        String::new()
    } else {
        format!("Created {created_at}")
    }
}

fn status_detail(job: &DownloadJob) -> String {
    if let Some(error) = &job.error {
        return error.clone();
    }
    if job.transfer_kind == TransferKind::Torrent {
        if let Some(torrent) = &job.torrent {
            if torrent.uploaded_bytes > 0 || torrent.ratio > 0.0 {
                return format!(
                    "Uploaded {} | ratio {:.2}",
                    format_bytes(torrent.uploaded_bytes),
                    torrent.ratio
                );
            }
        }
    }
    String::new()
}

fn status_tone(job: &DownloadJob) -> &'static str {
    if job_needs_attention(job) {
        "attention"
    } else if matches!(job.state, JobState::Completed | JobState::Seeding) {
        "success"
    } else if is_active_download_state(job.state) {
        "active"
    } else {
        "neutral"
    }
}

fn bytes_text(downloaded_bytes: u64, total_bytes: u64) -> String {
    if total_bytes == 0 {
        return format!("{} / Unknown", format_bytes(downloaded_bytes));
    }

    format!(
        "{} / {}",
        format_bytes(downloaded_bytes),
        format_bytes(total_bytes)
    )
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let value = bytes as f64;
    if value >= GIB {
        return format!("{:.1} GiB", value / GIB);
    }
    if value >= MIB {
        return format!("{:.1} MiB", value / MIB);
    }
    if value >= KIB {
        return format!("{:.1} KiB", value / KIB);
    }

    format!("{bytes} B")
}

const PROGRESS_SAMPLE_WINDOW_MS: u64 = 60_000;
const PROGRESS_MIN_SAMPLE_ELAPSED_MS: u64 = 1_000;

pub fn record_progress_sample(
    samples: Vec<ProgressSample>,
    job: &DownloadJob,
    timestamp: u64,
) -> Vec<ProgressSample> {
    let cutoff = timestamp.saturating_sub(PROGRESS_SAMPLE_WINDOW_MS);
    let mut retained: Vec<ProgressSample> = samples
        .into_iter()
        .filter(|sample| sample.job_id != job.id || sample.timestamp >= cutoff)
        .collect();

    if job.state == JobState::Downloading && job.downloaded_bytes > 0 {
        retained.retain(|sample| !(sample.job_id == job.id && sample.timestamp == timestamp));
        retained.push(ProgressSample {
            job_id: job.id.clone(),
            timestamp,
            downloaded_bytes: job.downloaded_bytes,
        });
    }

    retained
}

pub fn download_progress_metrics(
    job: &DownloadJob,
    samples: &[ProgressSample],
    now: u64,
) -> DownloadProgressMetrics {
    if job.transfer_kind == TransferKind::Torrent {
        let speed = torrent_download_speed(job);
        return DownloadProgressMetrics {
            average_speed: speed,
            time_remaining: time_remaining_from_speed(job, speed).unwrap_or(job.eta),
        };
    }

    let observed_speed = observed_average_speed(job, samples, now);
    let speed = observed_speed.unwrap_or(job.speed);
    DownloadProgressMetrics {
        average_speed: speed,
        time_remaining: time_remaining_from_speed(job, speed).unwrap_or(job.eta),
    }
}

fn observed_average_speed(job: &DownloadJob, samples: &[ProgressSample], now: u64) -> Option<u64> {
    let latest = samples
        .iter()
        .filter(|sample| sample.job_id == job.id)
        .max_by_key(|sample| sample.timestamp)?;
    let earliest = samples
        .iter()
        .filter(|sample| {
            sample.job_id == job.id
                && sample.timestamp <= latest.timestamp
                && latest.timestamp.saturating_sub(sample.timestamp)
                    >= PROGRESS_MIN_SAMPLE_ELAPSED_MS
        })
        .min_by_key(|sample| sample.timestamp)?;
    let elapsed_ms = latest.timestamp.saturating_sub(earliest.timestamp);
    if elapsed_ms < PROGRESS_MIN_SAMPLE_ELAPSED_MS
        || latest.downloaded_bytes <= earliest.downloaded_bytes
        || now.saturating_sub(latest.timestamp) > PROGRESS_SAMPLE_WINDOW_MS
    {
        return None;
    }
    let delta = latest.downloaded_bytes - earliest.downloaded_bytes;
    Some(((delta as f64 / elapsed_ms as f64) * 1000.0).round() as u64)
}

fn time_remaining_from_speed(job: &DownloadJob, speed: u64) -> Option<u64> {
    if speed == 0 || job.total_bytes == 0 || job.downloaded_bytes >= job.total_bytes {
        return None;
    }
    Some(
        job.total_bytes
            .saturating_sub(job.downloaded_bytes)
            .div_ceil(speed),
    )
}

fn clamp_progress(progress: f64) -> f64 {
    if progress.is_finite() {
        progress.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

fn progress_state_label(job: &DownloadJob) -> String {
    if job.transfer_kind == TransferKind::Torrent {
        if is_torrent_seeding_restore(job) {
            return "Restoring seeding".into();
        }
        if is_torrent_metadata_pending_for_progress(job) {
            return "Finding metadata".into();
        }
        if is_torrent_checking_files(job) {
            return "Checking files".into();
        }
    }
    job_state_label(job.state).into()
}

fn progress_status_tone(job: &DownloadJob) -> &'static str {
    match job.state {
        JobState::Completed | JobState::Seeding => "success",
        JobState::Failed => "error",
        JobState::Starting | JobState::Downloading => "active",
        JobState::Queued | JobState::Paused | JobState::Canceled => "neutral",
    }
}

fn host_from_url(url: &str) -> String {
    let without_scheme = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .trim_start_matches('/');
    let host = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .split('@')
        .next_back()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default();
    if host.is_empty() {
        url.into()
    } else {
        host.into()
    }
}

fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if hours < 48 {
        return format!("{hours}h");
    }
    format!("{}d", hours / 24)
}

fn format_speed(bytes_per_second: u64) -> String {
    if bytes_per_second == 0 {
        "--".into()
    } else {
        format!("{}/s", format_bytes(bytes_per_second))
    }
}

fn optional_u32_text(value: Option<u32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "--".into())
}

fn torrent_download_speed(job: &DownloadJob) -> u64 {
    job.torrent
        .as_ref()
        .and_then(|torrent| torrent.diagnostics.as_ref())
        .map(|diagnostics| diagnostics.session_download_speed)
        .filter(|speed| *speed > 0)
        .unwrap_or(job.speed)
}

fn torrent_upload_speed(job: &DownloadJob) -> u64 {
    job.torrent
        .as_ref()
        .and_then(|torrent| torrent.diagnostics.as_ref())
        .map(|diagnostics| diagnostics.session_upload_speed)
        .unwrap_or(0)
}

fn torrent_display_name(job: &DownloadJob) -> String {
    job.torrent
        .as_ref()
        .and_then(|torrent| torrent.name.as_ref())
        .filter(|name| !name.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| job.filename.clone())
}

fn torrent_subtitle(job: &DownloadJob) -> String {
    let summary = torrent_source_summary(job);
    if summary == "--" {
        job.url.clone()
    } else {
        summary
    }
}

pub fn torrent_source_summary(job: &DownloadJob) -> String {
    if !job.url.starts_with("magnet:") {
        return if job.url.trim().is_empty() {
            "--".into()
        } else {
            job.url.clone()
        };
    }

    let trackers = magnet_tracker_count(&job.url);
    match trackers {
        0 => "DHT".into(),
        1 => "DHT, 1 tracker".into(),
        count => format!("DHT, {count} trackers"),
    }
}

fn magnet_tracker_count(magnet: &str) -> usize {
    magnet
        .split(['?', '&'])
        .filter(|part| part.starts_with("tr="))
        .count()
}

pub fn torrent_info_hash(job: &DownloadJob) -> String {
    job.torrent
        .as_ref()
        .and_then(|torrent| torrent.info_hash.as_ref())
        .filter(|hash| !hash.trim().is_empty())
        .cloned()
        .or_else(|| magnet_info_hash(&job.url))
        .unwrap_or_else(|| "--".into())
}

fn magnet_info_hash(magnet: &str) -> Option<String> {
    magnet.split(['?', '&']).find_map(|part| {
        let value = part.strip_prefix("xt=urn:btih:")?;
        value
            .split(['&', '#'])
            .next()
            .filter(|hash| !hash.is_empty())
            .map(|hash| hash.to_ascii_lowercase())
    })
}

pub fn torrent_remaining_text(job: &DownloadJob) -> String {
    if job.total_bytes == 0 {
        return "Unknown remaining".into();
    }
    let remaining = job.total_bytes.saturating_sub(job.downloaded_bytes);
    if remaining == 0 {
        "Complete".into()
    } else {
        format!("{} remaining", format_bytes(remaining))
    }
}

pub fn torrent_peer_health_dots(job: &DownloadJob) -> Vec<String> {
    const DOT_COUNT: usize = 12;
    let Some(torrent) = job.torrent.as_ref() else {
        return vec!["muted".into(); DOT_COUNT];
    };
    let diagnostics = torrent.diagnostics.as_ref();

    let (live, errors) = if let Some(diagnostics) = diagnostics {
        if diagnostics.live_peers > 0
            || diagnostics.peer_errors > 0
            || diagnostics.peers_with_errors > 0
        {
            (
                diagnostics.live_peers.min(DOT_COUNT as u32) as usize,
                diagnostics
                    .peers_with_errors
                    .max(diagnostics.peer_errors)
                    .min(DOT_COUNT as u32) as usize,
            )
        } else {
            fallback_peer_health_counts(torrent.peers.unwrap_or_default())
        }
    } else {
        fallback_peer_health_counts(torrent.peers.unwrap_or_default())
    };
    let live = live.min(DOT_COUNT);
    let errors = errors.min(DOT_COUNT - live);
    let mut tones = Vec::with_capacity(DOT_COUNT);
    tones.extend(std::iter::repeat_n("success".into(), live));
    tones.extend(std::iter::repeat_n("warning".into(), errors));
    tones.extend(std::iter::repeat_n(
        "muted".into(),
        DOT_COUNT.saturating_sub(tones.len()),
    ));
    tones
}

fn fallback_peer_health_counts(peers: u32) -> (usize, usize) {
    if peers == 0 {
        return (0, 0);
    }
    let live = peers.min(6) as usize;
    let warnings = if peers > live as u32 { 2 } else { 0 };
    (live, warnings)
}

pub fn torrent_progress_strip_text(job: &DownloadJob) -> (String, String) {
    let progress = format!("{:.0}%", clamp_progress(job.progress));
    let Some(torrent) = &job.torrent else {
        return (progress, bytes_text(job.downloaded_bytes, job.total_bytes));
    };

    if torrent.fetched_bytes == 0 && job.downloaded_bytes > 0 && job.total_bytes > 0 {
        return (
            format!("Verified {progress}"),
            format!(
                "Verified {}",
                bytes_text(job.downloaded_bytes, job.total_bytes)
            ),
        );
    }

    (progress, bytes_text(job.downloaded_bytes, job.total_bytes))
}

fn torrent_files_text(job: &DownloadJob) -> String {
    let files = job
        .torrent
        .as_ref()
        .and_then(|torrent| torrent.total_files)
        .unwrap_or(0);
    let file_label = match files {
        0 => "Unknown files".into(),
        1 => "1 file".into(),
        count => format!("{count} files"),
    };
    if job.total_bytes > 0 {
        format!("{file_label} ({})", format_bytes(job.total_bytes))
    } else {
        file_label
    }
}

fn is_torrent_metadata_pending_for_progress(job: &DownloadJob) -> bool {
    job.transfer_kind == TransferKind::Torrent
        && matches!(
            job.state,
            JobState::Queued | JobState::Starting | JobState::Downloading
        )
        && job.total_bytes == 0
        && job
            .torrent
            .as_ref()
            .and_then(|torrent| torrent.info_hash.as_ref())
            .is_none()
}

fn is_torrent_checking_files(job: &DownloadJob) -> bool {
    job.transfer_kind == TransferKind::Torrent
        && job.state == JobState::Downloading
        && job.downloaded_bytes > 0
        && job
            .torrent
            .as_ref()
            .is_some_and(|torrent| torrent.fetched_bytes == 0)
}

fn is_torrent_seeding_restore(job: &DownloadJob) -> bool {
    job.transfer_kind == TransferKind::Torrent
        && job.state == JobState::Starting
        && job
            .torrent
            .as_ref()
            .and_then(|torrent| torrent.seeding_started_at)
            .is_some()
}

fn batch_job_is_active(job: &DownloadJob) -> bool {
    matches!(
        job.state,
        JobState::Queued | JobState::Starting | JobState::Downloading | JobState::Seeding
    )
}

fn batch_job_can_pause(job: &DownloadJob) -> bool {
    batch_job_is_active(job)
}

fn batch_job_can_resume(job: &DownloadJob) -> bool {
    matches!(
        job.state,
        JobState::Paused | JobState::Failed | JobState::Canceled
    )
}

fn batch_job_can_cancel(job: &DownloadJob) -> bool {
    matches!(
        job.state,
        JobState::Queued
            | JobState::Starting
            | JobState::Downloading
            | JobState::Seeding
            | JobState::Paused
    )
}

fn batch_progress_row(job: &DownloadJob) -> BatchProgressJobRow {
    let metrics = download_progress_metrics(job, &[], unix_timestamp_millis() as u64);
    BatchProgressJobRow {
        id: job.id.clone(),
        filename: torrent_display_name(job),
        subtitle: if job.transfer_kind == TransferKind::Torrent {
            torrent_source_summary(job)
        } else {
            host_from_url(&job.url)
        },
        status_text: progress_state_label(job),
        status_tone: progress_status_tone(job).into(),
        progress: clamp_progress(job.progress),
        progress_label: format!("{:.0}%", clamp_progress(job.progress)),
        speed_text: if job.state == JobState::Downloading && metrics.average_speed > 0 {
            format_speed(metrics.average_speed)
        } else {
            "--".into()
        },
        size_text: if job.total_bytes > 0 {
            format_bytes(job.total_bytes)
        } else {
            "Unknown".into()
        },
    }
}

fn batch_phase_details(
    context: &ProgressBatchContext,
    archive: Option<&simple_download_manager_desktop_core::storage::BulkArchiveInfo>,
    completed: usize,
    failed: usize,
    active: usize,
    total: usize,
) -> (String, String, String, String) {
    if context.kind == ProgressBatchKind::Bulk {
        if let Some(archive) = archive {
            return match archive.archive_status {
                BulkArchiveStatus::Pending => (
                    "downloading".into(),
                    "Downloading files".into(),
                    "active".into(),
                    String::new(),
                ),
                BulkArchiveStatus::Compressing => (
                    "compressing".into(),
                    "Compressing archive".into(),
                    "warning".into(),
                    String::new(),
                ),
                BulkArchiveStatus::Completed => (
                    "ready".into(),
                    "Archive ready".into(),
                    "success".into(),
                    String::new(),
                ),
                BulkArchiveStatus::Failed => (
                    "failed".into(),
                    "Archive failed".into(),
                    "error".into(),
                    archive.error.clone().unwrap_or_default(),
                ),
            };
        }
    }

    if failed > 0 && active == 0 {
        return (
            "failed".into(),
            "Some downloads failed".into(),
            "error".into(),
            String::new(),
        );
    }
    if total > 0 && completed == total {
        return (
            "ready".into(),
            "Downloads ready".into(),
            "success".into(),
            String::new(),
        );
    }
    (
        "downloading".into(),
        "Downloading files".into(),
        "active".into(),
        String::new(),
    )
}
