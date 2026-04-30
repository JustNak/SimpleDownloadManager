pub const TAURI_TRANSITION_FEED_NAME: &str = "latest-alpha.json";
pub const SLINT_UPDATER_FEED_NAME: &str = "latest-alpha-slint.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateFeedPlan {
    pub transition_feed_name: &'static str,
    pub slint_feed_name: &'static str,
}

impl Default for UpdateFeedPlan {
    fn default() -> Self {
        Self {
            transition_feed_name: TAURI_TRANSITION_FEED_NAME,
            slint_feed_name: SLINT_UPDATER_FEED_NAME,
        }
    }
}
