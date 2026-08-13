use super::LibraryProvider;
use crate::{
    core::model::{ItemKind, LibraryItem, ProviderKind},
    error::Result,
};
use std::{collections::HashSet, fs, path::PathBuf};
pub struct SteamProvider;
impl LibraryProvider for SteamProvider {
    fn name(&self) -> &'static str {
        "steam"
    }
    fn is_available(&self) -> bool {
        roots().iter().any(|p| p.exists())
    }
    fn scan(&self) -> Result<Vec<LibraryItem>> {
        let mut libraries = HashSet::new();
        let steam_roots: Vec<_> = roots().into_iter().filter(|p| p.exists()).collect();
        for root in &steam_roots {
            libraries.insert(root.clone());
            let config = root.join("steamapps/libraryfolders.vdf");
            if let Ok(text) = fs::read_to_string(config) {
                for path in extract_values(&text, "path") {
                    libraries.insert(PathBuf::from(path.replace("\\\\", "\\")));
                }
            }
        }
        let mut items = vec![];
        for lib in libraries {
            let steamapps = lib.join("steamapps");
            let Ok(entries) = fs::read_dir(&steamapps) else {
                continue;
            };
            for entry in entries.flatten() {
                let file = entry.file_name();
                let name = file.to_string_lossy();
                if !name.starts_with("appmanifest_") || !name.ends_with(".acf") {
                    continue;
                }
                let Ok(text) = fs::read_to_string(entry.path()) else {
                    continue;
                };
                let Some(appid) = extract_values(&text, "appid").into_iter().next() else {
                    continue;
                };
                let Some(name) = extract_values(&text, "name").into_iter().next() else {
                    continue;
                };
                let mut item = LibraryItem::new(
                    format!("steam:{appid}"),
                    name,
                    ItemKind::Game,
                    ProviderKind::Steam,
                );
                item.executable = Some("steam".into());
                item.working_directory = Some(
                    steamapps
                        .join("common")
                        .join(
                            extract_values(&text, "installdir")
                                .into_iter()
                                .next()
                                .unwrap_or_default(),
                        )
                        .to_string_lossy()
                        .into(),
                );
                item.category = Some("Steam".into());
                if let Some(cache) = steam_roots
                    .iter()
                    .map(|root| root.join("appcache/librarycache").join(&appid))
                    .find(|path| path.is_dir())
                {
                    let cover = cache.join("library_600x900.jpg");
                    let logo = cache.join("logo.png");
                    if cover.is_file() {
                        item.cover = Some(cover.to_string_lossy().into_owned());
                    }
                    if logo.is_file() {
                        item.icon = Some(logo.to_string_lossy().into_owned());
                    }
                }
                items.push(item)
            }
        }
        Ok(items)
    }
}
fn roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return vec![];
    };
    vec![
        home.join(".local/share/Steam"),
        home.join(".steam/steam"),
        home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
    ]
}
fn extract_values(text: &str, key: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let tokens: Vec<_> = line.split('"').collect();
            if tokens.len() >= 4 && tokens[1].trim() == key {
                Some(tokens[3].to_string())
            } else {
                None
            }
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_vdf() {
        let x = "\"appid\"  \"730\"\n\"name\" \"Counter-Strike 2\"";
        assert_eq!(extract_values(x, "appid"), ["730"]);
        assert_eq!(extract_values(x, "name"), ["Counter-Strike 2"])
    }
    #[test]
    fn supports_library_path() {
        assert_eq!(
            extract_values("\"path\" \"/games/SteamLibrary\"", "path"),
            ["/games/SteamLibrary"]
        )
    }
}
