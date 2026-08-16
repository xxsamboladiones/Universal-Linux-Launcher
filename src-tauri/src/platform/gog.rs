use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::{
    core::model::{ItemKind, LibraryItem, ProviderKind},
    error::{LauncherError, Result},
};

pub(crate) const MAX_OWNED_GAMES: usize = 5_000;

#[derive(Debug, Deserialize)]
pub(crate) struct GogCredentials {
    pub(crate) access_token: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GogProduct {
    id: serde_json::Value,
    title: String,
    is_installable: Option<bool>,
    game_type: String,
    images: GogImages,
    content_system_compatibility: GogCompatibility,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GogImages {
    background: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GogCompatibility {
    linux: bool,
}

pub(crate) fn parse_credentials(output: &[u8]) -> Result<GogCredentials> {
    let credentials: Option<GogCredentials> = serde_json::from_slice(output).map_err(|_| {
        LauncherError::ProviderUnavailable(
            "O GOGDL retornou credenciais inválidas. Conecte a conta novamente".into(),
        )
    })?;
    let credentials = credentials.ok_or_else(|| {
        LauncherError::ProviderUnavailable(
            "A sessão GOG expirou. Conecte a conta novamente e repita a sincronização".into(),
        )
    })?;
    if credentials.access_token.is_empty()
        || credentials.access_token.len() > 8_192
        || !credentials.access_token.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '-' | '.' | '_' | '~' | '+' | '/' | '=')
        })
    {
        return Err(LauncherError::ProviderUnavailable(
            "O GOGDL retornou uma sessão inválida. Conecte a conta novamente".into(),
        ));
    }
    Ok(credentials)
}

pub(crate) fn parse_owned_game_ids(output: &[u8]) -> Result<Vec<String>> {
    let document: serde_json::Value = serde_json::from_slice(output).map_err(|error| {
        LauncherError::ProviderUnavailable(format!(
            "O GOG retornou uma biblioteca inválida: {error}"
        ))
    })?;
    let owned = document
        .get("owned")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            LauncherError::ProviderUnavailable(
                "O GOG retornou uma biblioteca sem a lista de jogos".into(),
            )
        })?;
    if owned.len() > MAX_OWNED_GAMES {
        return Err(LauncherError::ProviderUnavailable(
            "A biblioteca GOG excedeu o limite de segurança".into(),
        ));
    }
    let mut seen = HashSet::with_capacity(owned.len());
    owned
        .iter()
        .map(|value| {
            let id = match value {
                serde_json::Value::Number(number) => number.to_string(),
                serde_json::Value::String(value) => value.trim().to_string(),
                _ => String::new(),
            };
            validate_product_id(&id)?;
            if !seen.insert(id.clone()) {
                return Err(LauncherError::ProviderUnavailable(
                    "O GOG retornou um identificador de jogo duplicado".into(),
                ));
            }
            Ok(id)
        })
        .collect()
}

pub(crate) fn catalog_item(
    product_id: &str,
    metadata: Option<&[u8]>,
    executable: &Path,
    auth_path: &Path,
    data_dir: &Path,
) -> Result<LibraryItem> {
    validate_product_id(product_id)?;
    let product = metadata.and_then(|bytes| serde_json::from_slice::<GogProduct>(bytes).ok());
    let verified_product = product.as_ref().filter(|product| {
        json_product_id(&product.id).as_deref() == Some(product_id)
            && product.title.len() <= 4_096
            && !product.title.contains('\0')
    });
    let title = verified_product
        .map(|product| product.title.trim())
        .filter(|title| !title.is_empty())
        .unwrap_or(product_id)
        .to_string();
    let platform =
        if verified_product.is_some_and(|product| product.content_system_compatibility.linux) {
            "linux"
        } else {
            "windows"
        };
    let install_path = install_path(data_dir, product_id)?;
    let mut item = LibraryItem::new(
        format!("gog:{product_id}"),
        title,
        ItemKind::Game,
        ProviderKind::Gog,
    );
    item.executable = Some(executable.to_string_lossy().into_owned());
    item.arguments = vec![
        "--auth-config-path".into(),
        auth_path.to_string_lossy().into_owned(),
        "launch".into(),
        install_path.to_string_lossy().into_owned(),
        product_id.into(),
        "--platform".into(),
        platform.into(),
    ];
    if platform == "linux" {
        item.arguments.push("--no-wine".into());
    }
    item.category = Some("GOG".into());
    item.tags = vec![format!("orbit:gog-platform:{platform}")];
    item.cover = verified_product.and_then(|product| safe_image_url(&product.images.background));
    item.owned = true;
    item.installed = valid_installation(&install_path, product_id, platform);
    item.working_directory = item
        .installed
        .then(|| install_path.to_string_lossy().into_owned());
    Ok(item)
}

