fn main() {
    match simple_download_manager_desktop_slint::smoke::run_smoke_command_from_env() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("Simple Download Manager smoke command failed: {error}");
            std::process::exit(1);
        }
    }

    if let Err(error) = simple_download_manager_desktop_slint::runtime::run_app() {
        eprintln!("Simple Download Manager failed to start: {error}");
        std::process::exit(1);
    }
}
