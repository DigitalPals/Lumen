use std::{
    fs,
    path::{Path, PathBuf},
};

use lumen_icons::IconRegistry;

use crate::cli::CliAction;

const SOURCE_RESOURCES_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../resources/icons/hicolor/scalable/actions"
);

const PACKAGED_RESOURCES_DIRS: &[&str] = &[
    "/app/share/lumen/icons/hicolor/scalable/actions",
    "/usr/share/lumen/icons/hicolor/scalable/actions",
    "/usr/local/share/lumen/icons/hicolor/scalable/actions",
];

fn resources_dir() -> PathBuf {
    option_env!("LUMEN_RESOURCES_DIR")
        .into_iter()
        .chain(PACKAGED_RESOURCES_DIRS.iter().copied())
        .chain(std::iter::once(SOURCE_RESOURCES_DIR))
        .map(PathBuf::from)
        .find(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(SOURCE_RESOURCES_DIR))
}

/// Installs bundled icons from the resources directory.
///
/// # Errors
///
/// Returns error if source directory doesn't exist or copy fails.
pub fn execute() -> CliAction {
    let source_dir = resources_dir();

    if !source_dir.exists() {
        return Err(format!(
            "Resources directory not found: {}",
            source_dir.display()
        ));
    }

    let registry = IconRegistry::new().map_err(|err| err.to_string())?;
    let dest_dir = registry.icons_dir();

    fs::create_dir_all(&dest_dir)
        .map_err(|err| format!("Failed to create icons directory: {err}"))?;

    let entries = fs::read_dir(Path::new(&source_dir))
        .map_err(|err| format!("Failed to read resources directory: {err}"))?;

    let mut count = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(filename) = path.file_name() else {
            continue;
        };
        if path.extension().is_some_and(|ext| ext == "svg") {
            let dest_path = dest_dir.join(filename);
            fs::copy(&path, &dest_path)
                .map_err(|err| format!("Failed to copy {}: {err}", path.display()))?;
            println!(
                "Installed: {}",
                filename.to_string_lossy().trim_end_matches(".svg")
            );
            count += 1;
        }
    }

    println!("\n{count} icons installed to {}", dest_dir.display());
    Ok(())
}