pub(crate) fn is_installable_metadata(metadata: &[u8]) -> bool {
    serde_json::from_slice::<GogProduct>(metadata).map_or(true, |product| {
        (product.game_type.is_empty() || matches!(product.game_type.as_str(), "game" | "mod"))
            && product.is_installable != Some(false)
    })
}

pub(crate) fn platform_from_item(item: &LibraryItem) -> &'static str {
    if item
        .tags
        .iter()
        .any(|tag| tag == "orbit:gog-platform:linux")
    {
        "linux"
    } else {
        "windows"
    }
}

pub(crate) fn install_path(data_dir: &Path, product_id: &str) -> Result<PathBuf> {
    validate_product_id(product_id)?;
    Ok(data_dir.join("games/gog").join(product_id))
}

pub(crate) fn valid_installation(path: &Path, product_id: &str, platform: &str) -> bool {
    if !path.is_dir()
        || std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return false;
    }
    let candidates = if platform == "linux" {
        vec![
            path.join("game").join(format!("goggame-{product_id}.info")),
            path.join("gameinfo"),
        ]
    } else {
        vec![path.join(format!("goggame-{product_id}.info"))]
    };
    candidates.into_iter().any(|marker| {
        marker.is_file()
            && std::fs::symlink_metadata(marker)
                .is_ok_and(|metadata| !metadata.file_type().is_symlink())
    })
}

pub(crate) fn validate_product_id(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 32 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(LauncherError::ProviderUnavailable(
            "O GOG retornou um identificador de jogo inválido".into(),
        ));
    }
    Ok(())
}

fn json_product_id(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::String(value) => Some(value.trim().to_string()),
        _ => None,
    }
}

fn safe_image_url(value: &str) -> Option<String> {
    let value = value.trim();
    let normalized = value
        .strip_prefix("//")
        .map(|value| format!("https://{value}"))
        .unwrap_or_else(|| value.to_string());
    let remainder = normalized.strip_prefix("https://images-")?;
    let (shard, path) = remainder.split_once(".gog-statics.com/")?;
    if !matches!(shard, "1" | "2" | "3" | "4")
        || path.is_empty()
        || normalized.len() > 2_048
        || normalized.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || matches!(character, '(' | ')' | '"' | '\\')
        })
    {
        return None;
    }
    Some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_owned_ids_and_rejects_traversal_or_duplicates() {
        assert_eq!(
            parse_owned_game_ids(br#"{"owned":[1207658997,"12345"]}"#).unwrap(),
            ["1207658997", "12345"]
        );
        assert!(parse_owned_game_ids(br#"{"owned":["../game"]}"#).is_err());
        assert!(parse_owned_game_ids(br#"{"owned":[42,"42"]}"#).is_err());
    }

    #[test]
    fn builds_a_managed_catalog_item_and_trusts_only_gog_images() {
        let root = tempfile::tempdir().unwrap();
        let metadata = br#"{
          "id":1207658997,"title":"Thief Gold","is_installable":true,"game_type":"game",
          "content_system_compatibility":{"linux":false},
          "images":{"background":"//images-3.gog-statics.com/hash.jpg"}
        }"#;
        let item = catalog_item(
            "1207658997",
            Some(metadata),
            Path::new("/managed/gogdl"),
            Path::new("/orbit/auth.json"),
            root.path(),
        )
        .unwrap();
        assert_eq!(item.id, "gog:1207658997");
        assert_eq!(item.name, "Thief Gold");
        assert_eq!(
            item.cover.as_deref(),
            Some("https://images-3.gog-statics.com/hash.jpg")
        );
        assert_eq!(item.arguments[2], "launch");
        assert_eq!(item.arguments[5..], ["--platform", "windows"]);
        assert!(!item.installed);

        let evil = br#"{
          "id":1207658997,"title":"Safe title","is_installable":true,"game_type":"game",
          "images":{"background":"//images-3.gog-statics.com.evil.test/hash.jpg"}
        }"#;
        assert_eq!(
            catalog_item(
                "1207658997",
                Some(evil),
                Path::new("gogdl"),
                Path::new("auth"),
                root.path()
            )
            .unwrap()
            .cover,
            None
        );
    }

    #[test]
    fn installation_requires_a_real_provider_marker() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("1207658997");
        std::fs::create_dir(&path).unwrap();
        assert!(!valid_installation(&path, "1207658997", "windows"));
        std::fs::write(path.join("goggame-1207658997.info"), "{}").unwrap();
        assert!(valid_installation(&path, "1207658997", "windows"));
    }

    #[test]
    fn credentials_are_bounded_and_cannot_inject_curl_headers() {
        assert_eq!(
            parse_credentials(br#"{"access_token":"token-._~+/="}"#)
                .unwrap()
                .access_token,
            "token-._~+/="
        );
        assert!(parse_credentials(b"null").is_err());
        assert!(parse_credentials(br#"{"access_token":"token\nInjected: yes"}"#).is_err());
    }
}
