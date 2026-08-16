use super::palette::{normalized, ColorPalette};
use crate::error::{LauncherError, Result};
use image::imageops::FilterType;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub provider: String,
    pub available: bool,
    pub version: Option<String>,
}
pub fn pywal_status() -> ProviderStatus {
    for name in ["wal", "pywal", "pywal16"] {
        if find_program(name).is_some() {
            return ProviderStatus {
                provider: name.into(),
                available: true,
                version: None,
            };
        }
    }
    ProviderStatus {
        provider: "native".into(),
        available: false,
        version: None,
    }
}
fn find_program(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?
        .to_string_lossy()
        .split(':')
        .map(PathBuf::from)
        .find(|p| {
            let x = p.join(name);
            fs::metadata(x).is_ok_and(|m| m.is_file())
        })
}
pub fn pywal_palette(influence: u8, mode: &str) -> Result<Option<ColorPalette>> {
    let Some(home) = dirs::home_dir() else {
        return Ok(None);
    };
    let path = home.join(".cache/wal/colors.json");
    let Ok(bytes) = fs::read(&path) else {
        return Ok(None);
    };
    if bytes.len() > 128 * 1024 {
        return Ok(None);
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| LauncherError::InvalidTheme("paleta Pywal inválida".into()))?;
    let special = value
        .get("special")
        .and_then(|v| v.as_object())
        .ok_or_else(|| LauncherError::InvalidTheme("paleta Pywal inválida".into()))?;
    let colors = value
        .get("colors")
        .and_then(|v| v.as_object())
        .ok_or_else(|| LauncherError::InvalidTheme("paleta Pywal inválida".into()))?;
    fn get<'a>(map: &'a serde_json::Map<String, serde_json::Value>, key: &str) -> Result<&'a str> {
        map.get(key)
            .and_then(|v| v.as_str())
            .ok_or_else(|| LauncherError::InvalidTheme("paleta Pywal inválida".into()))
    }
    Ok(Some(normalized(
        get(special, "background")?,
        get(colors, "color5").or_else(|_| get(colors, "color4"))?,
        get(colors, "color6").or_else(|_| get(colors, "color2"))?,
        influence,
        mode,
    )?))
}
pub fn native_palette(path: &Path, influence: u8, mode: &str) -> Result<(ColorPalette, String)> {
    let bytes = fs::read(path)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let image = image::load_from_memory(&bytes)
        .map_err(|_| LauncherError::InvalidTheme("imagem do wallpaper inválida".into()))?
        .resize(96, 96, FilterType::Triangle)
        .to_rgb8();
    let mut bins: HashMap<(u8, u8, u8), u32> = HashMap::new();
    for pixel in image.pixels() {
        let (r, g, b) = (pixel[0] / 32 * 32, pixel[1] / 32 * 32, pixel[2] / 32 * 32);
        *bins.entry((r, g, b)).or_default() += 1
    }
    let mut sorted: Vec<_> = bins.into_iter().collect();
    sorted.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let color = |n: usize| {
        let (r, g, b) = sorted.get(n).map(|(c, _)| *c).unwrap_or((90, 80, 180));
        format!("#{r:02x}{g:02x}{b:02x}")
    };
    Ok((
        normalized(&color(0), &color(1), &color(2), influence, mode)?,
        hash,
    ))
}
