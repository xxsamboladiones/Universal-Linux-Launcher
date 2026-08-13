mod phase3;

use crate::database::Database;
pub use phase3::{CredentialVault, DependencyManager, ProviderManager};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Disconnected,
    ComponentRequired,
    Connected,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyState {
    Missing,
    Installed,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreAccount {
    pub provider: String,
    pub display_name: String,
    pub description: String,
    pub state: ConnectionState,
    pub library_size: u64,
    pub dependency_ids: Vec<String>,
    pub strategy: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedDependency {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub state: DependencyState,
    pub installed_version: Option<String>,
    pub required_disk_bytes: u64,
    pub executable: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeVersion {
    pub id: String,
    pub family: String,
    pub name: String,
    pub version: String,
    pub installed: bool,
    pub source: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub id: String,
    pub provider: String,
    pub item_id: String,
    pub action: String,
    pub state: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub bytes_per_second: u64,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformOverview {
    pub accounts: Vec<StoreAccount>,
    pub dependencies: Vec<ManagedDependency>,
    pub runtimes: Vec<RuntimeVersion>,
    pub operations: Vec<Operation>,
    pub credential_store: String,
}

pub fn overview(root: &Path, db: &Database) -> PlatformOverview {
    let manager = DependencyManager::new(root.to_path_buf());
    let vault = CredentialVault::detect();
    let specs = [
        ("steamcmd", "SteamCMD", "steam", 192_000_000),
        ("legendary", "Legendary", "epic", 55_000_000),
        ("wine-ge", "Wine-GE", "compatibility", 680_000_000),
        (
            "battlenet-client",
            "Battle.net Client",
            "battlenet",
            500_000_000,
        ),
        ("gogdl", "GOGDL", "gog", 30_000_000),
    ];
    let dependencies = specs
        .iter()
        .map(|(id, name, provider, size)| {
            let path = manager.executable(id);
            ManagedDependency {
                id: (*id).into(),
                name: (*name).into(),
                provider: (*provider).into(),
                state: if path.is_some() {
                    DependencyState::Installed
                } else {
                    DependencyState::Missing
                },
                installed_version: path.as_ref().and_then(|_| detect_version(id)),
                required_disk_bytes: *size,
                executable: path.map(|p| p.to_string_lossy().into_owned()),
            }
        })
        .collect();
    let accounts = [
        (
            "steam",
            "Steam",
            "SteamCMD com autenticação interativa e Steam Guard.",
            "native",
            vec!["steamcmd"],
        ),
        (
            "epic",
            "Epic Games",
            "Biblioteca, downloads e execução por Legendary.",
            "replacement",
            vec!["legendary"],
        ),
        (
            "gog",
            "GOG",
            "Biblioteca e downloads DRM-free por GOGDL.",
            "replacement",
            vec!["gogdl"],
        ),
        (
            "battlenet",
            "Battle.net",
            "Cliente oficial isolado em prefixo Wine gerenciado.",
            "managed_client",
            vec!["wine-ge", "battlenet-client"],
        ),
    ]
    .into_iter()
    .map(|(id, name, description, strategy, deps)| {
        let ready = deps.iter().all(|dep| manager.executable(dep).is_some());
        StoreAccount {
            provider: id.into(),
            display_name: name.into(),
            description: description.into(),
            state: connection_state(ready, provider_connected(root, id)),
            library_size: 0,
            dependency_ids: deps.into_iter().map(str::to_string).collect(),
            strategy: strategy.into(),
        }
    })
    .collect();
    PlatformOverview {
        accounts,
        dependencies,
        runtimes: vec![],
        operations: db.operations().unwrap_or_default(),
        credential_store: vault.name().into(),
    }
}
fn connection_state(component_ready: bool, authenticated: bool) -> ConnectionState {
    if !component_ready {
        ConnectionState::ComponentRequired
    } else if authenticated {
        ConnectionState::Connected
    } else {
        ConnectionState::Disconnected
    }
}
fn provider_connected(root: &Path, provider: &str) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    match provider {
        "epic" => home.join(".config/legendary/user.json").is_file(),
        "steam" => {
            home.join(".steam/steam/config/loginusers.vdf").is_file()
                || home
                    .join(".local/share/Steam/config/loginusers.vdf")
                    .is_file()
        }
        "gog" => home.join(".config/heroic/gog_store/auth.json").is_file(),
        "battlenet" => root.join("prefixes/battlenet").is_dir(),
        _ => false,
    }
}
pub(crate) fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?
        .to_string_lossy()
        .split(':')
        .map(Path::new)
        .map(|p| p.join(name))
        .find(|p| p.is_file())
}
fn detect_version(binary: &str) -> Option<String> {
    let output = Command::new(binary).arg("--version").output().ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(|x| x.trim().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticated_steam_without_managed_steamcmd_requires_component() {
        assert!(matches!(
            connection_state(false, true),
            ConnectionState::ComponentRequired
        ));
    }
}
