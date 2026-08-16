use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter, Manager, State, Window};

#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt;
#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicI32;

use crate::providers::LibraryProvider;
use crate::{
    application::{ScanProgress, ScanReport},
    core::{
        compatibility::{self, CompatibilityOverview},
        icon, launch,
        model::{AppSettings, ItemInput, ItemKind, LibraryItem, ProviderKind},
    },
    database::Database,
    error::{LauncherError, Result},
    platform::{self, PlatformOverview},
    process::ProcessManager,
    product::{self, ProductStatus},
    providers,
    themes::{ThemeDetails, ThemeManager, ThemeSummary},
};

pub struct AppState {
    pub database: Mutex<Database>,
    pub data_dir: PathBuf,
    pub process_manager: ProcessManager,
    pub transfer_manager: TransferManager,
    pub library_sync_manager: LibrarySyncManager,
}

#[derive(Clone, Default)]
pub struct LibrarySyncManager {
    active: Arc<Mutex<HashMap<String, Arc<LibrarySyncControl>>>>,
    shutting_down: Arc<AtomicBool>,
}

struct LibrarySyncGuard {
    provider: String,
    active: Arc<Mutex<HashMap<String, Arc<LibrarySyncControl>>>>,
    control: Arc<LibrarySyncControl>,
}

struct LibrarySyncControl {
    cancelled: AtomicBool,
    #[cfg(target_os = "linux")]
    process_group: AtomicI32,
}

impl LibrarySyncManager {
    fn begin(&self, provider: &str) -> Result<LibrarySyncGuard> {
        let mut active = self.active.lock().expect("library sync lock poisoned");
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(LauncherError::InvalidArguments(
                "O Orbit está sendo encerrado e não pode sincronizar bibliotecas".into(),
            ));
        }
        if active.contains_key(provider) {
            return Err(LauncherError::InvalidArguments(format!(
                "A biblioteca {provider} já está sendo sincronizada"
            )));
        }
        let control = Arc::new(LibrarySyncControl::new());
        active.insert(provider.to_string(), control.clone());
        Ok(LibrarySyncGuard {
            provider: provider.to_string(),
            active: self.active.clone(),
            control,
        })
    }

    pub(crate) fn cancel_all(&self) {
        let controls = {
            let active = self.active.lock().expect("library sync lock poisoned");
            self.shutting_down.store(true, Ordering::Release);
            active.values().cloned().collect::<Vec<_>>()
        };
        for control in controls {
            control.cancel();
        }
    }
}

