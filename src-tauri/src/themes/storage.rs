use crate::error::{LauncherError, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn themes_root() -> Result<PathBuf> {
    let data = dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")))
        .ok_or_else(|| {
            LauncherError::InvalidTheme("não foi possível resolver o diretório XDG de dados".into())
        })?;
    let root = data.join("orbit-launcher").join("themes");
    fs::create_dir_all(root.join("installed"))?;
    fs::create_dir_all(root.join("cache"))?;
    Ok(root)
}
pub fn installed_dir() -> Result<PathBuf> {
    Ok(themes_root()?.join("installed"))
}
pub fn theme_dir(id: &str) -> Result<PathBuf> {
    Ok(installed_dir()?.join(id))
}
pub fn safe_remove(dir: &Path) -> Result<()> {
    let installed = installed_dir()?;
    let canonical_parent = fs::canonicalize(&installed)?;
    let canonical = fs::canonicalize(dir)?;
    if canonical.parent() != Some(canonical_parent.as_path()) {
        return Err(LauncherError::InvalidTheme(
            "diretório de tema fora do escopo permitido".into(),
        ));
    }
    fs::remove_dir_all(canonical)?;
    Ok(())
}
