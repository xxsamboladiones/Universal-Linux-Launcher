mod cache;
mod palette;
mod providers;
mod wallpaper;
mod watcher;
use crate::{error::Result, themes::manifest::ThemeTokens};
pub use palette::ColorPalette;
pub use providers::ProviderStatus;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
pub use watcher::PywalWatcher;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomaticTheme {
    pub palette: ColorPalette,
    pub tokens: ThemeTokens,
    pub source: String,
    pub wallpaper_path: String,
    pub palette_hash: String,
}
pub fn detect_provider() -> ProviderStatus {
    providers::pywal_status()
}
pub fn current_wallpaper() -> Result<String> {
    Ok(wallpaper::current_wallpaper()?.to_string_lossy().into())
}
pub fn generate(
    influence: u8,
    color_mode: &str,
    source_preference: &str,
    manual: Option<String>,
) -> Result<AutomaticTheme> {
    let influence = influence.min(100);
    let path = match manual {
        Some(path) => wallpaper::validate(&PathBuf::from(path))?,
        None => wallpaper::current_wallpaper()?,
    };
    let source = providers::pywal_status();
    if source_preference != "native" && source.available {
        if let Some(palette) = providers::pywal_palette(influence, color_mode)? {
            return Ok(AutomaticTheme {
                tokens: palette.tokens(),
                palette,
                source: source.provider,
                wallpaper_path: path.to_string_lossy().into(),
                palette_hash: "pywal".into(),
            });
        }
    }
    let (palette, hash) = providers::native_palette(&path, influence, color_mode)?;
    let key = format!("{hash}-{influence}-{color_mode}");
    let palette = cache::load(&key).unwrap_or(palette);
    cache::save(&key, &palette)?;
    Ok(AutomaticTheme {
        tokens: palette.tokens(),
        palette,
        source: "Orbit Native".into(),
        wallpaper_path: path.to_string_lossy().into(),
        palette_hash: hash,
    })
}
