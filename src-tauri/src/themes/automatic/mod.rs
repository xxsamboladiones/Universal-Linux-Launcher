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
    let fallback_key = format!("last-{influence}-{color_mode}");
    match generate_inner(influence, color_mode, source_preference, manual) {
        Ok(theme) => {
            if let Err(error) = cache::save(&fallback_key, &theme.palette) {
                tracing::warn!(%error, "não foi possível atualizar o cache da paleta automática");
            }
            Ok(theme)
        }
        Err(error) => {
            let Some(palette) = cache::load(&fallback_key) else {
                return Err(error);
            };
            Ok(AutomaticTheme {
                tokens: palette.tokens(),
                palette,
                source: "Última paleta válida".into(),
                wallpaper_path: "Wallpaper anterior".into(),
                palette_hash: fallback_key,
            })
        }
    }
}

fn generate_inner(
    influence: u8,
    color_mode: &str,
    source_preference: &str,
    manual: Option<String>,
) -> Result<AutomaticTheme> {
    let source = providers::pywal_status();
    if source_preference != "native" && source.available {
        match providers::pywal_palette(influence, color_mode) {
            Ok(Some(result)) => {
                let palette = result.palette;
                return Ok(AutomaticTheme {
                    tokens: palette.tokens(),
                    palette,
                    source: source.provider,
                    wallpaper_path: result
                        .wallpaper
                        .or_else(providers::pywal_wallpaper_path)
                        .or(manual.clone())
                        .unwrap_or_else(|| "Wallpaper do Pywal".into()),
                    palette_hash: result.hash,
                });
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, "paleta Pywal inválida; tentando gerador nativo");
            }
        }
    }
    let path = match manual {
        Some(path) => wallpaper::validate(&PathBuf::from(path))?,
        None => wallpaper::current_wallpaper()?,
    };
    let (bytes, hash) = providers::wallpaper_bytes(&path)?;
    let key = format!("{hash}-{influence}-{color_mode}");
    let palette = match cache::load(&key) {
        Some(palette) => palette,
        None => {
            let palette = providers::native_palette(&bytes, influence, color_mode)?;
            if let Err(error) = cache::save(&key, &palette) {
                tracing::warn!(%error, "não foi possível gravar o cache da paleta automática");
            }
            palette
        }
    };
    Ok(AutomaticTheme {
        tokens: palette.tokens(),
        palette,
        source: "Orbit Native".into(),
        wallpaper_path: path.to_string_lossy().into(),
        palette_hash: hash,
    })
}
