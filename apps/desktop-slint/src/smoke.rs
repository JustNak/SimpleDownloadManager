pub const ENABLE_SMOKE_COMMANDS_ENV: &str = "MYAPP_ENABLE_SMOKE_COMMANDS";
pub const SMOKE_SYNC_AUTOSTART_PREFIX: &str = "--smoke-sync-autostart=";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmokeCommand {
    SyncAutostart { enabled: bool },
}

pub fn parse_smoke_command_from_args<I, S>(args: I) -> Result<Option<SmokeCommand>, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut command = None;

    for argument in args {
        let argument = argument.as_ref();
        let Some(value) = argument.strip_prefix(SMOKE_SYNC_AUTOSTART_PREFIX) else {
            continue;
        };

        if command.is_some() {
            return Err("Only one Slint smoke command can be provided.".into());
        }

        command = Some(match value {
            "enable" => SmokeCommand::SyncAutostart { enabled: true },
            "disable" => SmokeCommand::SyncAutostart { enabled: false },
            _ => {
                return Err(format!(
                    "Unsupported startup smoke command value: {value}. Use enable or disable."
                ));
            }
        });
    }

    Ok(command)
}

pub fn smoke_commands_enabled_from_env() -> bool {
    std::env::var(ENABLE_SMOKE_COMMANDS_ENV)
        .map(|value| value == "1")
        .unwrap_or(false)
}

pub fn run_smoke_command_with<I, S, F>(
    args: I,
    smoke_commands_enabled: bool,
    mut sync_autostart: F,
) -> Result<bool, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    F: FnMut(bool) -> Result<(), String>,
{
    if !smoke_commands_enabled {
        return Ok(false);
    }

    match parse_smoke_command_from_args(args)? {
        Some(SmokeCommand::SyncAutostart { enabled }) => {
            sync_autostart(enabled)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

pub fn run_smoke_command_from_env() -> Result<bool, String> {
    run_smoke_command_with(
        std::env::args(),
        smoke_commands_enabled_from_env(),
        crate::shell::windows::sync_autostart_setting,
    )
}
