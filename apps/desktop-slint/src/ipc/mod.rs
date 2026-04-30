use simple_download_manager_desktop_core::backend::CoreDesktopBackend;
use simple_download_manager_desktop_core::contracts::ShellServices;
use simple_download_manager_desktop_core::host_protocol::{HostRequest, HostResponse};
use simple_download_manager_desktop_core::storage::DiagnosticLevel;

#[cfg(windows)]
use crate::runtime::{SlintBackend, UiDispatcher};
#[cfg(windows)]
use std::sync::Arc;
use std::time::Duration;

pub const PIPE_NAME: &str = r"\\.\pipe\myapp.downloads.v1";

#[cfg(windows)]
const DIAGNOSTIC_POLL_INTERVAL: Duration = Duration::from_secs(5);
const MAX_PIPE_REQUEST_BYTES: usize = 1024 * 1024;
#[cfg(windows)]
const PIPE_READ_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(windows)]
const PIPE_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(windows)]
const PIPE_MAX_INSTANCES: usize = 4;

#[cfg(windows)]
pub fn start_named_pipe_listener<D>(
    runtime: Arc<tokio::runtime::Runtime>,
    backend: Arc<SlintBackend<D>>,
) where
    D: UiDispatcher,
{
    let listener_backend = backend.clone();
    runtime.spawn(async move {
        let _ = listener_backend.refresh_host_connection_diagnostics().await;

        let mut first_pipe_instance = true;
        loop {
            if let Err(error) =
                accept_single_connection(listener_backend.clone(), first_pipe_instance).await
            {
                eprintln!("named pipe listener error: {error}");
                tokio::time::sleep(Duration::from_millis(500)).await;
            } else {
                first_pipe_instance = false;
            }
        }
    });

    runtime.spawn(async move {
        loop {
            let _ = backend.refresh_host_connection_diagnostics().await;
            tokio::time::sleep(DIAGNOSTIC_POLL_INTERVAL).await;
        }
    });
}

#[cfg(not(windows))]
pub fn start_named_pipe_listener<D>(
    _runtime: std::sync::Arc<tokio::runtime::Runtime>,
    _backend: std::sync::Arc<crate::runtime::SlintBackend<D>>,
) where
    D: crate::runtime::UiDispatcher,
{
}

pub async fn handle_request_line<S>(
    backend: &CoreDesktopBackend<S>,
    request_line: &str,
) -> Result<Option<String>, String>
where
    S: ShellServices + 'static,
{
    if request_line.trim().is_empty() {
        return Ok(None);
    }

    let request = serde_json::from_str::<HostRequest>(request_line)
        .map_err(|error| format!("Could not parse host request: {error}"))?;

    let response = backend.handle_host_request(request).await;
    if !response.ok {
        let _ = backend
            .state()
            .record_diagnostic_event(
                DiagnosticLevel::Warning,
                "native_host",
                response
                    .message
                    .clone()
                    .unwrap_or_else(|| "Native host request was rejected.".into()),
                None,
            )
            .await;
    }

    let mut response_json = serialize_host_response(&response)?;
    response_json.push('\n');
    Ok(Some(response_json))
}

fn serialize_host_response(response: &HostResponse) -> Result<String, String> {
    serde_json::to_string(response)
        .map_err(|error| format!("Could not serialize host response: {error}"))
}

pub async fn read_limited_request_line_for_tests<R>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<String, String>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    read_limited_request_line(reader, max_bytes).await
}

#[cfg(windows)]
async fn accept_single_connection<D>(
    backend: Arc<SlintBackend<D>>,
    first_pipe_instance: bool,
) -> Result<(), String>
where
    D: UiDispatcher,
{
    use tokio::io::{AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ServerOptions;

    let mut server_options = ServerOptions::new();
    server_options
        .reject_remote_clients(true)
        .max_instances(PIPE_MAX_INSTANCES);
    if first_pipe_instance {
        server_options.first_pipe_instance(true);
    }

    let server = server_options
        .create(PIPE_NAME)
        .map_err(|error| format!("Could not create named pipe server: {error}"))?;

    server
        .connect()
        .await
        .map_err(|error| format!("Could not accept named pipe connection: {error}"))?;

    tokio::spawn(async move {
        let result: Result<(), String> = async {
            let (reader, mut writer) = tokio::io::split(server);
            let mut reader = BufReader::new(reader);
            let request_line = tokio::time::timeout(
                PIPE_READ_TIMEOUT,
                read_limited_request_line(&mut reader, MAX_PIPE_REQUEST_BYTES),
            )
            .await
            .map_err(|_| "Timed out reading named pipe payload.".to_string())??;

            if let Some(response_json) = handle_request_line(&backend, &request_line).await? {
                tokio::time::timeout(PIPE_WRITE_TIMEOUT, async {
                    writer
                        .write_all(response_json.as_bytes())
                        .await
                        .map_err(|error| format!("Could not write named pipe response: {error}"))?;

                    writer
                        .flush()
                        .await
                        .map_err(|error| format!("Could not flush named pipe response: {error}"))
                })
                .await
                .map_err(|_| "Timed out writing named pipe response.".to_string())??;
            }

            Ok(())
        }
        .await;

        if let Err(error) = result {
            eprintln!("named pipe request error: {error}");
        }
    });

    Ok(())
}

async fn read_limited_request_line<R>(reader: &mut R, max_bytes: usize) -> Result<String, String>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;

    let mut request = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|error| format!("Could not read named pipe payload: {error}"))?;

        if available.is_empty() {
            break;
        }

        let newline_index = available.iter().position(|byte| *byte == b'\n');
        let read_len = newline_index
            .map(|index| index.saturating_add(1))
            .unwrap_or(available.len());

        if request.len().saturating_add(read_len) > max_bytes {
            return Err(format!("Named pipe payload exceeds {max_bytes} bytes."));
        }

        request.extend_from_slice(&available[..read_len]);
        reader.consume(read_len);

        if newline_index.is_some() {
            break;
        }
    }

    String::from_utf8(request)
        .map_err(|error| format!("Named pipe payload was not valid UTF-8: {error}"))
}
