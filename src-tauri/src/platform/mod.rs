mod phase3;

use crate::database::Database;
pub(crate) use phase3::parse_transfer_progress;
pub use phase3::{CredentialVault, DependencyManager, ProviderManager, GOG_LOGIN_URL};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
                installed_version: path.as_ref().and_then(|_| manager.installed_version(id)),
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
    ]
    .into_iter()
    .map(|(id, name, description, strategy, deps)| {
        let ready = deps.iter().all(|dep| manager.executable(dep).is_some());
        StoreAccount {
            provider: id.into(),
            display_name: name.into(),
            description: description.into(),
            state: connection_state(ready, provider_connected(root, db, id)),
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
fn provider_connected(root: &Path, db: &Database, provider: &str) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    if provider == "steam" {
        return connected_provider_user(root, db, provider).is_some();
    }
    if provider == "gog" {
        return ProviderManager::new(root.to_path_buf()).gog_authenticated();
    }
    if db
        .provider_account(provider)
        .ok()
        .flatten()
        .is_some_and(|account| account.state == "connected")
    {
        return true;
    }
    match provider {
        "epic" => home.join(".config/legendary/user.json").is_file(),
        "gog" => false,
        _ => false,
    }
}

/// Returns only the public account name saved by Orbit. Steam's refresh token
/// remains exclusively in Steam's own `config.vdf` and is never copied into
/// Orbit's database.
pub(crate) fn connected_provider_user(
    root: &Path,
    db: &Database,
    provider: &str,
) -> Option<String> {
    let manager = ProviderManager::new(root.to_path_buf());
    if provider == "steam" && !manager.steam_login_cache_exists() {
        return None;
    }
    if let Some(account) = db.provider_account(provider).ok().flatten() {
        if account.state != "connected" {
            return None;
        }
        if account.display_name.is_some() || provider != "steam" {
            return account.display_name;
        }
    } else if provider != "steam" {
        return None;
    }

    // v0.1.1 opened SteamCMD successfully, but checked a non-existent
    // `current/config/loginusers.vdf`. Recover that already authenticated
    // account from SteamCMD's own success transcript and persist only its
    // non-secret username.
    let account = manager.steam_account_from_log()?;
    if let Err(error) = db.upsert_provider_account("steam", "connected", Some(&account)) {
        tracing::warn!(%error, "não foi possível migrar a sessão SteamCMD existente");
    }
    Some(account)
}
pub(crate) fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?
        .to_string_lossy()
        .split(':')
        .map(Path::new)
        .map(|p| p.join(name))
        .find(|p| p.is_file())
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
