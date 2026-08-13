use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

use super::LibraryProvider;
use crate::{
    core::model::{ItemKind, LibraryItem, ProviderKind},
    error::Result,
};

pub struct AppImageProvider;

impl LibraryProvider for AppImageProvider {
    fn name(&self) -> &'static str {
        "appimage"
    }
    fn is_available(&self) -> bool {
        scan_dirs().iter().any(|path| path.is_dir())
    }

    fn scan(&self) -> Result<Vec<LibraryItem>> {
        let mut items = Vec::new();
        for directory in scan_dirs() {
            let Ok(entries) = fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let is_appimage = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("appimage"));
                let executable = entry
                    .metadata()
                    .map(|meta| meta.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false);
                if !is_appimage || !executable {
                    continue;
                }
                let name = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("AppImage");
                let stable = path.to_string_lossy();
                let mut item = LibraryItem::new(
                    format!(
                        "appimage:{:x}",
                        stable
                            .bytes()
                            .fold(1469598103934665603_u64, |hash, byte| (hash
                                ^ u64::from(byte))
                            .wrapping_mul(1099511628211))
                    ),
                    name.replace(['_', '-'], " "),
                    ItemKind::Application,
                    ProviderKind::Appimage,
                );
                item.executable = Some(stable.into_owned());
                item.working_directory = path
                    .parent()
                    .map(|value| value.to_string_lossy().into_owned());
                item.category = Some("AppImage".into());
                items.push(item);
            }
        }
        Ok(items)
    }
}

fn scan_dirs() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    vec![home.join("Applications"), home.join(".local/bin")]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scan_locations_are_bounded() {
        assert!(scan_dirs()
            .iter()
            .all(|path| path.ends_with("Applications") || path.ends_with(".local/bin")));
    }
}
