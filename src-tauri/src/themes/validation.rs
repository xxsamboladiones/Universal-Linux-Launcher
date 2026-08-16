use super::manifest::{ThemeManifest, ThemeTokens, THEME_SCHEMA_VERSION};
use crate::error::{LauncherError, Result};
use semver::{Version, VersionReq};
use std::path::{Component, Path};

pub const MAX_THEME_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_FILES: usize = 128;
pub const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_FONT_BYTES: u64 = 4 * 1024 * 1024;

pub fn validate_manifest(manifest: &ThemeManifest, orbit_version: &Version) -> Result<()> {
    if manifest.schema_version != THEME_SCHEMA_VERSION {
        return Err(LauncherError::InvalidTheme(format!(
            "schemaVersion {} não é suportado",
            manifest.schema_version
        )));
    }
    if !valid_id(&manifest.id) {
        return Err(LauncherError::InvalidTheme(
            "id deve conter apenas letras minúsculas, números e hífens".into(),
        ));
    }
    for (label, value, limit) in [
        ("nome", &manifest.name, 80),
        ("autor", &manifest.author, 80),
        ("descrição", &manifest.description, 500),
    ] {
        if value.trim().is_empty() || value.len() > limit {
            return Err(LauncherError::InvalidTheme(format!(
                "{label} é obrigatório e excede o limite permitido"
            )));
        }
    }
    Version::parse(&manifest.version)
        .map_err(|_| LauncherError::InvalidTheme("version deve seguir SemVer".into()))?;
    let requirement = VersionReq::parse(&manifest.orbit_version)
        .map_err(|_| LauncherError::InvalidTheme("orbitVersion inválido".into()))?;
    if !requirement.matches(orbit_version) {
        return Err(LauncherError::InvalidTheme(format!(
            "Este tema requer Orbit {}",
            manifest.orbit_version
        )));
    }
    validate_relative(&manifest.entry)?;
    if manifest.entry != "theme.json" {
        return Err(LauncherError::InvalidTheme(
            "entry deve apontar para theme.json".into(),
        ));
    }
    if let Some(preview) = &manifest.preview {
        validate_relative(preview)?;
        if !is_image(preview) {
            return Err(LauncherError::InvalidTheme(
                "preview deve ser PNG ou WebP".into(),
            ));
        }
    }
    Ok(())
}
pub fn validate_tokens(t: &ThemeTokens) -> Result<()> {
    for color in [
        &t.colors.background,
        &t.colors.surface,
        &t.colors.surface_elevated,
        &t.colors.primary,
        &t.colors.secondary,
        &t.colors.text,
        &t.colors.text_muted,
        &t.colors.border,
        &t.colors.success,
        &t.colors.warning,
        &t.colors.error,
    ] {
        if !valid_color(color) {
            return Err(LauncherError::InvalidTheme(format!(
                "cor inválida: {color}"
            )));
        }
    }
    for color in [
        t.colors.accent.as_deref(),
        t.colors.primary_foreground.as_deref(),
        t.colors.secondary_foreground.as_deref(),
        t.colors.accent_foreground.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if !valid_color(color) {
            return Err(LauncherError::InvalidTheme(format!(
                "cor opcional inválida: {color}"
            )));
        }
    }
    for length in [
        &t.radius.small,
        &t.radius.medium,
        &t.radius.large,
        &t.spacing.unit,
        &t.effects.blur,
    ] {
        if !valid_length(length) {
            return Err(LauncherError::InvalidTheme(format!(
                "medida inválida: {length}"
            )));
        }
    }
    if t.typography.font_family.len() > 160
        || t.typography.font_family.contains(['{', '}', ';'])
        || !(100..=900).contains(&t.typography.heading_weight)
        || !(100..=900).contains(&t.typography.body_weight)
    {
        return Err(LauncherError::InvalidTheme("tipografia inválida".into()));
    }
    if t.effects.shadow.len() > 160 || t.effects.shadow.contains(['{', '}', ';', '<', '>']) {
        return Err(LauncherError::InvalidTheme("sombra inválida".into()));
    }
    Ok(())
}
pub fn validate_relative(value: &str) -> Result<()> {
    let p = Path::new(value);
    if value.is_empty()
        || p.is_absolute()
        || p.components().any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(LauncherError::InvalidTheme(
            "caminho contém path traversal ou não é relativo".into(),
        ));
    }
    Ok(())
}
pub fn validate_theme_id(id: &str) -> Result<()> {
    if valid_id(id) {
        Ok(())
    } else {
        Err(LauncherError::InvalidTheme("id de tema inválido".into()))
    }
}
pub fn allowed_file(path: &str) -> bool {
    path == "manifest.json"
        || path == "theme.json"
        || (path.starts_with("assets/") && (is_image(path) || is_font(path)))
        || (path.starts_with("fonts/") && is_font(path))
        || (path.starts_with("preview.") && is_image(path))
}
pub fn is_image(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|e| e.to_str()),
        Some("png" | "webp" | "jpg" | "jpeg")
    )
}
pub fn is_font(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|e| e.to_str()),
        Some("woff" | "woff2" | "ttf")
    )
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !id.starts_with('-')
        && !id.ends_with('-')
}
fn valid_color(value: &str) -> bool {
    let b = value.as_bytes();
    (b.len() == 4 || b.len() == 7 || b.len() == 9)
        && b.first() == Some(&b'#')
        && b[1..].iter().all(u8::is_ascii_hexdigit)
}
fn valid_length(value: &str) -> bool {
    value.len() <= 20
        && ["px", "rem", "em", "%"].iter().any(|s| {
            value
                .strip_suffix(s)
                .is_some_and(|n| n.parse::<f32>().is_ok_and(|x| (0.0..=1000.0).contains(&x)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::manifest::*;
    fn manifest() -> ThemeManifest {
        ThemeManifest {
            schema_version: 1,
            id: "valid-theme".into(),
            name: "Valid".into(),
            version: "1.0.0".into(),
            author: "Orbit".into(),
            description: "Test".into(),
            theme_type: ThemeType::Dark,
            orbit_version: ">=0.1.2".into(),
            preview: Some("preview.png".into()),
            entry: "theme.json".into(),
        }
    }
    #[test]
    fn validates_manifest() {
        assert!(validate_manifest(&manifest(), &Version::parse("0.1.2").unwrap()).is_ok());
    }
    #[test]
    fn rejects_path_traversal_and_incompatible_versions() {
        let mut m = manifest();
        m.entry = "../theme.json".into();
        assert!(validate_manifest(&m, &Version::parse("0.1.2").unwrap()).is_err());
        m.entry = "theme.json".into();
        m.orbit_version = ">=9.0.0".into();
        assert!(validate_manifest(&m, &Version::parse("0.1.2").unwrap()).is_err());
    }
    #[test]
    fn blocks_unsafe_files() {
        assert!(!allowed_file("evil.js"));
        assert!(!allowed_file("assets/../../run.sh"));
        assert!(allowed_file("assets/card.webp"));
    }
}
