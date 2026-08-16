use crate::error::{LauncherError, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};
const MAX_WALLPAPER_BYTES: u64 = 30 * 1024 * 1024;
pub fn current_wallpaper() -> Result<PathBuf> {
    for path in kde_configs() {
        if let Some(found) = read_kde_wallpaper(&path)? {
            return validate(&found);
        }
    }
    Err(LauncherError::NotFound(
        "wallpaper atual não encontrado; defina um wallpaper manual ou use um tema manual".into(),
    ))
}
fn kde_configs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(home) = dirs::home_dir() {
        v.push(home.join(".config/plasma-org.kde.plasma.desktop-appletsrc"));
        v.push(home.join(".config/plasmashellrc"));
    }
    v
}
fn read_kde_wallpaper(path: &Path) -> Result<Option<PathBuf>> {
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(None);
    };
    if text.len() > 2 * 1024 * 1024 {
        return Ok(None);
    };
    for line in text.lines() {
        let value = line
            .trim()
            .strip_prefix("Image=")
            .or_else(|| line.trim().strip_prefix("wallpaper="))
            .unwrap_or("");
        if let Some(value) = value.strip_prefix("file://") {
            let decoded = value.replace("%20", " ");
            let p = PathBuf::from(decoded);
            if p.exists() {
                return Ok(Some(p));
            }
        }
    }
    Ok(None)
}
pub fn validate(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(LauncherError::InvalidTheme(
            "caminho de wallpaper deve ser absoluto".into(),
        ));
    }
    let meta = fs::metadata(path)?;
    if !meta.is_file() || meta.len() > MAX_WALLPAPER_BYTES {
        return Err(LauncherError::InvalidTheme(
            "wallpaper inválido ou maior que 30 MB".into(),
        ));
    }
    let canonical = fs::canonicalize(path)?;
    if !canonical.is_file() {
        return Err(LauncherError::InvalidTheme("wallpaper inválido".into()));
    }
    Ok(canonical)
}
