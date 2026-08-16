use super::palette::ColorPalette;
use crate::error::Result;
use std::{fs, path::PathBuf};
use tempfile::NamedTempFile;
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
    load_path(&p)
}
fn load_path(path: &std::path::Path) -> Option<ColorPalette> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() > 128 * 1024 {
        return None;
    }
    let palette: ColorPalette = serde_json::from_slice(&bytes).ok()?;
    palette.validate().ok()?;
    Some(palette)
}
pub fn save(key: &str, palette: &ColorPalette) -> Result<()> {
    palette.validate()?;
    let directory = dir()?;
    let destination = directory.join(format!("{key}.json"));
    let mut temporary = NamedTempFile::new_in(directory)?;
    serde_json::to_writer(&mut temporary, palette)
        .map_err(|error| crate::error::LauncherError::InvalidTheme(error.to_string()))?;
    temporary
        .persist(destination)
        .map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_corrupt_and_oversized_cache_entries() {
        let directory = tempfile::tempdir().unwrap();
        let corrupt = directory.path().join("corrupt.json");
        fs::write(&corrupt, b"not-json").unwrap();
        assert!(load_path(&corrupt).is_none());

        let oversized = directory.path().join("oversized.json");
        fs::write(&oversized, vec![b' '; 128 * 1024 + 1]).unwrap();
        assert!(load_path(&oversized).is_none());
    }
}
