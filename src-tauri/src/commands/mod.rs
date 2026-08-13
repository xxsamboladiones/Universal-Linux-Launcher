use std::{collections::HashMap, path::PathBuf, sync::Mutex};

use tauri::{AppHandle, Emitter, Manager, State, Window};

use crate::{
    application::{ScanProgress, ScanReport},
    core::{
        compatibility::{self, CompatibilityOverview},
        launch,
        model::{AppSettings, ItemInput, ItemKind, LibraryItem, ProviderKind},
    },
    database::Database,
    error::{LauncherError, Result},
    platform::{self, PlatformOverview},
    process::ProcessManager,
    product::{self, ProductStatus},
    providers,
};

pub struct AppState {
    pub database: Mutex<Database>,
    pub data_dir: PathBuf,
    pub process_manager: ProcessManager,
}

#[tauri::command]
pub fn get_library(window: Window, state: State<AppState>) -> Result<Vec<LibraryItem>> {
    let items = state
        .database
        .lock()
        .expect("database lock poisoned")
        .list()?;
    let scope = window.asset_protocol_scope();
    for path in items
        .iter()
        .flat_map(|item| [&item.icon, &item.cover, &item.background])
        .flatten()
    {
        let path = std::path::Path::new(path);
        if path.is_file() {
            if let Err(error) = scope.allow_file(path) {
                tracing::warn!(%error, path=%path.display(), "Não foi possível autorizar imagem local");
            }
        }
    }
    Ok(items)
}

