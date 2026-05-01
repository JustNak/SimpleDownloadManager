#[cfg(windows)]
pub fn write_text(text: &str) -> Result<(), String> {
    clipboard_win::set_clipboard_string(text)
        .map_err(|error| format!("Could not copy diagnostics report: {error}"))
}

#[cfg(not(windows))]
pub fn write_text(_text: &str) -> Result<(), String> {
    Err("Copying diagnostics to the clipboard is only supported on Windows in this build.".into())
}
