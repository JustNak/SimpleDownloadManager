use simple_download_manager_desktop_core::contracts::{
    AddJobResult, AddJobStatus, AddJobsResult, ProgressBatchContext, ProgressBatchKind,
};
use simple_download_manager_desktop_core::storage::{
    ConnectionState, DesktopSnapshot, DownloadJob, DownloadPrompt, JobState, ResumeSupport,
    TransferKind,
};
use std::cmp::Ordering;
use std::collections::BTreeSet;
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
    pub progress_intent: Option<AddDownloadProgressIntent>,
    pub view_id: &'static str,
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

#[derive(Debug, Clone, PartialEq)]
pub struct PromptWindowDetails {
    pub id: String,
    pub title: String,
    pub filename: String,
    pub url: String,
    pub destination: String,
    pub size_text: String,
    pub duplicate_text: String,
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatchProgressDetails {
    pub batch_id: String,
    pub title: String,
    pub summary: String,
    pub bytes_text: String,
    pub progress: f64,
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
            let progress_intent = if result.status == AddJobStatus::Queued {
                Some(AddDownloadProgressIntent::Single {
                    job_id: result.job_id.clone(),
                })
            } else {
                None
            };
            AddDownloadOutcome {
                primary_job_id: Some(result.job_id),
                progress_intent,
                view_id,
            }
        }
        AddDownloadResult::Batch(result) => {
            let queued_ids = result
                .results
                .iter()
                .filter(|item| item.status == AddJobStatus::Queued)
                .map(|item| item.job_id.clone())
                .collect::<Vec<_>>();
            let primary_job_id = result
                .results
                .iter()
                .find(|item| item.status == AddJobStatus::Queued)
                .or_else(|| result.results.first())
                .map(|item| item.job_id.clone());
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
                progress_intent,
                view_id,
            }
        }
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

pub fn prompt_details_from_prompt(prompt: &DownloadPrompt) -> PromptWindowDetails {
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
    let title = if prompt.duplicate_job.is_some()
        || prompt.duplicate_path.is_some()
        || !duplicate_text.is_empty()
    {
        "Duplicate download detected"
    } else {
        "New download detected"
    };

    PromptWindowDetails {
        id: prompt.id.clone(),
        title: title.into(),
        filename: prompt.filename.clone(),
        url: prompt.url.clone(),
        destination: prompt.target_path.clone(),
        size_text: prompt
            .total_bytes
            .map(format_bytes)
            .unwrap_or_else(|| "Unknown".into()),
        duplicate_text,
    }
}

pub fn slint_prompt_details_from_prompt(prompt: &DownloadPrompt) -> crate::PromptDetails {
    slint_prompt_details_from_details(prompt_details_from_prompt(prompt))
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
    })
}

pub fn progress_details_from_job(job: &DownloadJob, title: &str) -> ProgressWindowDetails {
    ProgressWindowDetails {
        id: job.id.clone(),
        title: title.into(),
        filename: job.filename.clone(),
        state: job_state_label(job.state).into(),
        bytes_text: bytes_text(job.downloaded_bytes, job.total_bytes),
        progress: job.progress,
        error_text: job.error.clone().unwrap_or_default(),
    }
}

pub fn slint_progress_details_from_job(job: &DownloadJob, title: &str) -> crate::ProgressDetails {
    slint_progress_details_from_details(progress_details_from_job(job, title))
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
    })
}

pub fn batch_details_from_context(
    context: &ProgressBatchContext,
    snapshot: &DesktopSnapshot,
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

    BatchProgressDetails {
        batch_id: context.batch_id.clone(),
        title: context.title.clone(),
        summary: format!("{completed} of {total} completed"),
        bytes_text: bytes_text(downloaded_bytes, total_bytes),
        progress,
    }
}

pub fn slint_batch_details_from_context(
    context: &ProgressBatchContext,
    snapshot: &DesktopSnapshot,
) -> crate::BatchDetails {
    slint_batch_details_from_details(batch_details_from_context(context, snapshot))
}

pub fn empty_batch_details(batch_id: &str) -> crate::BatchDetails {
    slint_batch_details_from_details(BatchProgressDetails {
        batch_id: batch_id.into(),
        title: "Batch progress".into(),
        summary: "Waiting for batch details".into(),
        bytes_text: "0 B / Unknown".into(),
        progress: 0.0,
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
    }
}

fn slint_batch_details_from_details(details: BatchProgressDetails) -> crate::BatchDetails {
    crate::BatchDetails {
        batch_id: details.batch_id.into(),
        title: details.title.into(),
        summary: details.summary.into(),
        bytes_text: details.bytes_text.into(),
        progress: details.progress as f32,
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
