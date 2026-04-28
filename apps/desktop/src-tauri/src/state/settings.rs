use super::*;

pub(super) fn normalize_diagnostic_events(
    mut events: Vec<DiagnosticEvent>,
) -> Vec<DiagnosticEvent> {
    trim_diagnostic_events(&mut events);
    events
}

pub(super) fn trim_diagnostic_events(events: &mut Vec<DiagnosticEvent>) {
    if events.len() > DIAGNOSTIC_EVENT_LIMIT {
        let overflow = events.len() - DIAGNOSTIC_EVENT_LIMIT;
        events.drain(0..overflow);
    }
}

pub(super) fn normalize_extension_settings(settings: &mut ExtensionIntegrationSettings) {
    if settings.listen_port == 0 || settings.listen_port > u16::MAX as u32 {
        settings.listen_port = default_extension_listen_port();
    }

    settings.excluded_hosts = normalize_host_patterns(&settings.excluded_hosts);
    settings.authenticated_handoff_hosts =
        normalize_host_patterns(&settings.authenticated_handoff_hosts);

    let mut normalized_extensions = Vec::new();
    let mut seen_extensions = HashSet::new();

    for extension in &settings.ignored_file_extensions {
        for candidate in
            extension.split(|character: char| character == ',' || character.is_whitespace())
        {
            let candidate = normalize_file_extension(candidate);
            if candidate.is_empty() || !seen_extensions.insert(candidate.clone()) {
                continue;
            }

            normalized_extensions.push(candidate);
        }
    }

    settings.ignored_file_extensions = normalized_extensions;
}

pub(super) fn normalize_host_patterns(hosts: &[String]) -> Vec<String> {
    let mut normalized_hosts = Vec::new();
    let mut seen_hosts = HashSet::new();

    for host in hosts {
        let mut host = host.trim().to_ascii_lowercase();
        if let Some(stripped) = host.strip_prefix("http://") {
            host = stripped.to_string();
        } else if let Some(stripped) = host.strip_prefix("https://") {
            host = stripped.to_string();
        }
        let host = host
            .split('/')
            .next()
            .unwrap_or_default()
            .split('?')
            .next()
            .unwrap_or_default()
            .split('#')
            .next()
            .unwrap_or_default()
            .split(':')
            .next()
            .unwrap_or_default()
            .trim_matches('/')
            .trim_matches('.')
            .to_string();

        if host.is_empty()
            || host.contains('\\')
            || host.split_whitespace().count() > 1
            || !host
                .chars()
                .any(|character| character.is_ascii_alphanumeric())
            || !host.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '*' | '-')
            })
            || !seen_hosts.insert(host.clone())
        {
            continue;
        }

        normalized_hosts.push(host);
    }

    normalized_hosts
}

pub(super) fn normalize_torrent_settings(settings: &mut TorrentSettings) {
    if !settings.seed_ratio_limit.is_finite() || settings.seed_ratio_limit < 0.1 {
        settings.seed_ratio_limit = 0.1;
    }

    if settings.seed_time_limit_minutes == 0 {
        settings.seed_time_limit_minutes = 1;
    }

    settings.upload_limit_kib_per_second = settings
        .upload_limit_kib_per_second
        .min(MAX_TORRENT_UPLOAD_LIMIT_KIB_PER_SECOND);

    if settings.port_forwarding_port < MIN_TORRENT_FORWARDING_PORT
        || settings.port_forwarding_port > MAX_TORRENT_FORWARDING_PORT
    {
        settings.port_forwarding_port = default_torrent_port_forwarding_port();
    }
}

pub(super) fn normalize_accent_color(settings: &mut Settings) {
    let accent_color = settings.accent_color.trim();
    let is_hex_color = accent_color.len() == 7
        && accent_color.starts_with('#')
        && accent_color
            .chars()
            .skip(1)
            .all(|character| character.is_ascii_hexdigit());

    if is_hex_color {
        settings.accent_color = accent_color.to_ascii_lowercase();
    } else {
        settings.accent_color = "#3b82f6".into();
    }
}

pub(super) fn normalize_file_extension(value: &str) -> String {
    let extension = value.trim().trim_start_matches('.').to_ascii_lowercase();
    if extension.is_empty()
        || extension.contains('/')
        || extension.contains('\\')
        || extension.chars().all(|character| character == '.')
    {
        return String::new();
    }

    extension
}

pub(super) fn should_reset_download_directory(
    download_directory: &str,
    has_data_dir_override: bool,
    storage_exists: bool,
) -> bool {
    download_directory.trim().is_empty()
        || is_legacy_default_download_directory(download_directory)
        || (has_data_dir_override && !storage_exists)
}

pub(super) fn is_legacy_default_download_directory(download_directory: &str) -> bool {
    let normalized = download_directory
        .trim()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase();

    normalized == "c:/downloads" || normalized == "c:/users/you/downloads"
}

pub fn validate_settings(settings: &mut Settings) -> Result<(), String> {
    if settings.download_directory.trim().is_empty() {
        return Err("Download directory cannot be empty.".into());
    }

    normalize_accent_color(settings);
    normalize_extension_settings(&mut settings.extension_integration);
    normalize_torrent_settings(&mut settings.torrent);

    std::fs::create_dir_all(&settings.download_directory)
        .map_err(|error| format!("Could not create download directory: {error}"))?;
    ensure_download_category_directories(Path::new(&settings.download_directory))?;

    Ok(())
}

impl SharedState {
    pub async fn save_settings(&self, mut settings: Settings) -> Result<DesktopSnapshot, String> {
        validate_settings(&mut settings)?;

        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.settings = settings;
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub fn save_settings_sync(&self, mut settings: Settings) -> Result<(), String> {
        validate_settings(&mut settings)?;

        let persisted = {
            let mut state = self.inner.blocking_write();
            state.settings = settings;
            state.persisted()
        };

        persist_state(&self.storage_path, &persisted)
    }

    pub async fn settings(&self) -> Settings {
        let state = self.inner.read().await;
        state.settings.clone()
    }

    pub fn settings_sync(&self) -> Settings {
        let state = self.inner.blocking_read();
        state.settings.clone()
    }

    pub async fn main_window_state(&self) -> Option<MainWindowState> {
        let state = self.inner.read().await;
        state.main_window.clone()
    }

    pub fn main_window_state_sync(&self) -> Option<MainWindowState> {
        let state = self.inner.blocking_read();
        state.main_window.clone()
    }

    pub async fn save_main_window_state(&self, main_window: MainWindowState) -> Result<(), String> {
        let persisted = {
            let mut state = self.inner.write().await;
            state.main_window = Some(main_window);
            state.persisted()
        };

        persist_state(&self.storage_path, &persisted)
    }

    pub fn save_main_window_state_sync(&self, main_window: MainWindowState) -> Result<(), String> {
        let persisted = {
            let mut state = self.inner.blocking_write();
            state.main_window = Some(main_window);
            state.persisted()
        };

        persist_state(&self.storage_path, &persisted)
    }

    pub async fn save_extension_integration_settings(
        &self,
        mut extension_settings: ExtensionIntegrationSettings,
    ) -> Result<DesktopSnapshot, String> {
        normalize_extension_settings(&mut extension_settings);

        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.settings.extension_integration = extension_settings;
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }
}
