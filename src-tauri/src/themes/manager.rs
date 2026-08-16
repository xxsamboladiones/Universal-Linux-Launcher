use super::{
    manifest::{
        Colors, Effects, Radius, Spacing, ThemeDetails, ThemeManifest, ThemeSource, ThemeSummary,
        ThemeTokens, ThemeType,
    },
    storage, validation,
};
use crate::error::{LauncherError, Result};
use semver::Version;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

const ORBIT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct ThemeManager;

impl ThemeManager {
    pub fn list() -> Result<Vec<ThemeSummary>> {
        let mut themes = builtin_themes()
            .into_iter()
            .map(|(manifest, tokens)| summary(&manifest, &tokens, ThemeSource::Builtin, None))
            .collect::<Result<Vec<_>>>()?;
        let root = storage::installed_dir()?;
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                if let Ok((m, t)) = read_theme(&entry.path()) {
                    themes.push(summary(
                        &m,
                        &t,
                        ThemeSource::External,
                        m.preview.as_ref().map(|p| asset_url(entry.path().join(p))),
                    )?);
                }
            }
        }
        themes.sort_by_key(|theme| theme.name.to_lowercase());
        Ok(themes)
    }
    pub fn get(id: &str) -> Result<ThemeDetails> {
        validation::validate_theme_id(id)?;
        if let Some((m, t)) = builtin_themes().into_iter().find(|(m, _)| m.id == id) {
            return Ok(ThemeDetails {
                summary: summary(&m, &t, ThemeSource::Builtin, None)?,
                tokens: t,
            });
        }
        let (m, t) = read_theme(&storage::theme_dir(id)?)?;
        let preview = m
            .preview
            .as_ref()
            .map(|p| asset_url(storage::theme_dir(id).unwrap_or_default().join(p)));
        Ok(ThemeDetails {
            summary: summary(&m, &t, ThemeSource::External, preview)?,
            tokens: t,
        })
    }
    pub fn validate_archive(path: &Path) -> Result<ThemeSummary> {
        let temp = extract_archive(path)?;
        let (m, t) = read_theme(temp.path())?;
        summary(&m, &t, ThemeSource::External, None)
    }
    pub fn import(path: &Path) -> Result<ThemeSummary> {
        if path.extension().and_then(|x| x.to_str()) != Some("orbit-theme") {
            return Err(LauncherError::InvalidTheme(
                "selecione um arquivo .orbit-theme".into(),
            ));
        }
        let temp = extract_archive(path)?;
        let (m, t) = read_theme(temp.path())?;
        let destination = storage::theme_dir(&m.id)?;
        if destination.exists() {
            return Err(LauncherError::InvalidTheme(format!(
                "o tema '{}' já está instalado",
                m.id
            )));
        }
        fs::rename(temp.path(), &destination)?;
        summary(
            &m,
            &t,
            ThemeSource::External,
            m.preview.as_ref().map(|p| asset_url(destination.join(p))),
        )
    }
    pub fn remove(id: &str) -> Result<()> {
        validation::validate_theme_id(id)?;
        if builtin_themes().iter().any(|(m, _)| m.id == id) {
            return Err(LauncherError::InvalidTheme(
                "temas internos não podem ser removidos".into(),
            ));
        }
        let dir = storage::theme_dir(id)?;
        if !dir.exists() {
            return Err(LauncherError::NotFound(format!("tema {id}")));
        }
        storage::safe_remove(&dir)
    }
    pub fn export(id: &str, target: &Path) -> Result<()> {
        validation::validate_theme_id(id)?;
        if builtin_themes().iter().any(|(m, _)| m.id == id) {
            return Err(LauncherError::InvalidTheme(
                "temas internos não podem ser exportados".into(),
            ));
        }
        let source = storage::theme_dir(id)?;
        let (manifest, _) = read_theme(&source)?;
        let file = fs::File::create(target)?;
        let mut archive = ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for path in walk_files(&source)? {
            let relative = path
                .strip_prefix(&source)
                .map_err(|_| LauncherError::Archive("caminho de exportação inválido".into()))?;
            let name = relative.to_string_lossy().replace('\\', "/");
            if !validation::allowed_file(&name) {
                continue;
            }
            archive.start_file(name, options).map_err(zip_error)?;
            let mut data = fs::File::open(path)?;
            std::io::copy(&mut data, &mut archive)?;
        }
        archive.finish().map_err(zip_error)?;
        let _ = manifest;
        Ok(())
    }
}

