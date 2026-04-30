use crate::commands::{ProgressBatchRegistry, TauriShellServices};
use crate::prompts::PromptRegistry;
use crate::state::SharedState;
use crate::storage::DiagnosticLevel;
use simple_download_manager_desktop_core::backend::CoreDesktopBackend;
use simple_download_manager_desktop_core::host_protocol::HostRequest;
use std::time::Duration;
use tauri::AppHandle;

pub const PIPE_NAME: &str = r"\\.\pipe\myapp.downloads.v1";

#[cfg(windows)]
const DIAGNOSTIC_POLL_INTERVAL: Duration = Duration::from_secs(5);
#[cfg(windows)]
const MAX_PIPE_REQUEST_BYTES: usize = 1024 * 1024;
#[cfg(windows)]
const PIPE_READ_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(windows)]
const PIPE_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(windows)]
const PIPE_MAX_INSTANCES: usize = 4;

#[cfg(windows)]
pub fn start_named_pipe_listener(app: AppHandle, state: SharedState, prompts: PromptRegistry) {
    let backend = CoreDesktopBackend::new(
        state,
        prompts,
        ProgressBatchRegistry::default(),
        TauriShellServices::new(app),
    );

    let listener_backend = backend.clone();
    tauri::async_runtime::spawn(async move {
        let _ = listener_backend.refresh_host_connection_diagnostics().await;

        let mut first_pipe_instance = true;
        loop {
            if let Err(error) =
                accept_single_connection(listener_backend.clone(), first_pipe_instance).await
            {
                eprintln!("named pipe listener error: {error}");
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            } else {
                first_pipe_instance = false;
            }
        }
    });

    let diagnostics_backend = backend;
    tauri::async_runtime::spawn(async move {
        loop {
            let _ = diagnostics_backend
                .refresh_host_connection_diagnostics()
                .await;
            tokio::time::sleep(DIAGNOSTIC_POLL_INTERVAL).await;
        }
    });
}

#[cfg(not(windows))]
pub fn start_named_pipe_listener(_app: AppHandle, _state: SharedState, _prompts: PromptRegistry) {}

#[cfg(windows)]
async fn accept_single_connection(
    backend: CoreDesktopBackend<TauriShellServices>,
    first_pipe_instance: bool,
) -> Result<(), String> {
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

    tauri::async_runtime::spawn(async move {
        let result: Result<(), String> = async {
            let (reader, mut writer) = tokio::io::split(server);
            let mut reader = BufReader::new(reader);
            let request_line =
                tokio::time::timeout(PIPE_READ_TIMEOUT, read_limited_request_line(&mut reader))
                    .await
                    .map_err(|_| "Timed out reading named pipe payload.".to_string())??;

            if request_line.trim().is_empty() {
                return Ok(());
            }

            let request = serde_json::from_str::<HostRequest>(&request_line)
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
            let response_json = serde_json::to_string(&response)
                .map_err(|error| format!("Could not serialize host response: {error}"))?;

            tokio::time::timeout(PIPE_WRITE_TIMEOUT, async {
                writer
                    .write_all(response_json.as_bytes())
                    .await
                    .map_err(|error| format!("Could not write named pipe response: {error}"))?;

                writer.write_all(b"\n").await.map_err(|error| {
                    format!("Could not write named pipe response terminator: {error}")
                })?;

                writer
                    .flush()
                    .await
                    .map_err(|error| format!("Could not flush named pipe response: {error}"))
            })
            .await
            .map_err(|_| "Timed out writing named pipe response.".to_string())??;

            Ok(())
        }
        .await;

        if let Err(error) = result {
            eprintln!("named pipe request error: {error}");
        }
    });

    Ok(())
}

#[cfg(windows)]
async fn read_limited_request_line<R>(reader: &mut R) -> Result<String, String>
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

        if request.len().saturating_add(read_len) > MAX_PIPE_REQUEST_BYTES {
            return Err(format!(
                "Named pipe payload exceeds {MAX_PIPE_REQUEST_BYTES} bytes."
            ));
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
