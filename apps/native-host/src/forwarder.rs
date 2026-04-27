use crate::protocol::{AppRequestEnvelope, AppResponseEnvelope};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_PIPE_PATH: &str = r"\\.\pipe\myapp.downloads.v1";
const DEFAULT_APP_EXECUTABLE: &str = "simple-download-manager.exe";
const CONNECT_ATTEMPTS: usize = 10;
const CONNECT_DELAY: Duration = Duration::from_millis(300);
const APP_FORWARD_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_APP_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Debug)]
pub enum ForwarderError {
    AppNotInstalled,
    AppUnreachable,
    Serialization(String),
    Transport(String),
}

pub struct AppForwarder {
    pipe_path: String,
    desktop_path: PathBuf,
}

impl AppForwarder {
    pub fn from_environment() -> Self {
        let pipe_path =
            std::env::var("MYAPP_PIPE_PATH").unwrap_or_else(|_| DEFAULT_PIPE_PATH.to_string());
        let desktop_path = resolve_desktop_path();

        Self {
            pipe_path,
            desktop_path,
        }
    }

    pub fn launch_app(&self) -> Result<(), ForwarderError> {
        if !self.desktop_path.exists() {
            return Err(ForwarderError::AppNotInstalled);
        }

        Command::new(&self.desktop_path)
            .spawn()
            .map(|_| ())
            .map_err(|error| {
                ForwarderError::Transport(format!("Could not launch desktop app: {error}"))
            })
    }

    pub fn send<T>(
        &self,
        request: &AppRequestEnvelope<T>,
    ) -> Result<AppResponseEnvelope, ForwarderError>
    where
        T: Serialize,
    {
        let deadline = Instant::now() + APP_FORWARD_TIMEOUT;
        match self.send_once_before_deadline(request, deadline) {
            Ok(response) => Ok(response),
            Err(ForwarderError::AppUnreachable) => {
                self.launch_app()?;

                for _ in 0..CONNECT_ATTEMPTS {
                    let Some(remaining) = remaining_timeout(deadline) else {
                        return Err(ForwarderError::AppUnreachable);
                    };
                    thread::sleep(remaining.min(CONNECT_DELAY));
                    if let Ok(response) = self.send_once_before_deadline(request, deadline) {
                        return Ok(response);
                    }
                }

                Err(ForwarderError::AppUnreachable)
            }
            Err(error) => Err(error),
        }
    }

    fn send_once_before_deadline<T>(
        &self,
        request: &AppRequestEnvelope<T>,
        deadline: Instant,
    ) -> Result<AppResponseEnvelope, ForwarderError>
    where
        T: Serialize,
    {
        let request_json = serde_json::to_string(request).map_err(|error| {
            ForwarderError::Serialization(format!("Could not serialize app request: {error}"))
        })?;

        let timeout = remaining_timeout(deadline).ok_or(ForwarderError::AppUnreachable)?;
        self.send_serialized_with_timeout(request_json, timeout)
    }

    fn send_serialized_with_timeout(
        &self,
        request_json: String,
        timeout: Duration,
    ) -> Result<AppResponseEnvelope, ForwarderError> {
        let pipe_path = self.pipe_path.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(send_serialized_once(&pipe_path, &request_json));
        });

        receiver
            .recv_timeout(timeout)
            .map_err(|_| ForwarderError::AppUnreachable)?
    }
}

fn send_serialized_once(
    pipe_path: &str,
    request_json: &str,
) -> Result<AppResponseEnvelope, ForwarderError> {
    let mut stream = OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe_path)
        .map_err(map_open_error)?;

    stream
        .write_all(request_json.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .and_then(|_| stream.flush())
        .map_err(|error| {
            ForwarderError::Transport(format!("Could not send app request: {error}"))
        })?;

    let mut reader = BufReader::new(stream);
    let response_line = read_limited_response_line(&mut reader)?;

    if response_line.trim().is_empty() {
        return Err(ForwarderError::AppUnreachable);
    }

    serde_json::from_str::<AppResponseEnvelope>(&response_line).map_err(|error| {
        ForwarderError::Serialization(format!("Could not parse app response: {error}"))
    })
}

fn read_limited_response_line<R: BufRead>(reader: &mut R) -> Result<String, ForwarderError> {
    let mut response = Vec::new();

    loop {
        let available = reader.fill_buf().map_err(|error| {
            ForwarderError::Transport(format!("Could not read app response: {error}"))
        })?;

        if available.is_empty() {
            break;
        }

        let newline_index = available.iter().position(|byte| *byte == b'\n');
        let read_len = newline_index
            .map(|index| index.saturating_add(1))
            .unwrap_or(available.len());

        if response.len().saturating_add(read_len) > MAX_APP_RESPONSE_BYTES {
            return Err(ForwarderError::Transport(format!(
                "App response exceeds {MAX_APP_RESPONSE_BYTES} bytes."
            )));
        }

        response.extend_from_slice(&available[..read_len]);
        reader.consume(read_len);

        if newline_index.is_some() {
            break;
        }
    }

    String::from_utf8(response).map_err(|error| {
        ForwarderError::Serialization(format!("Could not decode app response: {error}"))
    })
}

fn map_open_error(error: std::io::Error) -> ForwarderError {
    if error.kind() == std::io::ErrorKind::NotFound {
        return ForwarderError::AppUnreachable;
    }

    ForwarderError::Transport(format!("Could not connect to app transport: {error}"))
}

fn resolve_desktop_path() -> PathBuf {
    if let Ok(path) = std::env::var("MYAPP_DESKTOP_PATH") {
        return PathBuf::from(path);
    }

    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .map(|parent| parent.join(DEFAULT_APP_EXECUTABLE))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_APP_EXECUTABLE))
}

fn remaining_timeout(deadline: Instant) -> Option<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        None
    } else {
        Some(remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    #[test]
    fn app_response_reader_rejects_oversized_lines() {
        let raw = vec![b'x'; MAX_APP_RESPONSE_BYTES + 1];
        let mut reader = BufReader::new(Cursor::new(raw));

        let error = read_limited_response_line(&mut reader)
            .expect_err("oversized app response should reject");

        assert!(matches!(error, ForwarderError::Transport(_)));
    }
}