fn extract_archive(path: &Path) -> Result<TempDir> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > validation::MAX_THEME_BYTES {
        return Err(LauncherError::InvalidTheme(
            "arquivo excede o limite de 20 MB".into(),
        ));
    }
    let file = fs::File::open(path)?;
    let mut archive = ZipArchive::new(file).map_err(zip_error)?;
    if archive.len() > validation::MAX_FILES {
        return Err(LauncherError::InvalidTheme(
            "tema contém arquivos demais".into(),
        ));
    }
    let temp = tempfile::tempdir()?;
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(zip_error)?;
        let name = entry.name().to_string();
        if entry.is_dir() {
            continue;
        }
        validation::validate_relative(&name)?;
        if !validation::allowed_file(&name) {
            return Err(LauncherError::InvalidTheme(format!(
                "o tema contém um arquivo não permitido: {name}"
            )));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000 || mode & 0o111 != 0)
        {
            return Err(LauncherError::InvalidTheme(
                "symlinks e arquivos executáveis não são permitidos".into(),
            ));
        }
        let size = entry.size();
        total = total
            .checked_add(size)
            .ok_or_else(|| LauncherError::InvalidTheme("tamanho inválido".into()))?;
        if total > validation::MAX_THEME_BYTES {
            return Err(LauncherError::InvalidTheme(
                "conteúdo descompactado excede 20 MB".into(),
            ));
        }
        if validation::is_image(&name) && size > validation::MAX_IMAGE_BYTES {
            return Err(LauncherError::InvalidTheme("imagem excede 8 MB".into()));
        }
        if validation::is_font(&name) && size > validation::MAX_FONT_BYTES {
            return Err(LauncherError::InvalidTheme("fonte excede 4 MB".into()));
        }
        let destination = temp.path().join(&name);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = fs::File::create(destination)?;
        std::io::copy(&mut entry, &mut output)?;
    }
    Ok(temp)
}
fn read_theme(dir: &Path) -> Result<(ThemeManifest, ThemeTokens)> {
    let manifest: ThemeManifest = read_json(&dir.join("manifest.json"), "Manifest inválido")?;
    validation::validate_manifest(
        &manifest,
        &Version::parse(ORBIT_VERSION).expect("package version semver"),
    )?;
    let tokens: ThemeTokens = read_json(&dir.join("theme.json"), "theme.json inválido")?;
    validation::validate_tokens(&tokens)?;
    if let Some(preview) = &manifest.preview {
        let file = dir.join(preview);
        if !file.is_file() {
            return Err(LauncherError::InvalidTheme(
                "arquivo de preview não encontrado".into(),
            ));
        }
    }
    Ok((manifest, tokens))
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path, message: &str) -> Result<T> {
    let text = fs::read_to_string(path).map_err(|_| LauncherError::InvalidTheme(message.into()))?;
    if text.len() > 128 * 1024 {
        return Err(LauncherError::InvalidTheme(format!(
            "{message}: arquivo grande demais"
        )));
    }
    serde_json::from_str(&text).map_err(|_| LauncherError::InvalidTheme(message.into()))
}
fn summary(
    m: &ThemeManifest,
    _: &ThemeTokens,
    source: ThemeSource,
    preview_url: Option<String>,
) -> Result<ThemeSummary> {
    validation::validate_manifest(
        m,
        &Version::parse(ORBIT_VERSION).expect("package version semver"),
    )?;
    Ok(ThemeSummary {
        id: m.id.clone(),
        name: m.name.clone(),
        version: m.version.clone(),
        author: m.author.clone(),
        description: m.description.clone(),
        theme_type: m.theme_type.clone(),
        orbit_version: m.orbit_version.clone(),
        preview_url,
        source,
        compatible: true,
    })
}
fn asset_url(path: PathBuf) -> String {
    format!(
        "asset://localhost/{}",
        path.to_string_lossy().replace('#', "%23")
    )
}
fn walk_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            out.extend(walk_files(&path)?)
        } else if entry.file_type()?.is_file() {
            out.push(path)
        }
    }
    Ok(out)
}
fn zip_error(error: zip::result::ZipError) -> LauncherError {
    LauncherError::Archive(error.to_string())
}