#[tauri::command]
pub async fn scan_providers(window: Window, state: State<'_, AppState>) -> Result<ScanReport> {
    let results = tauri::async_runtime::spawn_blocking(move || {
        let handles = providers::defaults()
            .into_iter()
            .map(|provider| {
                let window = window.clone();
                std::thread::spawn(move || {
                    let name = provider.name().to_string();
                    if !provider.is_available() {
                        return (name, None);
                    }
                    let _ = window.emit(
                        "scan-progress",
                        ScanProgress {
                            provider: name.clone(),
                            status: "scanning".into(),
                            found: 0,
                        },
                    );
                    let result = provider.scan();
                    let found = result.as_ref().map_or(0, Vec::len);
                    let _ = window.emit(
                        "scan-progress",
                        ScanProgress {
                            provider: name.clone(),
                            status: "completed".into(),
                            found,
                        },
                    );
                    (name, Some(result))
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|error| LauncherError::LaunchFailed(error.to_string()))?;

    let mut report = ScanReport::default();
    let mut database = state.database.lock().expect("database lock poisoned");
    for (provider, result) in results {
        let Some(result) = result else {
            report.unavailable.push(provider);
            continue;
        };
        match result {
            Ok(items) => {
                report.found += items.len();
                match database.apply_provider_scan(&provider, &items) {
                    Ok((added, updated)) => {
                        report.added += added;
                        report.updated += updated;
                    }
                    Err(error) => report.errors.push(format!("{provider}: {error}")),
                }
            }
            Err(error) => report.errors.push(format!("{provider}: {error}")),
        }
    }
    Ok(report)
}

#[tauri::command]
pub fn launch_item(id: String, state: State<AppState>) -> Result<u32> {
    let item = state
        .database
        .lock()
        .expect("database lock poisoned")
        .get(&id)?
        .ok_or_else(|| LauncherError::NotFound(id.clone()))?;
    let settings = state
        .database
        .lock()
        .expect("database lock poisoned")
        .settings()?;
    let mut spec = launch::resolve(&item, settings.preferred_terminal.as_deref())?;
    let notes = compatibility::apply(&mut spec, &item.compatibility, &state.data_dir, &id)?;
    let log_path = state.data_dir.join("logs/compatibility").join(format!(
        "{}.log",
        id.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
    ));
    if !notes.is_empty() {
        std::fs::create_dir_all(log_path.parent().expect("log parent"))?;
        std::fs::write(
            &log_path,
            format!("Configuração de compatibilidade\n{}\n", notes.join("\n")),
        )?;
    }
    let child = launch::spawn(&spec, Some(&log_path))?;
    let pid = child.id();
    let session = state
        .database
        .lock()
        .expect("database lock poisoned")
        .start_session(&id, pid)?;
    state.process_manager.track(id, session, child);
    Ok(pid)
}

#[tauri::command]
pub fn get_running_items(state: State<AppState>) -> HashMap<String, u32> {
    state.process_manager.running()
}

#[tauri::command]
pub fn set_favorite(id: String, value: bool, state: State<AppState>) -> Result<()> {
    state
        .database
        .lock()
        .expect("database lock poisoned")
        .flag(&id, "favorite", value)
}

#[tauri::command]
pub fn set_hidden(id: String, value: bool, state: State<AppState>) -> Result<()> {
    state
        .database
        .lock()
        .expect("database lock poisoned")
        .flag(&id, "hidden", value)
}

#[tauri::command]
pub fn delete_item(id: String, state: State<AppState>) -> Result<()> {
    state
        .database
        .lock()
        .expect("database lock poisoned")
        .delete(&id)
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<AppSettings> {
    state
        .database
        .lock()
        .expect("database lock poisoned")
        .settings()
}

#[tauri::command]
pub fn update_settings(settings: AppSettings, state: State<AppState>) -> Result<()> {
    state
        .database
        .lock()
        .expect("database lock poisoned")
        .save_settings(&settings)
}

#[tauri::command]
pub fn update_item(item: ItemInput, state: State<AppState>) -> Result<LibraryItem> {
    let id = item
        .id
        .unwrap_or_else(|| format!("custom:{}", uuid::Uuid::new_v4()));
    let mut saved = LibraryItem::new(id, item.name, item.kind, item.provider);
    saved.executable = item.executable;
    saved.arguments = item.arguments;
    saved.working_directory = item.working_directory;
    saved.environment = item.environment;
    saved.icon = item.icon;
    saved.category = item.category;
    saved.terminal = item.terminal;
    saved.compatibility = item.compatibility;
    state
        .database
        .lock()
        .expect("database lock poisoned")
        .save_user_item(&saved)?;
    Ok(saved)
}

#[tauri::command]
pub fn get_platform_overview(state: State<AppState>) -> PlatformOverview {
    platform::overview(
        &state.data_dir,
        &state.database.lock().expect("database lock poisoned"),
    )
}

#[tauri::command]
pub async fn prepare_provider(
    provider: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let dependencies: &[&str] = match provider.as_str() {
        "steam" => &["steamcmd"],
        "epic" => &["legendary"],
        "battlenet" => &["wine-ge", "battlenet-client"],
        "gog" => &["gogdl"],
        _ => return Err(LauncherError::ProviderUnavailable(provider)),
    };
    let dependencies = dependencies
        .iter()
        .map(|dependency| (*dependency).to_string())
        .collect::<Vec<_>>();
    let data_dir = state.data_dir.clone();
    let selected_provider = provider.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let manager = platform::DependencyManager::new(data_dir);
        for dependency in dependencies {
            manager.install_with_progress(&dependency, |progress| {
                let _ = app.emit(
                    "dependency-progress",
                    serde_json::json!({
                        "provider": selected_provider,
                        "dependency": progress.dependency,
                        "stage": progress.stage,
                        "downloadedBytes": progress.downloaded_bytes,
                        "totalBytes": progress.total_bytes,
                    }),
                );
            })?;
        }
        Ok(())
    })
    .await
    .map_err(|error| LauncherError::LaunchFailed(error.to_string()))?
}

#[tauri::command]
pub fn rollback_dependency(id: String, state: State<AppState>) -> Result<()> {
    platform::DependencyManager::new(state.data_dir.clone()).rollback(&id)
}

#[tauri::command]
pub fn connect_provider(
    provider: String,
    user: Option<String>,
    state: State<AppState>,
) -> Result<()> {
    let (command, args) = platform::ProviderManager::new(state.data_dir.clone())
        .authenticate_command(&provider, user.as_deref())?;
    let settings = state
        .database
        .lock()
        .expect("database lock poisoned")
        .settings()?;
    let terminal = settings
        .preferred_terminal
        .unwrap_or_else(|| "konsole".into());
    std::process::Command::new(terminal)
        .arg("-e")
        .arg(command)
        .args(args)
        .spawn()
        .map_err(|e| LauncherError::LaunchFailed(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn store_provider_token(provider: String, token: String) -> Result<()> {
    platform::CredentialVault::detect().store(&provider, &token)
}

#[tauri::command]
pub fn queue_store_operation(
    provider: String,
    item_id: String,
    action: String,
    app: AppHandle,
    state: State<AppState>,
) -> Result<String> {
    if !["install", "update", "verify"].contains(&action.as_str()) {
        return Err(LauncherError::InvalidArguments(action));
    }
    let operation = platform::ProviderManager::operation(&provider, &item_id, &action);
    state
        .database
        .lock()
        .expect("database lock poisoned")
        .queue_operation(&operation)?;
    let id = operation.id.clone();
    let db_path = state.data_dir.join("orbit.db");
    let data = state.data_dir.clone();
    std::thread::spawn(move || run_store_operation(app, db_path, data, operation));
    Ok(id)
}

fn run_store_operation(
    app: AppHandle,
    db_path: PathBuf,
    data: PathBuf,
    operation: platform::Operation,
) {
    let update = |state: &str, error: Option<&str>| {
        if let Ok(db) = Database::open(&db_path) {
            let _ = db.update_operation(&operation.id, state, 0, 0, 0, error);
        }
        let mut changed = operation.clone();
        changed.state = state.into();
        changed.error = error.map(str::to_string);
        let _ = app.emit("transfer-progress", changed);
    };
    update("running", None);
    let manager = platform::DependencyManager::new(data.clone());
    let command = match operation.provider.as_str() {
        "epic" => manager.executable("legendary").map(|exe| {
            (
                exe,
                vec![
                    operation.action.clone(),
                    operation.item_id.clone(),
                    "--base-path".into(),
                    data.join("games/epic").to_string_lossy().into_owned(),
                ],
            )
        }),
        "steam" => manager.executable("steamcmd").map(|exe| {
            (
                exe,
                vec![
                    "+login".into(),
                    "anonymous".into(),
                    "+app_update".into(),
                    operation.item_id.clone(),
                    "validate".into(),
                    "+quit".into(),
                ],
            )
        }),
        "gog" => manager.executable("gogdl").map(|exe| {
            (
                exe,
                vec![
                    "download".into(),
                    operation.item_id.clone(),
                    "--path".into(),
                    data.join("games/gog").to_string_lossy().into_owned(),
                ],
            )
        }),
        "battlenet" => Some((
            manager
                .executable("wine-ge")
                .unwrap_or_else(|| PathBuf::from("wine")),
            vec![data
                .join("providers/battlenet/Battle.net-Setup.exe")
                .to_string_lossy()
                .into_owned()],
        )),
        _ => None,
    };
    let result = command
        .ok_or_else(|| "Componente do provider não instalado".to_string())
        .and_then(|(exe, args)| {
            let mut process = std::process::Command::new(exe);
            process.args(args);
            if operation.provider == "battlenet" {
                let prefix = data.join("prefixes/battlenet");
                let _ = std::fs::create_dir_all(&prefix);
                process.env("WINEPREFIX", prefix);
            }
            process.status().map_err(|e| e.to_string())
        })
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(format!("Processo terminou com {status}"))
            }
        });
    match result {
        Ok(()) => update("completed", None),
        Err(error) => update("failed", Some(&error)),
    }
}

#[tauri::command]
pub fn retry_operation(id: String, app: AppHandle, state: State<AppState>) -> Result<()> {
    let operation = state
        .database
        .lock()
        .expect("database lock poisoned")
        .operations()?
        .into_iter()
        .find(|op| op.id == id)
        .ok_or_else(|| LauncherError::NotFound(id.clone()))?;
    state
        .database
        .lock()
        .expect("database lock poisoned")
        .update_operation(
            &id,
            "queued",
            operation.downloaded_bytes,
            operation.total_bytes,
            0,
            None,
        )?;
    let db_path = state.data_dir.join("orbit.db");
    let data = state.data_dir.clone();
    std::thread::spawn(move || run_store_operation(app, db_path, data, operation));
    Ok(())
}

#[tauri::command]
pub fn sync_store_library(provider: String, state: State<AppState>) -> Result<usize> {
    if provider != "epic" {
        return Err(LauncherError::ProviderUnavailable(format!(
            "Sincronização do catálogo {provider} ainda não está disponível"
        )));
    }
    let executable = platform::DependencyManager::new(state.data_dir.clone())
        .executable("legendary")
        .ok_or_else(|| LauncherError::ExecutableNotFound("legendary".into()))?;
    let output = std::process::Command::new(executable)
        .args(["list", "--json"])
        .output()?;
    if !output.status.success() {
        return Err(LauncherError::ProviderUnavailable(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    let values: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)
        .map_err(|error| LauncherError::ProviderUnavailable(error.to_string()))?;
    let items = values
        .into_iter()
        .filter_map(|value| {
            let app = value
                .get("app_name")
                .or_else(|| value.get("appName"))?
                .as_str()?;
            let name = value.get("title").and_then(|v| v.as_str()).unwrap_or(app);
            let mut item = LibraryItem::new(
                format!("epic:{app}"),
                name.into(),
                ItemKind::Game,
                ProviderKind::Epic,
            );
            item.executable = Some("legendary".into());
            item.category = Some("Epic Games".into());
            item.installed = value
                .get("is_installed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(item)
        })
        .collect::<Vec<_>>();
    let count = items.len();
    state
        .database
        .lock()
        .expect("database lock poisoned")
        .apply_provider_scan("epic", &items)?;
    Ok(count)
}

#[tauri::command]
pub fn get_compatibility_overview(state: State<AppState>) -> CompatibilityOverview {
    compatibility::overview(&state.data_dir)
}

#[tauri::command]
pub fn create_game_prefix(id: String, state: State<AppState>) -> Result<String> {
    compatibility::create_prefix(&state.data_dir, &id)
}

#[tauri::command]
pub fn open_path(path: String) -> Result<()> {
    let target = std::path::Path::new(&path);
    if !target.exists() {
        return Err(LauncherError::NotFound(path));
    }
    std::process::Command::new("xdg-open")
        .arg(target)
        .spawn()
        .map_err(|e| LauncherError::LaunchFailed(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn open_compatibility_log(id: String, state: State<AppState>) -> Result<()> {
    let safe: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = state
        .data_dir
        .join("logs/compatibility")
        .join(format!("{safe}.log"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        std::fs::write(&path, "Nenhuma execução de compatibilidade registrada.\n")?;
    }
    std::process::Command::new("xdg-open")
        .arg(&path)
        .spawn()
        .map_err(|e| LauncherError::LaunchFailed(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn get_product_status() -> ProductStatus {
    product::status()
}

#[tauri::command]
pub fn set_autostart(enabled: bool) -> Result<()> {
    product::set_autostart(enabled)
}

#[tauri::command]
pub fn export_backup(path: String, state: State<AppState>) -> Result<()> {
    product::export_backup(
        &state.database.lock().expect("database lock poisoned"),
        &state.data_dir,
        std::path::Path::new(&path),
    )
}

#[tauri::command]
pub fn import_backup(path: String, state: State<AppState>) -> Result<()> {
    product::import_backup(
        &mut state.database.lock().expect("database lock poisoned"),
        std::path::Path::new(&path),
    )
}

#[tauri::command]
pub fn check_for_updates() -> Result<product::UpdateStatus> {
    product::check_update()
}

#[tauri::command]
pub fn install_update() -> Result<()> {
    product::install_update()
}
