use super::palette::ColorPalette;
use crate::error::Result;
use std::{fs, path::PathBuf};
fn dir() -> Result<PathBuf> {
    let base = dirs::cache_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".cache")))
        .ok_or_else(|| {
            crate::error::LauncherError::InvalidTheme("cache XDG indisponível".into())
        })?;
    let p = base.join("orbit-launcher/themes");
    fs::create_dir_all(&p)?;
    Ok(p)
}
pub fn load(key: &str) -> Option<ColorPalette> {
    let p = dir().ok()?.join(format!("{key}.json"));
    serde_json::from_slice(&fs::read(p).ok()?).ok()
}
pub fn save(key: &str, palette: &ColorPalette) -> Result<()> {
    let p = dir()?.join(format!("{key}.json"));
    fs::write(p, serde_json::to_vec(palette).unwrap_or_default())?;
    Ok(())
}
