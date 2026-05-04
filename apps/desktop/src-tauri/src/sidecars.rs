use std::path::{Path, PathBuf};

pub fn resolve_seven_zip_binary_path() -> Result<PathBuf, String> {
    let candidates = seven_zip_binary_candidates()?;
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            "Bundled 7-Zip sidecar was not found. Reinstall the app or rebuild the installer."
                .into()
        })
}

fn seven_zip_binary_candidates() -> Result<Vec<PathBuf>, String> {
    let mut candidates = Vec::new();

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            push_seven_zip_candidates(&mut candidates, exe_dir);
            push_seven_zip_candidates(&mut candidates, &exe_dir.join("resources"));
            push_seven_zip_candidates(&mut candidates, &exe_dir.join("resources").join("bin"));
            push_seven_zip_candidates(
                &mut candidates,
                &exe_dir.join("resources").join("resources").join("bin"),
            );
        }
    }

    let current_dir = std::env::current_dir()
        .map_err(|error| format!("Could not resolve current directory for 7-Zip: {error}"))?;
    push_seven_zip_candidates(&mut candidates, &current_dir);
    push_seven_zip_candidates(&mut candidates, &current_dir.join("resources").join("bin"));
    push_seven_zip_candidates(
        &mut candidates,
        &current_dir
            .join("apps")
            .join("desktop")
            .join("src-tauri")
            .join("resources")
            .join("bin"),
    );

    Ok(candidates)
}

fn push_seven_zip_candidates(candidates: &mut Vec<PathBuf>, directory: &Path) {
    candidates.push(directory.join("7z.exe"));
    candidates.push(directory.join("7zz.exe"));
}