fn builtin_themes() -> Vec<(ThemeManifest, ThemeTokens)> {
    vec![
        builtin(
            "orbit-dark",
            "Orbit Dark",
            "Orbit Team",
            "O visual padrão do Orbit.",
            ThemeType::Dark,
            "#090b10",
            "#11141b",
            "#1c202a",
            "#755be9",
            "#4cc9f0",
            "#f4f5f8",
            "#747a89",
            "#292d38",
        ),
        builtin(
            "orbit-light",
            "Orbit Light",
            "Orbit Team",
            "Tema claro oficial do Orbit.",
            ThemeType::Light,
            "#f5f7fb",
            "#ffffff",
            "#edf0f6",
            "#6046d8",
            "#087ea4",
            "#162033",
            "#5c6472",
            "#d8dde7",
        ),
        builtin(
            "midnight",
            "Midnight",
            "Orbit Team",
            "Tema escuro inspirado em interfaces espaciais.",
            ThemeType::Dark,
            "#0b0d12",
            "#11151c",
            "#181d26",
            "#7c5cff",
            "#4cc9f0",
            "#f5f7fa",
            "#9aa4b2",
            "#252c38",
        ),
        builtin(
            "aurora",
            "Aurora",
            "Orbit Team",
            "Gradientes frios e luminosos.",
            ThemeType::Dark,
            "#09131a",
            "#10212a",
            "#17313b",
            "#39d5bd",
            "#77b7ff",
            "#ecfffb",
            "#9ac0c8",
            "#24505a",
        ),
        builtin(
            "cyber",
            "Cyber",
            "Orbit Team",
            "Contraste neon para sua biblioteca.",
            ThemeType::Dark,
            "#100b1b",
            "#1c1230",
            "#291849",
            "#ff3ea5",
            "#25d9f8",
            "#fff4fc",
            "#c3a8c7",
            "#57366a",
        ),
        builtin(
            "solarized",
            "Solarized",
            "Orbit Team",
            "Paleta Solarized equilibrada.",
            ThemeType::Light,
            "#fdf6e3",
            "#eee8d5",
            "#e5ddc6",
            "#268bd2",
            "#2aa198",
            "#073642",
            "#657b83",
            "#d6cfbb",
        ),
    ]
}
#[allow(clippy::too_many_arguments)] // Compacta a declaração dos temas internos sem adicionar um formato paralelo.
fn builtin(
    id: &str,
    name: &str,
    author: &str,
    description: &str,
    theme_type: ThemeType,
    bg: &str,
    surface: &str,
    elevated: &str,
    primary: &str,
    secondary: &str,
    text: &str,
    muted: &str,
    border: &str,
) -> (ThemeManifest, ThemeTokens) {
    (
        ThemeManifest {
            schema_version: 1,
            id: id.into(),
            name: name.into(),
            version: "1.0.0".into(),
            author: author.into(),
            description: description.into(),
            theme_type,
            orbit_version: ">=0.1.2".into(),
            preview: None,
            entry: "theme.json".into(),
        },
        ThemeTokens {
            colors: Colors {
                background: bg.into(),
                surface: surface.into(),
                surface_elevated: elevated.into(),
                primary: primary.into(),
                secondary: secondary.into(),
                text: text.into(),
                text_muted: muted.into(),
                border: border.into(),
                success: "#4ade80".into(),
                warning: "#facc15".into(),
                error: "#f87171".into(),
                accent: None,
                primary_foreground: None,
                secondary_foreground: None,
                accent_foreground: None,
            },
            radius: Radius {
                small: "6px".into(),
                medium: "10px".into(),
                large: "16px".into(),
            },
            spacing: Spacing { unit: "4px".into() },
            typography: super::manifest::Typography {
                font_family: "Inter, system-ui, sans-serif".into(),
                heading_weight: 700,
                body_weight: 400,
            },
            effects: Effects {
                blur: "12px".into(),
                shadow: "0 8px 32px rgba(0,0,0,0.35)".into(),
            },
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtin_default_is_available() {
        assert_eq!(
            ThemeManager::get("orbit-dark").unwrap().summary.name,
            "Orbit Dark"
        );
    }
}
