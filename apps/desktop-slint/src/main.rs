fn main() {
    if let Err(error) = simple_download_manager_desktop_slint::runtime::run_app() {
        eprintln!("Simple Download Manager failed to start: {error}");
        std::process::exit(1);
    }
}
