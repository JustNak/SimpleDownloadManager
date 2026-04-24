use crate::protocol::{AppRequestEnvelope, AppResponseEnvelope};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

const DEFAULT_PIPE_PATH: &str = r"\\.\pipe\myapp.downloads.v1";
const DEFAULT_APP_EXECUTABLE: &str = "simple-download-manager.exe";
const CONNECT_ATTEMPTS: usize = 10;
const CONNECT_DELAY: Duration = Duration::from_millis(300);

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
        match self.send_once(request) {
            Ok(response) => Ok(response),
            Err(ForwarderError::AppUnreachable) => {
                self.launch_app()?;

                for _ in 0..CONNECT_ATTEMPTS {
                    thread::sleep(CONNECT_DELAY);
                    if let Ok(response) = self.send_once(request) {
                        return Ok(response);
                    }
                }

                Err(ForwarderError::AppUnreachable)
            }
            Err(error) => Err(error),
        }
    }

    fn send_once<T>(
        &self,
        request: &AppRequestEnvelope<T>,
    ) -> Result<AppResponseEnvelope, ForwarderError>
    where
        T: Serialize,
    {
        let mut stream = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.pipe_path)
            .map_err(map_open_error)?;

        let request_json = serde_json::to_string(request).map_err(|error| {
            ForwarderError::Serialization(format!("Could not serialize app request: {error}"))
        })?;

        stream
            .write_all(request_json.as_bytes())
            .and_then(|_| stream.write_all(b"\n"))
            .and_then(|_| stream.flush())
            .map_err(|error| {
                ForwarderError::Transport(format!("Could not send app request: {error}"))
            })?;

        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        reader.read_line(&mut response_line).map_err(|error| {
            ForwarderError::Transport(format!("Could not read app response: {error}"))
        })?;

        if response_line.trim().is_empty() {
            return Err(ForwarderError::AppUnreachable);
        }

        serde_json::from_str::<AppResponseEnvelope>(&response_line).map_err(|error| {
            ForwarderError::Serialization(format!("Could not parse app response: {error}"))
        })
    }
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
