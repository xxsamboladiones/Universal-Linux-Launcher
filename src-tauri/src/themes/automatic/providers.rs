use super::palette::{normalized, ColorPalette};
use crate::error::{LauncherError, Result};
use image::imageops::FilterType;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
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
    let cache_available = pywal_cache_dir()
        .map(|directory| directory.join("colors.json"))
        .and_then(|path| fs::read(path).ok())
        .filter(|bytes| bytes.len() <= 128 * 1024)
        .is_some_and(|bytes| parse_pywal_palette(&bytes, 100, "automatic").is_ok());
    if cache_available {
        return ProviderStatus {
            provider: "pywal-cache".into(),
            available: true,
            version: None,
        };
    }
    ProviderStatus {
        provider: "native".into(),
        available: false,
        version: None,
    }
}
fn find_program(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|path| is_executable(path))
}
fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    return metadata.is_file() && metadata.permissions().mode() & 0o111 != 0;
    #[cfg(not(unix))]
    return metadata.is_file();
}

pub(super) fn pywal_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".cache")))
        .map(|cache| cache.join("wal"))
}

pub struct PywalPalette {
    pub palette: ColorPalette,
    pub hash: String,
    pub wallpaper: Option<String>,
}

pub fn pywal_palette(influence: u8, mode: &str) -> Result<Option<PywalPalette>> {
    let Some(path) = pywal_cache_dir().map(|directory| directory.join("colors.json")) else {
        return Ok(None);
    };
    let Ok(bytes) = fs::read(&path) else {
        return Ok(None);
    };
    if bytes.len() > 128 * 1024 {
        return Ok(None);
    }
    parse_pywal_palette(&bytes, influence, mode).map(Some)
}

fn parse_pywal_palette(bytes: &[u8], influence: u8, mode: &str) -> Result<PywalPalette> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
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
    let palette = normalized(
        get(special, "background")?,
        get(colors, "color5").or_else(|_| get(colors, "color4"))?,
        get(colors, "color6").or_else(|_| get(colors, "color2"))?,
        influence,
        mode,
    )?;
    let wallpaper = value
        .get("wallpaper")
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    Ok(PywalPalette {
        palette,
        hash: format!("{:x}", Sha256::digest(bytes)),
        wallpaper,
    })
}

/// O caminho é informativo para a UI; jamais é usado como comando ou para
/// abrir arquivos sem a validação do provider nativo.
pub fn pywal_wallpaper_path() -> Option<String> {
    let bytes = fs::read(pywal_cache_dir()?.join("colors.json")).ok()?;
    if bytes.len() > 128 * 1024 {
        return None;
    }
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()?
        .get("wallpaper")?
        .as_str()
        .map(str::to_owned)
}
pub fn wallpaper_bytes(path: &Path) -> Result<(Vec<u8>, String)> {
    let bytes = fs::read(path)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    Ok((bytes, hash))
}

pub fn native_palette(bytes: &[u8], influence: u8, mode: &str) -> Result<ColorPalette> {
    let image = image::load_from_memory(bytes)
        .map_err(|_| LauncherError::InvalidTheme("imagem do wallpaper inválida".into()))?
        .resize(96, 96, FilterType::Triangle)
        .to_rgb8();
    let mut bins: HashMap<(u8, u8, u8), u32> = HashMap::new();
    for pixel in image.pixels() {
        let (r, g, b) = (pixel[0] / 32 * 32, pixel[1] / 32 * 32, pixel[2] / 32 * 32);
        *bins.entry((r, g, b)).or_default() += 1
    }
    let mut sorted: Vec<_> = bins.into_iter().collect();
    sorted.sort_by(|(left_color, left_count), (right_color, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_color.cmp(right_color))
    });
    let color = |n: usize| {
        let (r, g, b) = sorted.get(n).map(|(c, _)| *c).unwrap_or((90, 80, 180));
        format!("#{r:02x}{g:02x}{b:02x}")
    };
    normalized(&color(0), &color(1), &color(2), influence, mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
    use std::io::Cursor;

    #[test]
    fn parses_a_valid_pywal_palette_and_hashes_its_contents() {
        let bytes = br##"{
          "wallpaper":"/tmp/wallpaper.png",
          "special":{"background":"#101018","foreground":"#eeeeee"},
          "colors":{"color4":"#4455aa","color5":"#8855cc","color6":"#22aacc"}
        }"##;
        let result = parse_pywal_palette(bytes, 100, "automatic").unwrap();
        assert_eq!(result.wallpaper.as_deref(), Some("/tmp/wallpaper.png"));
        assert_eq!(result.palette.primary, "#8855cc");
        assert_eq!(result.hash.len(), 64);
    }

    #[test]
    fn rejects_invalid_pywal_colors() {
        let bytes = br##"{
          "special":{"background":"not-a-color"},
          "colors":{"color4":"#4455aa","color5":"#8855cc","color6":"#22aacc"}
        }"##;
        assert!(parse_pywal_palette(bytes, 70, "automatic").is_err());
    }

    #[test]
    fn native_generation_is_deterministic() {
        let mut image = RgbImage::new(4, 4);
        for (index, pixel) in image.pixels_mut().enumerate() {
            *pixel = if index % 2 == 0 {
                Rgb([20, 40, 90])
            } else {
                Rgb([170, 40, 120])
            };
        }
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        let first = native_palette(bytes.get_ref(), 80, "automatic").unwrap();
        let second = native_palette(bytes.get_ref(), 80, "automatic").unwrap();
        assert_eq!(first.background, second.background);
        assert_eq!(first.primary, second.primary);
        assert_eq!(first.secondary, second.secondary);
    }
}