impl LibrarySyncControl {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            #[cfg(target_os = "linux")]
            process_group: AtomicI32::new(0),
        }
    }

    fn register_process(&self, process_id: u32) {
        #[cfg(target_os = "linux")]
        {
            self.process_group
                .store(process_id as i32, Ordering::Release);
            if self.cancelled.load(Ordering::Acquire) {
                signal_process_group(process_id as i32, libc::SIGKILL);
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = process_id;
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        #[cfg(target_os = "linux")]
        {
            let process_group = self.process_group.load(Ordering::Acquire);
            if process_group > 0 {
                signal_process_group(process_group, libc::SIGKILL);
            }
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn clear_process(&self) {
        #[cfg(target_os = "linux")]
        self.process_group.store(0, Ordering::Release);
    }
}

impl Drop for LibrarySyncGuard {
    fn drop(&mut self) {
        self.active
            .lock()
            .expect("library sync lock poisoned")
            .remove(&self.provider);
    }
}

#[derive(Clone, Default)]
pub struct TransferManager {
    active: Arc<Mutex<HashMap<String, Arc<TransferControl>>>>,
    shutting_down: Arc<AtomicBool>,
}

struct TransferControl {
    // 0 = ativo, 1 = cancelamento normal, 2 = encerramento forçado do app.
    cancellation: AtomicU8,
    #[cfg(target_os = "linux")]
    process_group: AtomicI32,
}

impl TransferControl {
    fn new() -> Self {
        Self {
            cancellation: AtomicU8::new(0),
            #[cfg(target_os = "linux")]
            process_group: AtomicI32::new(0),
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancellation.load(Ordering::Acquire) != 0
    }

    fn register_process(&self, process_id: u32) {
        #[cfg(target_os = "linux")]
        {
            self.process_group
                .store(process_id as i32, Ordering::Release);
            match self.cancellation.load(Ordering::Acquire) {
                2 => signal_process_group(process_id as i32, libc::SIGKILL),
                1 => signal_process_group(process_id as i32, libc::SIGTERM),
                _ => {}
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = process_id;
    }

    fn request_cancel(&self) {
        let _ = self
            .cancellation
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
        #[cfg(target_os = "linux")]
        {
            let process_group = self.process_group.load(Ordering::Acquire);
            if process_group > 0 {
                let signal = if self.cancellation.load(Ordering::Acquire) == 2 {
                    libc::SIGKILL
                } else {
                    libc::SIGTERM
                };
                signal_process_group(process_group, signal);
            }
        }
    }

    fn force_cancel(&self) {
        self.cancellation.store(2, Ordering::Release);
        #[cfg(target_os = "linux")]
        {
            let process_group = self.process_group.load(Ordering::Acquire);
            if process_group > 0 {
                signal_process_group(process_group, libc::SIGKILL);
            }
        }
    }

    fn clear_process(&self) {
        #[cfg(target_os = "linux")]
        self.process_group.store(0, Ordering::Release);
    }
}

impl TransferManager {
    fn begin(&self, id: &str) -> Result<Arc<TransferControl>> {
        let mut active = self.active.lock().expect("transfer lock poisoned");
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(LauncherError::InvalidArguments(
                "O Orbit está sendo encerrado e não pode iniciar novos downloads".into(),
            ));
        }
        if active.contains_key(id) {
            return Err(LauncherError::InvalidArguments(
                "Esta operação já está em execução".into(),
            ));
        }
        let cancellation = Arc::new(TransferControl::new());
        active.insert(id.to_string(), cancellation.clone());
        Ok(cancellation)
    }

    fn cancel(&self, id: &str) -> bool {
        let cancellation = self
            .active
            .lock()
            .expect("transfer lock poisoned")
            .get(id)
            .cloned();
        cancellation.is_some_and(|cancellation| {
            cancellation.request_cancel();
            true
        })
    }

    pub(crate) fn cancel_all(&self) {
        let active = {
            let active = self.active.lock().expect("transfer lock poisoned");
            // A flag é alterada enquanto o mesmo lock de `begin` está retido:
            // toda operação fica no snapshot ou é rejeitada a partir daqui.
            self.shutting_down.store(true, Ordering::Release);
            active.values().cloned().collect::<Vec<_>>()
        };
        for transfer in active {
            transfer.force_cancel();
        }
    }

    fn finish(&self, id: &str) {
        self.active
            .lock()
            .expect("transfer lock poisoned")
            .remove(id);
    }
}

#[tauri::command]
pub async fn get_library(window: Window, state: State<'_, AppState>) -> Result<Vec<LibraryItem>> {
    let data_dir = state.data_dir.clone();
    let database_path = data_dir.join("orbit.db");
    let items = tauri::async_runtime::spawn_blocking(move || {
        load_library_with_icons(&database_path, &data_dir)
    })
    .await
    .map_err(|error| LauncherError::LaunchFailed(error.to_string()))??;

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

fn load_library_with_icons(database_path: &Path, data_dir: &Path) -> Result<Vec<LibraryItem>> {
    let database = Database::open(database_path)?;
    let mut items = database.list()?;
    for item in &mut items {
        if item.provider == ProviderKind::Custom
            && item
                .icon
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            let Some(executable) = item.executable.as_deref().map(Path::new) else {
                continue;
            };
            let Some(path) = icon::cached_executable_icon(executable, data_dir) else {
                continue;
            };
            if database.set_custom_icon_if_missing(&item.id, &path)? {
                item.icon = Some(path.to_string_lossy().into_owned());
            }
            continue;
        }

        if matches!(item.provider, ProviderKind::Desktop | ProviderKind::Flatpak) {
            let Some(unresolved) = item.icon.as_deref().map(str::trim) else {
                continue;
            };
            if unresolved.is_empty() || Path::new(unresolved).is_absolute() {
                continue;
            }
            let Some(resolved) = providers::desktop::resolve_icon(unresolved) else {
                continue;
            };
            let resolved = Path::new(&resolved);
            if database.set_scanned_icon_if_matches(&item.id, unresolved, resolved)? {
                item.icon = Some(resolved.to_string_lossy().into_owned());
            }
        }
    }
    normalize_library_asset_paths(&mut items);
    Ok(items)
}

/// O protocolo de assets do Tauri 2.11 não lida corretamente com links
/// simbólicos relativos (comuns em temas de ícones KDE). A URL ainda contém o
/// link, enquanto a validação do escopo pode acabar comparando somente o alvo
/// relativo. Entregar o caminho canônico evita a divergência sem liberar uma
/// árvore inteira do diretório pessoal.
fn normalize_library_asset_paths(items: &mut [LibraryItem]) {
    for item in items {
        for asset in [&mut item.icon, &mut item.cover, &mut item.background] {
            let Some(value) = asset.as_deref() else {
                continue;
            };
            let path = Path::new(value);
            if !path.is_absolute() {
                continue;
            }
            let Ok(canonical) = path.canonicalize() else {
                continue;
            };
            if canonical.is_file() && canonical != path {
                *asset = Some(canonical.to_string_lossy().into_owned());
            }
        }
    }
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
    let mut spec = launch::resolve(&item)?;
    let notes = compatibility::apply(&mut spec, &item.compatibility, &state.data_dir, &id)?;
    launch::apply_terminal(&mut spec, settings.preferred_terminal.as_deref())?;
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
pub async fn uninstall_item(id: String, app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    if state.process_manager.running().contains_key(&id) {
        return Err(LauncherError::InvalidArguments(
            "Feche o jogo ou aplicativo antes de desinstalá-lo".into(),
        ));
    }
    let item = state
        .database
        .lock()
        .expect("database lock poisoned")
        .get(&id)?
        .ok_or_else(|| LauncherError::NotFound(id.clone()))?;
    if !item.installed {
        return Err(LauncherError::InvalidArguments(
            "Este item não está instalado".into(),
        ));
    }
    if state
        .database
        .lock()
        .expect("database lock poisoned")
        .operations()?
        .iter()
        .any(|operation| {
            format!("{}:{}", operation.provider, operation.item_id) == id
                && matches!(
                    operation.state.as_str(),
                    "queued" | "running" | "cancelling" | "paused"
                )
        })
    {
        return Err(LauncherError::InvalidArguments(
            "Aguarde ou cancele a operação atual antes de desinstalar".into(),
        ));
    }

    let data_dir = state.data_dir.clone();
    let steam_user = if item.provider == ProviderKind::Steam {
        Some(
            platform::connected_provider_user(
                &data_dir,
                &state.database.lock().expect("database lock poisoned"),
                "steam",
            )
            .ok_or_else(|| {
                LauncherError::ProviderUnavailable(
                    "Conecte sua conta Steam antes de desinstalar este jogo".into(),
                )
            })?,
        )
    } else {
        None
    };
    let item_id = item
        .id
        .split_once(':')
        .map(|(_, value)| value.to_string())
        .ok_or_else(|| LauncherError::InvalidArguments("ID de item inválido".into()))?;
    let provider = item.provider.clone();
    let executable = item.executable.clone();
    let database_path = data_dir.join("orbit.db");
    let id_for_worker = id.clone();

    let completed = tauri::async_runtime::spawn_blocking(move || {
        let completed = uninstall_provider_item(
            &provider,
            &item_id,
            executable.as_deref(),
            steam_user.as_deref(),
            &data_dir,
        )?;
        if !completed {
            return Ok(false);
        }
        let database = Database::open(&database_path)?;
        if !database.set_uninstalled(&id_for_worker)? {
            return Err(LauncherError::InvalidArguments(
                "O estado do item mudou durante a desinstalação".into(),
            ));
        }
        Ok(true)
    })
    .await
    .map_err(|error| LauncherError::LaunchFailed(error.to_string()))??;

    if completed {
        let _ = app.emit("library-changed", id);
    }
    Ok(())
}

fn uninstall_provider_item(
    provider: &ProviderKind,
    item_id: &str,
    executable: Option<&str>,
    steam_user: Option<&str>,
    data_dir: &Path,
) -> Result<bool> {
    let manager = platform::DependencyManager::new(data_dir.to_path_buf());
    let mut command = match provider {
        ProviderKind::Epic => {
            let executable = manager
                .executable("legendary")
                .ok_or_else(|| LauncherError::ExecutableNotFound("legendary".into()))?;
            let mut command = std::process::Command::new(executable);
            command.args(["-y", "uninstall", item_id]);
            command
        }
        ProviderKind::Steam => {
            let user = steam_user.ok_or_else(|| {
                LauncherError::ProviderUnavailable("Conta Steam não conectada".into())
            })?;
            let executable = manager
                .executable("steamcmd")
                .ok_or_else(|| LauncherError::ExecutableNotFound("steamcmd".into()))?;
            let mut command = std::process::Command::new(executable);
            command.args(steam_uninstall_arguments(user, item_id));
            command
        }
        ProviderKind::Flatpak => {
            let mut command = std::process::Command::new("flatpak");
            command.args(["uninstall", "--noninteractive", item_id]);
            command
        }
        ProviderKind::Appimage => {
            let path = executable
                .map(Path::new)
                .ok_or_else(|| LauncherError::ExecutableNotFound(item_id.into()))?;
            if !path.is_file() {
                return Err(LauncherError::ExecutableNotFound(
                    path.to_string_lossy().into_owned(),
                ));
            }
            if std::env::current_exe()
                .ok()
                .and_then(|current| current.canonicalize().ok())
                .zip(path.canonicalize().ok())
                .is_some_and(|(current, target)| current == target)
            {
                return Err(LauncherError::InvalidArguments(
                    "O Orbit não pode mover a própria execução para a lixeira".into(),
                ));
            }
            let mut command = std::process::Command::new("gio");
            command.arg("trash").arg(path);
            command
        }
        _ => {
            return Err(LauncherError::ProviderUnavailable(
                "A origem deste item não oferece uma desinstalação segura pelo Orbit".into(),
            ));
        }
    };
    clean_appimage_environment(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    run_managed_command(command, Duration::from_secs(10 * 60))?;

    if *provider == ProviderKind::Epic
        && confirm_legendary_installation(data_dir, item_id)?.is_some()
    {
        return Err(LauncherError::ProviderUnavailable(
            "O Legendary terminou, mas o jogo continua registrado como instalado".into(),
        ));
    }
    if *provider == ProviderKind::Steam
        && providers::steam::SteamProvider
            .scan()?
            .iter()
            .any(|item| item.id == format!("steam:{item_id}"))
    {
        return Err(LauncherError::ProviderUnavailable(
            "O SteamCMD terminou, mas o jogo continua registrado como instalado".into(),
        ));
    }
    if *provider == ProviderKind::Flatpak
        && std::process::Command::new("flatpak")
            .args(["info", item_id])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    {
        return Err(LauncherError::ProviderUnavailable(
            "O Flatpak terminou, mas o aplicativo continua instalado".into(),
        ));
    }
    if *provider == ProviderKind::Appimage
        && executable.is_some_and(|path| Path::new(path).exists())
    {
        return Err(LauncherError::ProviderUnavailable(
            "O AppImage não foi movido para a lixeira".into(),
        ));
    }
    Ok(true)
}

fn run_managed_command(mut command: std::process::Command, timeout: Duration) -> Result<()> {
    #[cfg(target_os = "linux")]
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|error| LauncherError::LaunchFailed(error.to_string()))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                return Err(LauncherError::LaunchFailed(format!(
                    "O desinstalador terminou com {status}"
                )))
            }
            None if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            None => {
                terminate_store_process(&mut child);
                let _ = child.wait();
                return Err(LauncherError::LaunchFailed(
                    "A desinstalação excedeu 10 minutos e foi encerrada".into(),
                ));
            }
        }
    }
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
pub async fn list_themes() -> Result<Vec<ThemeSummary>> {
    tauri::async_runtime::spawn_blocking(ThemeManager::list)
        .await
        .map_err(|error| LauncherError::InvalidTheme(error.to_string()))?
}

#[tauri::command]
pub async fn get_theme(id: String) -> Result<ThemeDetails> {
    tauri::async_runtime::spawn_blocking(move || ThemeManager::get(&id))
        .await
        .map_err(|error| LauncherError::InvalidTheme(error.to_string()))?
}

#[tauri::command]
pub fn get_active_theme(state: State<AppState>) -> Result<ThemeDetails> {
    let id = state
        .database
        .lock()
        .expect("database lock poisoned")
        .settings()?
        .active_theme_id;
    ThemeManager::get(&id).or_else(|_| ThemeManager::get("orbit-dark"))
}

#[tauri::command]
pub fn set_active_theme(id: String, state: State<AppState>) -> Result<ThemeDetails> {
    let theme = ThemeManager::get(&id)?;
    let mut settings = state
        .database
        .lock()
        .expect("database lock poisoned")
        .settings()?;
    settings.active_theme_id = theme.summary.id.clone();
    // Mantém consumidores antigos da preferência `theme` funcionais durante a migração.
    settings.theme = match theme.summary.theme_type {
        crate::themes::manifest::ThemeType::Light => "system",
        crate::themes::manifest::ThemeType::Dark => "dark",
    }
    .into();
    state
        .database
        .lock()
        .expect("database lock poisoned")
        .save_settings(&settings)?;
    Ok(theme)
}

#[tauri::command]
pub async fn validate_theme(path: String) -> Result<ThemeSummary> {
    let path = PathBuf::from(path);
    tauri::async_runtime::spawn_blocking(move || ThemeManager::validate_archive(&path))
        .await
        .map_err(|error| LauncherError::InvalidTheme(error.to_string()))?
}

#[tauri::command]
pub async fn import_theme(path: String) -> Result<ThemeSummary> {
    let path = PathBuf::from(path);
    tauri::async_runtime::spawn_blocking(move || ThemeManager::import(&path))
        .await
        .map_err(|error| LauncherError::InvalidTheme(error.to_string()))?
}

#[tauri::command]
pub async fn remove_theme(id: String, state: State<'_, AppState>) -> Result<()> {
    let id_for_remove = id.clone();
    tauri::async_runtime::spawn_blocking(move || ThemeManager::remove(&id_for_remove))
        .await
        .map_err(|error| LauncherError::InvalidTheme(error.to_string()))??;
    let mut settings = state
        .database
        .lock()
        .expect("database lock poisoned")
        .settings()?;
    if settings.active_theme_id == id {
        settings.active_theme_id = "orbit-dark".into();
        settings.theme = "dark".into();
        state
            .database
            .lock()
            .expect("database lock poisoned")
            .save_settings(&settings)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn export_theme(id: String, path: String) -> Result<()> {
    let destination = PathBuf::from(path);
    tauri::async_runtime::spawn_blocking(move || ThemeManager::export(&id, &destination))
        .await
        .map_err(|error| LauncherError::InvalidTheme(error.to_string()))?
}

#[tauri::command]
pub async fn detect_color_scheme_provider() -> Result<crate::themes::automatic::ProviderStatus> {
    tauri::async_runtime::spawn_blocking(crate::themes::automatic::detect_provider)
        .await
        .map_err(|error| LauncherError::InvalidTheme(error.to_string()))
}

#[tauri::command]
pub async fn get_pywal_status() -> Result<crate::themes::automatic::ProviderStatus> {
    detect_color_scheme_provider().await
}

#[tauri::command]
pub async fn get_current_wallpaper() -> Result<String> {
    tauri::async_runtime::spawn_blocking(crate::themes::automatic::current_wallpaper)
        .await
        .map_err(|error| LauncherError::InvalidTheme(error.to_string()))?
}

#[tauri::command]
pub async fn generate_automatic_palette(
    state: State<'_, AppState>,
) -> Result<crate::themes::automatic::AutomaticTheme> {
    let settings = state
        .database
        .lock()
        .expect("database lock poisoned")
        .settings()?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::themes::automatic::generate(
            settings.wallpaper_influence,
            &settings.automatic_color_mode,
            &settings.palette_source,
            settings.manual_wallpaper_path,
        )
    })
    .await
    .map_err(|error| LauncherError::InvalidTheme(error.to_string()))?
}

#[tauri::command]
pub async fn get_automatic_theme(
    state: State<'_, AppState>,
) -> Result<crate::themes::automatic::AutomaticTheme> {
    generate_automatic_palette(state).await
}

#[tauri::command]
pub async fn refresh_automatic_theme(
    state: State<'_, AppState>,
) -> Result<crate::themes::automatic::AutomaticTheme> {
    let automatic = generate_automatic_palette(state).await?;
    Ok(automatic)
}

#[tauri::command]
pub fn list_argument_presets(
    state: State<AppState>,
) -> Result<Vec<crate::core::model::ArgumentPreset>> {
    state
        .database
        .lock()
        .expect("database lock poisoned")
        .list_argument_presets()
}

#[tauri::command]
pub fn save_argument_preset(
    preset: crate::core::model::ArgumentPreset,
    state: State<AppState>,
) -> Result<()> {
    state
        .database
        .lock()
        .expect("database lock poisoned")
        .save_argument_preset(&preset)
}

#[tauri::command]
pub fn delete_argument_preset(id: String, state: State<AppState>) -> Result<()> {
    state
        .database
        .lock()
        .expect("database lock poisoned")
        .delete_argument_preset(&id)
}

#[tauri::command]
pub fn get_argument_preset(
    id: String,
    state: State<AppState>,
) -> Result<Option<crate::core::model::ArgumentPreset>> {
    state
        .database
        .lock()
        .expect("database lock poisoned")
        .get_argument_preset(&id)
}

#[tauri::command]
pub fn update_item(item: ItemInput, state: State<AppState>) -> Result<LibraryItem> {
    let extracted_icon = if item.provider == ProviderKind::Custom && item.icon.is_none() {
        item.executable
            .as_deref()
            .and_then(|path| icon::cached_executable_icon(Path::new(path), &state.data_dir))
            .map(|path| path.to_string_lossy().into_owned())
    } else {
        None
    };
    let id = item
        .id
        .clone()
        .unwrap_or_else(|| format!("custom:{}", uuid::Uuid::new_v4()));
    // Provider entries carry catalog-only state (cover, ownership and physical
    // installation). Editing launch options must not turn an uninstalled Epic
    // entitlement into an installed game or discard its synchronized cover.
    let mut saved = state
        .database
        .lock()
        .expect("database lock poisoned")
        .get(&id)?
        .unwrap_or_else(|| {
            LibraryItem::new(
                id.clone(),
                item.name.clone(),
                item.kind.clone(),
                item.provider.clone(),
            )
        });
    saved.name = item.name;
    saved.kind = item.kind;
    saved.provider = item.provider;
    saved.executable = item.executable;
    saved.arguments = item.arguments;
    saved.working_directory = item.working_directory;
    saved.environment = item.environment;
    saved.icon = item.icon.or(extracted_icon).or(saved.icon);
    saved.category = item.category;
    saved.terminal = item.terminal;
    saved.compatibility = item.compatibility;
    saved.updated_at = chrono::Utc::now().to_rfc3339();
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
pub async fn connect_provider(
    provider: String,
    user: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let data_dir = state.data_dir.clone();
    let manager = platform::ProviderManager::new(data_dir.clone());
    let (command, args) = manager.authenticate_command(&provider, user.as_deref())?;
    let steam_user = (provider == "steam")
        .then(|| args.get(1).cloned())
        .flatten();
    let steam_log_offset = steam_user.as_ref().map(|_| manager.steam_log_len());
    let settings = state
        .database
        .lock()
        .expect("database lock poisoned")
        .settings()?;
    let terminal = settings
        .preferred_terminal
        .unwrap_or_else(|| "konsole".into());

    let status = tauri::async_runtime::spawn_blocking(move || {
        let mut process = std::process::Command::new(&terminal);
        if Path::new(&terminal)
            .file_name()
            .is_some_and(|name| name == "konsole")
        {
            // Without --nofork, Konsole may hand the tab to an existing
            // process and exit immediately, making Orbit refresh too early.
            process.arg("--nofork");
        }
        process.arg("-e").arg(command).args(args);
        clean_appimage_environment(&mut process);
        process.status()
    })
    .await
    .map_err(|error| LauncherError::LaunchFailed(error.to_string()))?
    .map_err(|error| LauncherError::LaunchFailed(error.to_string()))?;
    if !status.success() {
        return Err(LauncherError::ProviderUnavailable(
            "A autenticação foi cancelada ou o terminal terminou com erro".into(),
        ));
    }

    if let (Some(expected_user), Some(offset)) = (steam_user, steam_log_offset) {
        let verified_user = platform::ProviderManager::new(data_dir)
            .steam_account_from_log_since(offset)
            .filter(|account| account.eq_ignore_ascii_case(&expected_user))
            .ok_or_else(|| {
                LauncherError::ProviderUnavailable(
                    "O SteamCMD terminou sem confirmar uma sessão conectada. Tente novamente e conclua o Steam Guard no terminal".into(),
                )
            })?;
        state
            .database
            .lock()
            .expect("database lock poisoned")
            .upsert_provider_account("steam", "connected", Some(&verified_user))?;
    }
    let _ = app.emit("provider-state-changed", &provider);
    Ok(())
}

fn clean_appimage_environment(command: &mut std::process::Command) {
    for variable in [
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "PYTHONHOME",
        "PYTHONPATH",
        "GI_TYPELIB_PATH",
        "GDK_PIXBUF_MODULE_FILE",
        "GTK_PATH",
    ] {
        command.env_remove(variable);
    }
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
    if provider == "steam"
        && platform::connected_provider_user(
            &state.data_dir,
            &state.database.lock().expect("database lock poisoned"),
            "steam",
        )
        .is_none()
    {
        return Err(LauncherError::ProviderUnavailable(
            "Conecte sua conta Steam antes de instalar jogos da biblioteca".into(),
        ));
    }
    if provider == "epic" {
        let item = state
            .database
            .lock()
            .expect("database lock poisoned")
            .get(&format!("epic:{item_id}"))?
            .ok_or_else(|| LauncherError::NotFound(format!("epic:{item_id}")))?;
        if !item.owned {
            return Err(LauncherError::ProviderUnavailable(
                "Este jogo não pertence à conta Epic conectada".into(),
            ));
        }
    }
    let operation = platform::ProviderManager::operation(&provider, &item_id, &action);
    let id = operation.id.clone();
    let db_path = state.data_dir.join("orbit.db");
    let data = state.data_dir.clone();
    let transfer_manager = state.transfer_manager.clone();
    let cancellation = transfer_manager.begin(&id)?;
    if let Err(error) = state
        .database
        .lock()
        .expect("database lock poisoned")
        .queue_operation(&operation)
    {
        transfer_manager.finish(&id);
        return Err(error);
    }
    std::thread::spawn(move || {
        run_store_operation(
            app,
            db_path,
            data,
            operation,
            transfer_manager,
            cancellation,
        )
    });
    Ok(id)
}

fn run_store_operation(
    app: AppHandle,
    db_path: PathBuf,
    data: PathBuf,
    operation: platform::Operation,
    transfer_manager: TransferManager,
    cancellation: Arc<TransferControl>,
) {
    let progress_database = match Database::open(&db_path) {
        Ok(database) => database,
        Err(error) => {
            tracing::error!(%error, operation=%operation.id, "não foi possível abrir a fila");
            transfer_manager.finish(&operation.id);
            return;
        }
    };
    let mut current_operation = match progress_database.start_operation(&operation.id) {
        Ok(Some(operation)) => operation,
        Ok(None) => {
            // O cancelamento pode vencer a pequena janela entre enfileirar e o
            // worker adquirir a operação. Conclua o estado em vez de deixá-lo
            // preso indefinidamente em `cancelling`.
            if let Ok(Some(operation)) = progress_database.operation(&operation.id) {
                if operation.state == "cancelling" {
                    if let Ok(Some(cancelled)) = progress_database.finish_operation(
                        &operation.id,
                        "failed",
                        operation.downloaded_bytes,
                        operation.total_bytes,
                        Some("Download cancelado pelo usuário"),
                    ) {
                        let _ = app.emit("transfer-progress", cancelled);
                    }
                }
            }
            transfer_manager.finish(&operation.id);
            return;
        }
        Err(error) => {
            tracing::error!(%error, operation=%operation.id, "não foi possível adquirir a operação");
            transfer_manager.finish(&operation.id);
            return;
        }
    };
    let _ = app.emit("transfer-progress", current_operation.clone());
    let manager = platform::DependencyManager::new(data.clone());
    let steam_user = if operation.provider == "steam" {
        Database::open(&db_path)
            .ok()
            .and_then(|db| platform::connected_provider_user(&data, &db, "steam"))
    } else {
        None
    };
    let command = match operation.provider.as_str() {
        "epic" => manager.executable("legendary").map(|exe| {
            (
                exe,
                epic_store_arguments(&operation.item_id, &operation.action, &data),
            )
        }),
        "steam" => manager.executable("steamcmd").and_then(|exe| {
            steam_user.as_ref().map(|user| {
                (
                    exe,
                    steam_store_arguments(user, &operation.item_id, &operation.action),
                )
            })
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
    let missing_command = if operation.provider == "steam" && steam_user.is_none() {
        "Conta Steam não conectada; conecte novamente antes de instalar".to_string()
    } else {
        "Componente do provider não instalado".to_string()
    };
    let mut downloaded_bytes = current_operation.downloaded_bytes;
    let mut total_bytes = current_operation.total_bytes;
    let mut bytes_per_second = 0;
    let mut last_sample: Option<(Instant, u64)> = None;
    let mut provider_error = None;
    let result = command
        .ok_or(missing_command)
        .and_then(|(exe, args)| {
            let mut process = std::process::Command::new(exe);
            process
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            clean_appimage_environment(&mut process);
            if operation.provider == "battlenet" {
                let prefix = data.join("prefixes/battlenet");
                let _ = std::fs::create_dir_all(&prefix);
                process.env("WINEPREFIX", prefix);
            }
            #[cfg(target_os = "linux")]
            process.process_group(0);
            if cancellation.is_cancelled() {
                return Err("cancelled".into());
            }
            let mut child = process.spawn().map_err(|error| error.to_string())?;
            cancellation.register_process(child.id());
            let (sender, receiver) = mpsc::channel();
            if let Some(stdout) = child.stdout.take() {
                stream_process_chunks(stdout, sender.clone());
            }
            if let Some(stderr) = child.stderr.take() {
                stream_process_chunks(stderr, sender.clone());
            }
            drop(sender);

            loop {
                if cancellation.is_cancelled() {
                    terminate_store_process(&mut child);
                    break;
                }
                let line = match receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(line) => line,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                if let Some(error) = provider_operation_error(&operation.provider, &line) {
                    provider_error = Some(error);
                }
                let Some(progress) = platform::parse_transfer_progress(&operation.provider, &line)
                else {
                    continue;
                };
                let now = Instant::now();
                if let Some((sampled_at, sampled_bytes)) = last_sample {
                    let elapsed = now.duration_since(sampled_at).as_secs_f64();
                    if elapsed > 0.0 && progress.downloaded_bytes >= sampled_bytes {
                        bytes_per_second =
                            ((progress.downloaded_bytes - sampled_bytes) as f64 / elapsed) as u64;
                    }
                }
                last_sample = Some((now, progress.downloaded_bytes));
                downloaded_bytes = progress.downloaded_bytes;
                total_bytes = progress.total_bytes;
                match progress_database.update_running_progress(
                    &operation.id,
                    downloaded_bytes,
                    total_bytes,
                    bytes_per_second,
                ) {
                    Ok(true) => {
                        current_operation.downloaded_bytes = downloaded_bytes;
                        current_operation.total_bytes = total_bytes;
                        current_operation.bytes_per_second = bytes_per_second;
                        current_operation.error = None;
                        current_operation.updated_at = chrono::Utc::now().to_rfc3339();
                        let _ = app.emit("transfer-progress", current_operation.clone());
                    }
                    Ok(false) => {}
                    Err(error) => {
                        tracing::warn!(%error, operation=%operation.id, "não foi possível persistir o progresso");
                    }
                }
            }
            let status = child.wait().map_err(|error| error.to_string());
            cancellation.clear_process();
            status
        })
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(provider_error.unwrap_or_else(|| format!("Processo terminou com {status}")))
            }
        });
    let mut epic_installation = None;
    let result = result.and_then(|()| {
        if operation.provider != "epic" || operation.action == "verify" {
            return Ok(());
        }
        match confirm_legendary_installation(&data, &operation.item_id) {
            Ok(Some(path)) => {
                epic_installation = Some(path);
                Ok(())
            }
            Ok(None) => {
                Err("O Legendary terminou sem confirmar a instalação local do jogo".to_string())
            }
            Err(error) => Err(error.to_string()),
        }
    });
    let (outcome, error) = match result {
        Ok(()) if !cancellation.is_cancelled() => {
            if total_bytes > 0 {
                downloaded_bytes = total_bytes;
            }
            ("completed", None)
        }
        Ok(()) => (
            "failed",
            Some("Download cancelado pelo usuário".to_string()),
        ),
        Err(error) => ("failed", Some(error)),
    };
    let finished = match progress_database.finish_operation(
        &operation.id,
        outcome,
        downloaded_bytes,
        total_bytes,
        error.as_deref(),
    ) {
        Ok(finished) => finished,
        Err(database_error) => {
            tracing::error!(%database_error, operation=%operation.id, "não foi possível finalizar a operação");
            None
        }
    };
    transfer_manager.finish(&operation.id);
    if let Some(finished) = finished {
        if finished.state == "completed" && operation.provider == "epic" {
            let item_id = format!("epic:{}", operation.item_id);
            match epic_installation.as_deref().map_or(Ok(false), |path| {
                progress_database.set_installation(&item_id, path)
            }) {
                Ok(true) => {
                    let _ = app.emit("library-changed", item_id);
                }
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(%error, item=%item_id, "não foi possível atualizar a instalação Epic");
                }
            }
        }
        let _ = app.emit("transfer-progress", finished);
    }
}

fn terminate_store_process(child: &mut std::process::Child) {
    #[cfg(target_os = "linux")]
    {
        let process_group = child.id() as i32;
        // O processo foi criado como líder de um grupo próprio. Encerrar o
        // grupo impede que wrappers do SteamCMD/Legendary continuem baixando
        // depois de o item aparecer como cancelado no Orbit.
        signal_process_group(process_group, libc::SIGTERM);
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        // Mesmo se o processo líder já saiu, algum descendente pode ter
        // ignorado SIGTERM. O segundo sinal garante que o grupo inteiro pare.
        signal_process_group(process_group, libc::SIGKILL);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = child.kill();
    }
}

#[cfg(target_os = "linux")]
fn signal_process_group(process_group: i32, signal: i32) {
    if process_group <= 0 {
        return;
    }
    // SAFETY: `process_group` comes from the positive PID of a child created
    // with `process_group(0)`. Negating it targets only that child's group.
    unsafe {
        libc::kill(-process_group, signal);
    }
}

fn stream_process_chunks<R>(reader: R, sender: mpsc::Sender<String>)
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buffer = Vec::with_capacity(512);
        loop {
            buffer.clear();
            let read = reader.read_until(b'\n', &mut buffer).unwrap_or_default();
            if read == 0 {
                break;
            }
            for chunk in buffer.split(|byte| *byte == b'\r') {
                if chunk.is_empty() {
                    continue;
                }
                let line = String::from_utf8_lossy(chunk)
                    .trim_end_matches('\n')
                    .to_string();
                if !line.is_empty() && sender.send(line).is_err() {
                    return;
                }
            }
        }
    });
}

#[cfg(test)]
fn collect_process_chunks(input: &[u8]) -> Vec<String> {
    let (sender, receiver) = mpsc::channel();
    stream_process_chunks(std::io::Cursor::new(input.to_vec()), sender);
    receiver.into_iter().collect()
}

fn steam_store_arguments(user: &str, item_id: &str, action: &str) -> Vec<String> {
    let mut arguments = vec!["+login".into(), user.into()];
    if action == "install" {
        // Jogos Free on Demand não pertencem à conta até que uma licença
        // gratuita seja solicitada. Sem isso, o SteamCMD termina com
        // `No subscription`, mesmo quando a página da loja mostra Gratuito.
        arguments.extend(["+app_license_request".into(), item_id.into()]);
    }
    arguments.extend([
        "+app_update".into(),
        item_id.into(),
        "validate".into(),
        "+quit".into(),
    ]);
    arguments
}

fn steam_uninstall_arguments(user: &str, item_id: &str) -> Vec<String> {
    vec![
        "+login".into(),
        user.into(),
        "+app_uninstall".into(),
        item_id.into(),
        "+quit".into(),
    ]
}

fn epic_store_arguments(item_id: &str, action: &str, data_dir: &Path) -> Vec<String> {
    if action == "verify" {
        return vec!["verify".into(), item_id.into()];
    }

    let mut arguments = vec![
        "-y".into(),
        action.into(),
        item_id.into(),
        "--base-path".into(),
        data_dir.join("games/epic").to_string_lossy().into_owned(),
    ];
    // Make the install fully non-interactive. With global `-y`, Legendary
    // otherwise selects every DLC; SDL can also prompt for optional packs.
    arguments.extend(["--skip-sdl".into(), "--skip-dlcs".into()]);
    arguments
}

fn confirm_legendary_installation(data_dir: &Path, item_id: &str) -> Result<Option<PathBuf>> {
    let executable = platform::DependencyManager::new(data_dir.to_path_buf())
        .executable("legendary")
        .ok_or_else(|| LauncherError::ExecutableNotFound("legendary".into()))?;
    let control = LibrarySyncControl::new();
    let output = run_legendary_json_command(
        &executable,
        &["list-installed", "--json"],
        Duration::from_secs(30),
        &control,
    )?;
    let mut installed = parse_legendary_installed(&output)?;
    Ok(installed
        .remove(item_id)
        .map(|game| game.install_path)
        .filter(|path| path.is_dir()))
}

fn provider_operation_error(provider: &str, output: &str) -> Option<String> {
    if provider == "steam" && output.contains("No subscription") {
        return Some(
            "A Steam não concedeu uma licença para este AppID; ele pode não estar disponível para esta conta ou região".into(),
        );
    }
    if provider == "epic" {
        let normalized = output.to_ascii_lowercase();
        if normalized.contains("has to be installed via a third-party store") {
            return Some(
                "Este título precisa ser instalado pelo cliente indicado pela Epic Games".into(),
            );
        }
        if normalized.contains("installation failed")
            || normalized.contains("exception occurred while waiting for the downloader")
            || normalized.contains("installation cannot proceed")
            || normalized.contains("login failed")
            || normalized.contains("could not find")
        {
            return Some("O Legendary não conseguiu concluir a instalação do jogo".into());
        }
    }
    None
}

#[tauri::command]
pub fn retry_operation(id: String, app: AppHandle, state: State<AppState>) -> Result<()> {
    let db_path = state.data_dir.join("orbit.db");
    let data = state.data_dir.clone();
    let transfer_manager = state.transfer_manager.clone();
    let cancellation = transfer_manager.begin(&id)?;
    let retried_result = {
        let database = state.database.lock().expect("database lock poisoned");
        database.retry_operation(&id)
    };
    let retried = match retried_result {
        Ok(operation) => operation,
        Err(error) => {
            transfer_manager.finish(&id);
            return Err(error);
        }
    };
    let operation = match retried {
        Some(operation) => operation,
        None => {
            transfer_manager.finish(&id);
            let existing = state
                .database
                .lock()
                .expect("database lock poisoned")
                .operation(&id)?;
            return match existing {
                None => Err(LauncherError::NotFound(id)),
                Some(_) => Err(LauncherError::InvalidArguments(
                    "Somente operações com falha ou canceladas podem ser repetidas".into(),
                )),
            };
        }
    };
    std::thread::spawn(move || {
        run_store_operation(
            app,
            db_path,
            data,
            operation,
            transfer_manager,
            cancellation,
        )
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_store_operation(id: String, app: AppHandle, state: State<AppState>) -> Result<()> {
    let operation = state
        .database
        .lock()
        .expect("database lock poisoned")
        .request_cancel(&id)?
        .ok_or_else(|| LauncherError::NotFound(id.clone()))?;
    if operation.state == "cancelled" {
        let _ = app.emit("transfer-progress", operation);
        return Ok(());
    }
    if operation.state != "cancelling" {
        return Err(LauncherError::InvalidArguments(
            "Somente um download em execução pode ser cancelado".into(),
        ));
    }
    if !state.transfer_manager.cancel(&id) {
        // O processo pode ter terminado exatamente entre a transição no banco
        // e a sinalização. Finalizar via CAS evita deixar `cancelling` preso.
        let finished = state
            .database
            .lock()
            .expect("database lock poisoned")
            .finish_operation(
                &id,
                "failed",
                operation.downloaded_bytes,
                operation.total_bytes,
                Some("Download cancelado pelo usuário"),
            )?;
        if let Some(finished) = finished {
            let _ = app.emit("transfer-progress", finished);
            return Ok(());
        }
        let current = state
            .database
            .lock()
            .expect("database lock poisoned")
            .operation(&id)?;
        if let Some(current) = current {
            if current.state == "cancelled" {
                let _ = app.emit("transfer-progress", current);
                return Ok(());
            }
        }
        return Err(LauncherError::InvalidArguments(
            "O processo deste download não está mais ativo".into(),
        ));
    }
    let _ = app.emit("transfer-progress", operation);
    Ok(())
}

#[tauri::command]
pub fn remove_store_operation(id: String, app: AppHandle, state: State<AppState>) -> Result<()> {
    let database = state.database.lock().expect("database lock poisoned");
    let operation = database
        .operations()?
        .into_iter()
        .find(|operation| operation.id == id)
        .ok_or_else(|| LauncherError::NotFound(id.clone()))?;
    if operation.state == "running" {
        return Err(LauncherError::InvalidArguments(
            "Um download em execução precisa terminar antes de ser removido".into(),
        ));
    }
    if !database.remove_operation(&id)? {
        return Err(LauncherError::InvalidArguments(
            "Esta operação não pode ser removida no estado atual".into(),
        ));
    }
    let _ = app.emit("transfer-operation-removed", &id);
    Ok(())
}

const STORE_LIBRARY_SYNC_TIMEOUT: Duration = Duration::from_secs(120);
const STORE_LIBRARY_OUTPUT_LIMIT: usize = 64 * 1024 * 1024;
const STORE_LIBRARY_ERROR_LIMIT: usize = 256 * 1024;

#[derive(Debug, serde::Deserialize)]
struct LegendaryLibraryEntry {
    #[serde(alias = "appName")]
    app_name: String,
    #[serde(default, alias = "appTitle", alias = "title")]
    app_title: Option<String>,
    #[serde(default)]
    metadata: Option<LegendaryMetadata>,
}

#[derive(Debug, serde::Deserialize)]
struct LegendaryInstalledEntry {
    #[serde(alias = "appName")]
    app_name: String,
    #[serde(default, alias = "installPath")]
    install_path: Option<String>,
}

#[derive(Debug)]
struct LegendaryInstalledGame {
    install_path: PathBuf,
}

#[derive(Debug, serde::Deserialize)]
struct LegendaryMetadata {
    #[serde(default, rename = "keyImages", alias = "key_images")]
    key_images: Vec<LegendaryKeyImage>,
}

#[derive(Debug, serde::Deserialize)]
struct LegendaryKeyImage {
    #[serde(default, rename = "type")]
    image_type: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    width: u64,
    #[serde(default)]
    height: u64,
}

#[tauri::command]
pub async fn sync_store_library(provider: String, state: State<'_, AppState>) -> Result<usize> {
    if provider != "epic" {
        return Err(LauncherError::ProviderUnavailable(format!(
            "Sincronização do catálogo {provider} ainda não está disponível"
        )));
    }
    let sync_guard = state.library_sync_manager.begin(&provider)?;
    let data_dir = state.data_dir.clone();
    let executable = platform::DependencyManager::new(data_dir.clone())
        .executable("legendary")
        .ok_or_else(|| LauncherError::ExecutableNotFound("legendary".into()))?;
    let database_path = data_dir.join("orbit.db");

    tauri::async_runtime::spawn_blocking(move || {
        // Keep the single-flight guard inside the blocking job. It is released
        // on every success/error path and prevents two Legendary instances
        // from contending for the same metadata cache.
        let sync_control = sync_guard.control.clone();
        let _sync_guard = sync_guard;
        let catalog_output = run_legendary_json_command(
            &executable,
            &["list", "--json"],
            STORE_LIBRARY_SYNC_TIMEOUT,
            &sync_control,
        )?;
        let installed_output = run_legendary_json_command(
            &executable,
            &["list-installed", "--json"],
            STORE_LIBRARY_SYNC_TIMEOUT,
            &sync_control,
        )?;
        let installed = parse_legendary_installed(&installed_output)?;
        let mut items = parse_legendary_library(&catalog_output, &executable)?;
        let count = items.len();
        let mut database = Database::open(&database_path)?;
        if count == 0 && database.provider_item_count("epic")? > 0 {
            return Err(LauncherError::ProviderUnavailable(
                "A Epic retornou uma biblioteca vazia inesperadamente; o catálogo anterior foi preservado"
                    .into(),
            ));
        }
        // A valid `[]` is authoritative and repairs the legacy state where
        // every owned Epic game was incorrectly persisted as installed.
        // Command/JSON errors returned above still abort before this write.
        merge_legendary_installations(&mut items, &installed);
        database.apply_provider_scan("epic", &items)?;
        Ok(count)
    })
    .await
    .map_err(|error| LauncherError::LaunchFailed(error.to_string()))?
}

fn run_legendary_json_command(
    executable: &Path,
    arguments: &[&str],
    timeout: Duration,
    control: &LibrarySyncControl,
) -> Result<Vec<u8>> {
    // Files avoid pipe backpressure when a large account returns many games.
    // They are unlinked automatically and read only after Legendary exits.
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut command = std::process::Command::new(executable);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.try_clone()?))
        .stderr(Stdio::from(stderr.try_clone()?));
    clean_appimage_environment(&mut command);
    #[cfg(target_os = "linux")]
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|error| LauncherError::LaunchFailed(error.to_string()))?;
    control.register_process(child.id());
    let deadline = Instant::now() + timeout;
    let status = loop {
        let exceeds_limit = match temporary_output_exceeds_limit(&stdout, &stderr) {
            Ok(exceeds_limit) => exceeds_limit,
            Err(error) => {
                terminate_store_process(&mut child);
                let _ = child.wait();
                control.clear_process();
                return Err(error);
            }
        };
        if exceeds_limit {
            terminate_store_process(&mut child);
            let _ = child.wait();
            control.clear_process();
            return Err(LauncherError::ProviderUnavailable(
                "A saída do Legendary excedeu o limite de segurança".into(),
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if temporary_output_exceeds_limit(&stdout, &stderr)? {
                    control.clear_process();
                    return Err(LauncherError::ProviderUnavailable(
                        "A saída do Legendary excedeu o limite de segurança".into(),
                    ));
                }
                break status;
            }
            Ok(None) => {}
            Err(error) => {
                terminate_store_process(&mut child);
                let _ = child.wait();
                control.clear_process();
                return Err(error.into());
            }
        }
        if control.is_cancelled() {
            terminate_store_process(&mut child);
            let _ = child.wait();
            control.clear_process();
            return Err(LauncherError::ProviderUnavailable(
                "A sincronização da Epic foi encerrada".into(),
            ));
        }
        if Instant::now() >= deadline {
            terminate_store_process(&mut child);
            let _ = child.wait();
            control.clear_process();
            return Err(LauncherError::ProviderUnavailable(
                "A sincronização da Epic excedeu 2 minutos e foi encerrada. Verifique sua conexão"
                    .into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    control.clear_process();

    if !status.success() {
        let error =
            read_temporary_output(&mut stderr, STORE_LIBRARY_ERROR_LIMIT, "erro do Legendary")?;
        let message = String::from_utf8_lossy(&error).to_ascii_lowercase();
        let detail = if message.contains("not logged")
            || message.contains("no saved credentials")
            || message.contains("authentication")
        {
            "A sessão Epic expirou. Conecte a conta novamente e repita a sincronização"
        } else {
            "O Legendary não conseguiu consultar a biblioteca. Verifique sua conexão e tente novamente"
        };
        return Err(LauncherError::ProviderUnavailable(detail.into()));
    }

    read_temporary_output(&mut stdout, STORE_LIBRARY_OUTPUT_LIMIT, "biblioteca Epic")
}

fn read_temporary_output(
    file: &mut std::fs::File,
    limit: usize,
    description: &str,
) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(LauncherError::ProviderUnavailable(format!(
            "A saída de {description} excedeu o limite de segurança"
        )));
    }
    Ok(bytes)
}

fn temporary_output_exceeds_limit(stdout: &std::fs::File, stderr: &std::fs::File) -> Result<bool> {
    Ok(stdout.metadata()?.len() > STORE_LIBRARY_OUTPUT_LIMIT as u64
        || stderr.metadata()?.len() > STORE_LIBRARY_ERROR_LIMIT as u64)
}

fn parse_legendary_library(output: &[u8], executable: &Path) -> Result<Vec<LibraryItem>> {
    let entries: Vec<LegendaryLibraryEntry> = serde_json::from_slice(output).map_err(|error| {
        LauncherError::ProviderUnavailable(format!(
            "O Legendary retornou uma biblioteca inválida: {error}"
        ))
    })?;
    let entry_count = entries.len();
    let executable = executable.to_string_lossy().into_owned();
    let mut seen = HashSet::new();
    let items = entries
        .into_iter()
        .map(|entry| {
            let app_name = entry.app_name.trim().to_string();
            if app_name.is_empty()
                || app_name.len() > 512
                || app_name.chars().any(char::is_control)
                || !seen.insert(app_name.clone())
            {
                return Err(LauncherError::ProviderUnavailable(
                    "O Legendary retornou um identificador de jogo inválido ou duplicado".into(),
                ));
            }
            let title = entry
                .app_title
                .as_deref()
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .unwrap_or(&app_name)
                .to_string();
            if title.len() > 4096 || title.contains('\0') {
                return Err(LauncherError::ProviderUnavailable(
                    "O Legendary retornou um título de jogo inválido".into(),
                ));
            }
            let mut item = LibraryItem::new(
                format!("epic:{app_name}"),
                title,
                ItemKind::Game,
                ProviderKind::Epic,
            );
            // `current/bin` is the stable managed location, while a bare
            // `legendary` is normally absent from the user's PATH.
            item.executable = Some(executable.clone());
            item.category = Some("Epic Games".into());
            item.cover = epic_cover(entry.metadata.as_ref());
            item.owned = true;
            item.installed = false;
            Ok(item)
        })
        .collect::<Result<Vec<_>>>()?;
    debug_assert!(entry_count == items.len());
    Ok(items)
}

fn parse_legendary_installed(output: &[u8]) -> Result<HashMap<String, LegendaryInstalledGame>> {
    let entries: Vec<LegendaryInstalledEntry> =
        serde_json::from_slice(output).map_err(|error| {
            LauncherError::ProviderUnavailable(format!(
                "O Legendary retornou uma lista de instalações inválida: {error}"
            ))
        })?;
    let mut installed = HashMap::with_capacity(entries.len());
    for entry in entries {
        let app_name = validate_legendary_app_name(&entry.app_name)?;
        let install_path = entry
            .install_path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| {
                LauncherError::ProviderUnavailable(
                    "O Legendary retornou uma instalação sem diretório".into(),
                )
            })?;
        if !install_path.is_absolute() || install_path.to_string_lossy().contains('\0') {
            return Err(LauncherError::ProviderUnavailable(
                "O Legendary retornou um diretório de instalação inválido".into(),
            ));
        }
        if installed
            .insert(app_name, LegendaryInstalledGame { install_path })
            .is_some()
        {
            return Err(LauncherError::ProviderUnavailable(
                "O Legendary retornou uma instalação duplicada".into(),
            ));
        }
    }
    Ok(installed)
}

fn validate_legendary_app_name(value: &str) -> Result<String> {
    let app_name = value.trim().to_string();
    if app_name.is_empty() || app_name.len() > 512 || app_name.chars().any(char::is_control) {
        return Err(LauncherError::ProviderUnavailable(
            "O Legendary retornou um identificador de jogo inválido".into(),
        ));
    }
    Ok(app_name)
}

fn merge_legendary_installations(
    items: &mut [LibraryItem],
    installed: &HashMap<String, LegendaryInstalledGame>,
) {
    for item in items {
        let app_name = item.id.strip_prefix("epic:").unwrap_or(&item.id);
        let Some(game) = installed.get(app_name) else {
            item.installed = false;
            item.working_directory = None;
            continue;
        };
        // Legendary may retain stale records after a directory is removed.
        // Only an existing directory is physically installed/launchable.
        item.installed = game.install_path.is_dir();
        item.working_directory = item
            .installed
            .then(|| game.install_path.to_string_lossy().into_owned());
    }
}

/// Selects a card image from Legendary's catalog metadata. Only Epic's HTTPS
/// CDN is persisted, keeping the WebView CSP narrow and rejecting catalog
/// values that could escape the frontend's `background-image: url(...)`.
fn epic_cover(metadata: Option<&LegendaryMetadata>) -> Option<String> {
    metadata?
        .key_images
        .iter()
        .filter_map(|image| {
            let url = safe_epic_image_url(&image.url)?;
            let portrait = image.height > image.width && image.width > 0;
            let priority = match image.image_type.as_str() {
                "DieselGameBoxTall" => 0,
                "OfferImageTall" => 1,
                _ if portrait => 2,
                "Thumbnail" => 3,
                "DieselGameBox" => 4,
                "OfferImageWide" => 5,
                _ => 6,
            };
            let area = image.width.saturating_mul(image.height);
            Some((priority, std::cmp::Reverse(area), url))
        })
        .min_by(|left, right| (left.0, left.1, &left.2).cmp(&(right.0, right.1, &right.2)))
        .map(|(_, _, url)| url.to_string())
}

fn safe_epic_image_url(value: &str) -> Option<String> {
    let value = value.trim();
    let path = value.strip_prefix("https://cdn1.epicgames.com/")?;
    if path.is_empty()
        || value.len() > 2_048
        || value.chars().any(|character| {
            character.is_control()
                || (character.is_whitespace() && character != ' ')
                || matches!(character, '(' | ')' | '"' | '\\')
        })
    {
        return None;
    }
    // A few official catalog entries contain literal spaces (for example GTA
    // V). Persist their URL-encoded form so CSS receives a valid, inert URL
    // while keeping the host allow-list above exact.
    Some(value.replace(' ', "%20"))
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

#[cfg(test)]
mod tests {
    use std::{sync::atomic::Ordering, time::Duration};

    use super::{
        collect_process_chunks, epic_store_arguments, load_library_with_icons,
        merge_legendary_installations, parse_legendary_installed, parse_legendary_library,
        provider_operation_error, run_legendary_json_command, steam_store_arguments,
        steam_uninstall_arguments, terminate_store_process, LibrarySyncManager, TransferManager,
    };
    use crate::{
        core::model::{ItemKind, LibraryItem, ProviderKind},
        database::Database,
    };

    #[test]
    fn library_load_backfills_a_custom_icon_cache() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("demo.ico");
        let image = ico::IconImage::from_rgba_data(2, 2, vec![255; 16]);
        let entry = ico::IconDirEntry::encode(&image).unwrap();
        let mut icon = ico::IconDir::new(ico::ResourceType::Icon);
        icon.add_entry(entry);
        icon.write(std::fs::File::create(&executable).unwrap())
            .unwrap();

        let database_path = directory.path().join("orbit.db");
        let mut database = Database::open(&database_path).unwrap();
        let mut item = LibraryItem::new(
            "custom:demo".into(),
            "Demo".into(),
            ItemKind::Application,
            ProviderKind::Custom,
        );
        item.executable = Some(executable.to_string_lossy().into_owned());
        database.save_user_item(&item).unwrap();
        drop(database);

        let items = load_library_with_icons(&database_path, directory.path()).unwrap();
        let cached = items[0].icon.as_deref().unwrap();
        assert!(cached.ends_with(".png"));
        assert!(std::path::Path::new(cached).is_file());
        assert_eq!(
            Database::open(&database_path)
                .unwrap()
                .get("custom:demo")
                .unwrap()
                .unwrap()
                .icon
                .as_deref(),
            Some(cached)
        );
    }

    #[test]
    fn transfer_manager_cancels_only_the_registered_operation() {
        let manager = TransferManager::default();
        let first = manager.begin("first").unwrap();
        let second = manager.begin("second").unwrap();

        assert!(manager.cancel("first"));
        assert_eq!(first.cancellation.load(Ordering::Acquire), 1);
        assert_eq!(second.cancellation.load(Ordering::Acquire), 0);
        assert!(!manager.cancel("missing"));

        manager.finish("first");
        assert!(!manager.cancel("first"));
    }

    #[test]
    fn transfer_manager_rejects_work_after_shutdown_starts() {
        let manager = TransferManager::default();
        let transfer = manager.begin("active").unwrap();

        manager.cancel_all();

        assert_eq!(transfer.cancellation.load(Ordering::Acquire), 2);
        assert!(manager.begin("too-late").is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cancellation_terminates_the_download_process_group() {
        use std::{os::unix::process::CommandExt, process::Command, time::Instant};

        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30"]).process_group(0);
        let mut child = command.spawn().unwrap();
        let started = Instant::now();

        terminate_store_process(&mut child);
        let status = child.wait().unwrap();

        assert!(!status.success());
        assert!(started.elapsed().as_secs() < 3);
    }

    #[test]
    fn steam_store_operation_uses_the_connected_account() {
        let arguments = steam_store_arguments("orbit_user", "645630", "install");
        assert_eq!(arguments[0..2], ["+login", "orbit_user"]);
        assert_eq!(
            arguments[2..],
            [
                "+app_license_request",
                "645630",
                "+app_update",
                "645630",
                "validate",
                "+quit"
            ]
        );
        assert!(!arguments.iter().any(|argument| argument == "anonymous"));
    }

    #[test]
    fn steam_updates_do_not_request_a_new_license() {
        let arguments = steam_store_arguments("orbit_user", "645630", "update");
        assert!(!arguments
            .iter()
            .any(|argument| argument == "+app_license_request"));
    }

    #[test]
    fn steam_uninstall_uses_the_connected_account_and_quits() {
        assert_eq!(
            steam_uninstall_arguments("orbit_user", "645630"),
            ["+login", "orbit_user", "+app_uninstall", "645630", "+quit"]
        );
    }

    #[test]
    fn epic_install_is_fully_non_interactive_without_automatic_dlcs() {
        let arguments = epic_store_arguments("GameId", "install", std::path::Path::new("/orbit"));
        assert_eq!(
            arguments,
            [
                "-y",
                "install",
                "GameId",
                "--base-path",
                "/orbit/games/epic",
                "--skip-sdl",
                "--skip-dlcs"
            ]
        );
    }

    #[test]
    fn epic_verify_uses_only_supported_arguments() {
        assert_eq!(
            epic_store_arguments("GameId", "verify", std::path::Path::new("/orbit")),
            ["verify", "GameId"]
        );
    }

    #[test]
    fn explains_steam_free_license_failures() {
        let error = provider_operation_error(
            "steam",
            "ERROR! Failed to install app '1050280' (No subscription)",
        )
        .unwrap();
        assert!(error.contains("licença"));
        assert!(!error.contains("1050280"));
    }

    #[test]
    fn process_output_supports_carriage_return_progress_updates() {
        let chunks = collect_process_chunks(b"progress 1%\rprogress 2%\nfinished\n");
        assert_eq!(chunks, ["progress 1%", "progress 2%", "finished"]);
    }

    #[test]
    fn legendary_library_uses_real_titles_and_managed_executable() {
        let output = br#"[
          {"app_name":"HashOne","app_title":"A Short Hike","metadata":{"keyImages":[
            {"type":"DieselGameBox","url":"https://cdn1.epicgames.com/hash/wide.jpg","width":1920,"height":1080},
            {"type":"DieselGameBoxTall","url":"https://cdn1.epicgames.com/hash/tall.jpg","width":1200,"height":1600}
          ]}},
          {"appName":"HashTwo","appTitle":"Alan Wake 2"}
        ]"#;

        let items = parse_legendary_library(
            output,
            std::path::Path::new("/managed/legendary/current/bin/legendary"),
        )
        .unwrap();

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "epic:HashOne");
        assert_eq!(items[0].name, "A Short Hike");
        assert_eq!(items[1].name, "Alan Wake 2");
        assert_eq!(
            items[0].cover.as_deref(),
            Some("https://cdn1.epicgames.com/hash/tall.jpg")
        );
        assert_eq!(items[1].cover, None);
        assert_eq!(
            items[0].executable.as_deref(),
            Some("/managed/legendary/current/bin/legendary")
        );
    }

    #[test]
    fn legendary_cover_rejects_untrusted_or_css_unsafe_urls() {
        let output = br#"[
          {"app_name":"EvilHost","metadata":{"keyImages":[
            {"type":"DieselGameBoxTall","url":"https://cdn1.epicgames.com.evil.invalid/cover.jpg","width":1200,"height":1600},
            {"type":"DieselGameBox","url":"https://cdn1.epicgames.com/safe/wide.jpg","width":1920,"height":1080}
          ]}},
          {"app_name":"CssEscape","metadata":{"keyImages":[
            {"type":"DieselGameBoxTall","url":"https://cdn1.epicgames.com/bad/cover.jpg);color:red","width":1200,"height":1600}
          ]}}
        ]"#;

        let items =
            parse_legendary_library(output, std::path::Path::new("/managed/legendary")).unwrap();

        assert_eq!(
            items[0].cover.as_deref(),
            Some("https://cdn1.epicgames.com/safe/wide.jpg")
        );
        assert_eq!(items[1].cover, None);
    }

    #[test]
    fn legendary_cover_encodes_spaces_from_official_catalog_urls() {
        let output = br#"[{"app_name":"Gta","metadata":{"keyImages":[
          {"type":"DieselGameBoxTall","url":"https://cdn1.epicgames.com/item/Portrait Store Banner.jpg","width":1200,"height":1600}
        ]}}]"#;

        let items =
            parse_legendary_library(output, std::path::Path::new("/managed/legendary")).unwrap();

        assert_eq!(
            items[0].cover.as_deref(),
            Some("https://cdn1.epicgames.com/item/Portrait%20Store%20Banner.jpg")
        );
    }

    #[test]
    fn legendary_library_rejects_partial_or_duplicate_catalogs() {
        let executable = std::path::Path::new("/managed/legendary");
        assert!(parse_legendary_library(
            br#"[{"app_name":"valid","app_title":"Valid"},{"app_title":"Missing id"}]"#,
            executable,
        )
        .is_err());
        assert!(parse_legendary_library(
            br#"[{"app_name":"same"},{"app_name":"same"}]"#,
            executable,
        )
        .is_err());
    }

    #[test]
    fn legendary_installed_uses_physical_paths_and_keeps_owned_catalog() {
        let directory = tempfile::tempdir().unwrap();
        let existing = directory.path().join("installed-game");
        std::fs::create_dir(&existing).unwrap();
        let output = serde_json::to_vec(&serde_json::json!([
            {
                "app_name": "Installed",
                "title": "Installed title",
                "install_path": existing,
                "is_dlc": false,
                "manifest_path": null,
                "future_field": "ignored"
            },
            {
                "app_name": "Stale",
                "title": "Stale title",
                "install_path": directory.path().join("deleted-game")
            }
        ]))
        .unwrap();
        let installed = parse_legendary_installed(&output).unwrap();
        let mut items = parse_legendary_library(
            br#"[{"app_name":"Installed"},{"app_name":"Stale"},{"app_name":"OwnedOnly"}]"#,
            std::path::Path::new("/managed/legendary"),
        )
        .unwrap();

        merge_legendary_installations(&mut items, &installed);

        assert!(items.iter().all(|item| item.owned));
        assert!(items[0].installed);
        assert_eq!(items[0].working_directory.as_deref(), existing.to_str());
        assert!(!items[1].installed, "stale cache path is not installed");
        assert!(!items[2].installed, "owned does not imply installed");
    }

    #[test]
    fn legendary_installed_rejects_partial_or_duplicate_records() {
        assert!(parse_legendary_installed(br#"[{"app_name":"missing-path"}]"#).is_err());
        assert!(parse_legendary_installed(
            br#"[{"app_name":"same","install_path":"/one"},{"app_name":"same","install_path":"/two"}]"#,
        )
        .is_err());
        assert!(parse_legendary_installed(
            br#"[{"app_name":"relative","install_path":"games/relative"}]"#,
        )
        .is_err());
    }

    #[test]
    fn library_sync_manager_rejects_a_duplicate_provider() {
        let manager = LibrarySyncManager::default();
        let first = manager.begin("epic").unwrap();

        assert!(manager.begin("epic").is_err());
        assert!(manager.begin("gog").is_ok());
        drop(first);
        assert!(manager.begin("epic").is_ok());
    }

    #[test]
    fn library_sync_manager_blocks_new_work_during_shutdown() {
        let manager = LibrarySyncManager::default();
        let active = manager.begin("epic").unwrap();

        manager.cancel_all();

        assert!(active.control.is_cancelled());
        assert!(manager.begin("gog").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn legendary_library_command_times_out_and_terminates() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("legendary");
        std::fs::write(&executable, b"#!/bin/sh\nsleep 30\n").unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();
        let started = std::time::Instant::now();
        let manager = LibrarySyncManager::default();
        let guard = manager.begin("epic").unwrap();

        let error = run_legendary_json_command(
            &executable,
            &["list", "--json"],
            Duration::from_millis(50),
            &guard.control,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("excedeu"));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn legendary_library_command_stops_excessive_output() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("legendary");
        std::fs::write(
            &executable,
            b"#!/bin/sh\nhead -c 300000 /dev/zero >&2\nsleep 30\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();
        let manager = LibrarySyncManager::default();
        let guard = manager.begin("epic").unwrap();
        let started = std::time::Instant::now();

        let error = run_legendary_json_command(
            &executable,
            &["list", "--json"],
            Duration::from_secs(10),
            &guard.control,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("limite de segurança"));
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
