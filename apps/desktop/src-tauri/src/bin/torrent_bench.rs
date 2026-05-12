#[cfg(debug_assertions)]
#[tokio::main]
async fn main() {
    match simple_download_manager_desktop_backend::torrent_bench::run_benchmark_from_env().await {
        Ok(report) => {
            match simple_download_manager_desktop_backend::torrent_bench::redacted_report_json(
                &report,
            ) {
                Ok(json) => println!("{json}"),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(debug_assertions))]
fn main() {
    eprintln!("torrent_bench is a dev-only binary; build it without --release.");
    std::process::exit(1);
}
