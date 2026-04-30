pub const APP_NAME: &str = "Simple Download Manager";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationRequest {
    pub app_name: String,
    pub title: String,
    pub body: String,
}

impl NotificationRequest {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            app_name: APP_NAME.into(),
            title: title.into(),
            body: body.into(),
        }
    }
}

#[cfg(windows)]
pub fn show_notification(title: &str, body: &str) -> Result<(), String> {
    let request = NotificationRequest::new(title, body);
    notify_rust::Notification::new()
        .appname(&request.app_name)
        .summary(&request.title)
        .body(&request.body)
        .show()
        .map(|_| ())
        .map_err(|error| format!("Could not show notification: {error}"))
}

#[cfg(not(windows))]
pub fn show_notification(_title: &str, _body: &str) -> Result<(), String> {
    Ok(())
}
