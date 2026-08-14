use crate::{
    database::Database,
    error::{LauncherError, Result},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{AppHandle, Manager};

pub struct InstanceGuard {
    listener: UnixListener,
    path: PathBuf,
}
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
impl InstanceGuard {
    pub fn acquire() -> Result<Option<Self>> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = runtime.join("orbit-launcher.sock");
        match UnixListener::bind(&path) {
            Ok(listener) => Ok(Some(Self { listener, path })),
            Err(_) => {
                if let Ok(mut stream) = UnixStream::connect(&path) {
                    stream.write_all(b"show")?;
                    return Ok(None);
                }
                let _ = fs::remove_file(&path);
                Ok(Some(Self {
                    listener: UnixListener::bind(&path)?,
                    path,
                }))
            }
        }
    }
    pub fn listen(&self, app: AppHandle) {
        let Ok(listener) = self.listener.try_clone() else {
            return;
        };
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stream.is_ok() {
                    let app = app.clone();
                    let foreground = app.clone();
                    let _ = app.run_on_main_thread(move || show_main(&foreground));
                }
            }
        });
    }
}
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Retorna o caminho que continuará existindo depois que o processo atual
/// terminar. Dentro de AppImage, `current_exe()` aponta para `/tmp/.mount_*`,
/// enquanto `$APPIMAGE` aponta para o arquivo real escolhido pelo usuário.
fn persistent_executable_path() -> Result<PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        let path = PathBuf::from(appimage);
        if path.as_os_str().is_empty() {
            return Err(LauncherError::InvalidArguments(
                "APPIMAGE contém um caminho vazio".into(),
            ));
        }
        return Ok(path);
    }
    Ok(std::env::current_exe()?)
}

fn desktop_exec_path() -> Result<String> {
    let executable = persistent_executable_path()?
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    if executable.contains(['\n', '\r']) {
        return Err(LauncherError::InvalidArguments("caminho inválido".into()));
    }
    Ok(executable)
}

pub fn set_autostart(enabled: bool) -> Result<()> {
    let config = dirs::config_dir()
        .ok_or_else(|| LauncherError::LaunchFailed("XDG_CONFIG_HOME indisponível".into()))?;
    let dir = config.join("autostart");
    let file = dir.join("io.orbit.launcher.desktop");
    if !enabled {
        if file.exists() {
            fs::remove_file(file)?
        }
        return Ok(());
    }
    fs::create_dir_all(&dir)?;
    let executable = desktop_exec_path()?;
    fs::write(file,format!("[Desktop Entry]\nType=Application\nName=Orbit Launcher\nExec=\"{executable}\" --hidden\nIcon=io.orbit.launcher\nTerminal=false\nStartupWMClass=io.orbit.launcher\nX-KDE-autostart-after=panel\nX-GNOME-Autostart-enabled=true\n"))?;
    Ok(())
}
pub fn autostart_enabled() -> bool {
    dirs::config_dir().is_some_and(|p| p.join("autostart/io.orbit.launcher.desktop").is_file())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductStatus {
    pub autostart: bool,
    pub appimage: bool,
    pub executable: String,
}
pub fn status() -> ProductStatus {
    ProductStatus {
        autostart: autostart_enabled(),
        appimage: std::env::var_os("APPIMAGE").is_some(),
        executable: persistent_executable_path()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
    }
}

/// Registra o app para que Plasma/Wayland consiga resolver o `app_id` GTK para
/// o ícone correto, inclusive durante desenvolvimento e ao executar o AppImage.
pub fn ensure_desktop_integration() -> Result<()> {
    let data = dirs::data_dir().ok_or_else(|| LauncherError::NotFound("XDG_DATA_HOME".into()))?;
    let applications = data.join("applications");
    fs::create_dir_all(&applications)?;
    let mut changed = false;
    for (size, bytes) in [
        (32, include_bytes!("../icons/32x32.png").as_slice()),
        (64, include_bytes!("../icons/64x64.png").as_slice()),
        (128, include_bytes!("../icons/128x128.png").as_slice()),
        (256, include_bytes!("../icons/128x128@2x.png").as_slice()),
        (512, include_bytes!("../icons/icon.png").as_slice()),
    ] {
        let directory = data.join(format!("icons/hicolor/{size}x{size}/apps"));
        fs::create_dir_all(&directory)?;
        for icon_name in ["io.orbit.launcher.png", "orbit-launcher.png"] {
            changed |= write_if_changed(&directory.join(icon_name), bytes)?;
        }
    }
    let executable = desktop_exec_path()?;
    let desktop_icon = data
        .join("icons/hicolor/256x256/apps/orbit-launcher.png")
        .to_string_lossy()
        .replace('"', "\\\"");
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=Orbit Launcher\nGenericName=Game Launcher\nExec=\"{executable}\"\nIcon={desktop_icon}\nTerminal=false\nCategories=Game;Utility;\nStartupNotify=true\nStartupWMClass=orbit-launcher\n"
    );
    // `io.orbit.launcher` é o GTK app_id. O linuxdeploy/Tauri usa
    // `orbit-launcher` como desktop-id e WM_CLASS no AppImage (XWayland).
    // Os dois aliases permitem que o Plasma resolva o ícone nos dois backends.
    for desktop_name in ["io.orbit.launcher.desktop", "orbit-launcher.desktop"] {
        changed |= write_if_changed(&applications.join(desktop_name), desktop.as_bytes())?;
    }
    if changed {
        let _ = Command::new("gtk-update-icon-cache")
            .args(["-f", "-t"])
            .arg(data.join("icons/hicolor"))
            .status();
        let _ = Command::new("kbuildsycoca6")
            .arg("--noincremental")
            .status();
    }
    Ok(())
}

fn write_if_changed(path: &Path, contents: &[u8]) -> Result<bool> {
    if fs::read(path).ok().as_deref() == Some(contents) {
        return Ok(false);
    }
    fs::write(path, contents)?;
    Ok(true)
}

#[derive(Debug, Deserialize)]
struct UpdateManifest {
    version: String,
    url: String,
    sha256: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub configured: bool,
    pub current_version: String,
    pub available_version: Option<String>,
    pub can_install: bool,
}
fn version_numbers(version: &str) -> Option<Vec<u64>> {
    let core = version.split(['-', '+']).next()?;
    core.split('.').map(|part| part.parse().ok()).collect()
}
fn is_newer_version(candidate: &str, current: &str) -> bool {
    let (Some(mut candidate), Some(mut current)) =
        (version_numbers(candidate), version_numbers(current))
    else {
        return false;
    };
    let width = candidate.len().max(current.len());
    candidate.resize(width, 0);
    current.resize(width, 0);
    candidate > current
}
fn verified_update_manifest() -> Result<Option<UpdateManifest>> {
    let Ok(base) = std::env::var("ORBIT_UPDATE_URL") else {
        return Ok(None);
    };
    let key = std::env::var_os("ORBIT_UPDATE_PUBLIC_KEY")
        .map(PathBuf::from)
        .ok_or_else(|| {
            LauncherError::ProviderUnavailable("ORBIT_UPDATE_PUBLIC_KEY não configurada".into())
        })?;
    let temp = tempfile::tempdir()?;
    let manifest = temp.path().join("latest.json");
    let signature = temp.path().join("latest.json.sig");
    for (url, path) in [
        (format!("{base}/latest.json"), &manifest),
        (format!("{base}/latest.json.sig"), &signature),
    ] {
        if !Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--output",
            ])
            .arg(path)
            .arg(url)
            .status()?
            .success()
        {
            return Err(LauncherError::ProviderUnavailable(
                "Falha ao consultar atualização".into(),
            ));
        }
    }
    if !Command::new("openssl")
        .args(["dgst", "-sha256", "-verify"])
        .arg(key)
        .arg("-signature")
        .arg(signature)
        .arg(&manifest)
        .status()?
        .success()
    {
        return Err(LauncherError::ProviderUnavailable(
            "Assinatura da atualização inválida".into(),
        ));
    }
    let parsed: UpdateManifest = serde_json::from_slice(&fs::read(manifest)?)
        .map_err(|e| LauncherError::ProviderUnavailable(e.to_string()))?;
    if parsed.sha256.len() != 64 || !parsed.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(LauncherError::ProviderUnavailable(
            "Manifesto de atualização inválido".into(),
        ));
    }
    Ok(Some(parsed))
}
pub fn check_update() -> Result<UpdateStatus> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let manifest = verified_update_manifest()?;
    Ok(UpdateStatus {
        configured: manifest.is_some(),
        available_version: manifest.and_then(|m| {
            if is_newer_version(&m.version, &current) {
                Some(m.version)
            } else {
                None
            }
        }),
        can_install: std::env::var_os("APPIMAGE").is_some(),
        current_version: current,
    })
}
pub fn install_update() -> Result<()> {
    let manifest = verified_update_manifest()?.ok_or_else(|| {
        LauncherError::ProviderUnavailable("Atualização automática não configurada".into())
    })?;
    if !is_newer_version(&manifest.version, env!("CARGO_PKG_VERSION")) {
        return Err(LauncherError::InvalidArguments(
            "o manifesto não contém uma versão mais nova".into(),
        ));
    }
    let appimage = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .ok_or_else(|| {
            LauncherError::ProviderUnavailable("Em pacotes Arch, atualize com pacman/AUR".into())
        })?;
    let partial = appimage.with_extension("AppImage.part");
    if !Command::new("curl")
        .args(["--fail", "--location", "--continue-at", "-", "--output"])
        .arg(&partial)
        .arg(manifest.url)
        .status()?
        .success()
    {
        return Err(LauncherError::LaunchFailed(
            "Download da atualização interrompido".into(),
        ));
    }
    let output = Command::new("sha256sum").arg(&partial).output()?;
    let actual = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if actual != manifest.sha256.to_ascii_lowercase() {
        return Err(LauncherError::ProviderUnavailable(
            "SHA-256 da atualização não confere".into(),
        ));
    }
    let permissions = fs::metadata(&appimage)?.permissions();
    fs::set_permissions(&partial, permissions)?;
    let backup = appimage.with_extension("AppImage.old");
    if backup.exists() {
        fs::remove_file(&backup)?
    }
    fs::rename(&appimage, &backup)?;
    if let Err(error) = fs::rename(&partial, &appimage) {
        let _ = fs::rename(&backup, &appimage);
        return Err(error.into());
    }
    Ok(())
}

pub fn export_backup(db: &Database, data: &Path, destination: &Path) -> Result<()> {
    if destination.extension().and_then(|v| v.to_str()) != Some("orbitbackup") {
        return Err(LauncherError::InvalidArguments(
            "o backup deve terminar em .orbitbackup".into(),
        ));
    }
    let staging = tempfile::tempdir()?;
    let snapshot = staging.path().join("orbit.db");
    db.backup_to(&snapshot)?;
    fs::write(
        staging.path().join("backup.json"),
        format!(
            "{{\"format\":1,\"createdAt\":\"{}\",\"app\":\"io.orbit.launcher\"}}",
            chrono::Utc::now().to_rfc3339()
        ),
    )?;
    let partial = destination.with_extension("orbitbackup.part");
    let status = Command::new("tar")
        .arg("-czf")
        .arg(&partial)
        .arg("-C")
        .arg(staging.path())
        .args(["orbit.db", "backup.json"])
        .status()?;
    if !status.success() {
        return Err(LauncherError::LaunchFailed(
            "Falha ao compactar backup".into(),
        ));
    }
    fs::rename(partial, destination)?;
    let _ = data;
    Ok(())
}
pub fn import_backup(db: &mut Database, source: &Path) -> Result<()> {
    if !source.is_file() {
        return Err(LauncherError::NotFound(source.display().to_string()));
    }
    let listing = Command::new("tar").arg("-tzf").arg(source).output()?;
    if !listing.status.success() {
        return Err(LauncherError::InvalidArguments("backup corrompido".into()));
    }
    let listing_text = String::from_utf8_lossy(&listing.stdout);
    let entries = listing_text.lines().collect::<Vec<_>>();
    if entries
        .iter()
        .any(|entry| !matches!(*entry, "orbit.db" | "backup.json"))
    {
        return Err(LauncherError::InvalidArguments(
            "backup contém arquivos inesperados".into(),
        ));
    }
    let staging = tempfile::tempdir()?;
    if !Command::new("tar")
        .arg("-xzf")
        .arg(source)
        .arg("-C")
        .arg(staging.path())
        .status()?
        .success()
    {
        return Err(LauncherError::InvalidArguments(
            "não foi possível extrair backup".into(),
        ));
    }
    let metadata = fs::read_to_string(staging.path().join("backup.json"))?;
    if !metadata.contains("\"app\":\"io.orbit.launcher\"") {
        return Err(LauncherError::InvalidArguments(
            "backup não pertence ao Orbit".into(),
        ));
    }
    db.restore_from(&staging.path().join("orbit.db"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn updater_only_accepts_newer_versions() {
        assert!(is_newer_version("1.2.0", "1.1.9"));
        assert!(is_newer_version("2.0", "1.99.99"));
        assert!(!is_newer_version("1.0.0", "1.0.0"));
        assert!(!is_newer_version("0.9.9", "1.0.0"));
        assert!(!is_newer_version("inválida", "1.0.0"));
    }
    #[test]
    fn backup_roundtrip_restores_settings() {
        let root = tempfile::tempdir().unwrap();
        let original = Database::open(&root.path().join("original.db")).unwrap();
        let mut settings = original.settings().unwrap();
        settings.theme = "system".into();
        original.save_settings(&settings).unwrap();
        let archive = root.path().join("library.orbitbackup");
        export_backup(&original, root.path(), &archive).unwrap();
        let mut restored = Database::open(&root.path().join("restored.db")).unwrap();
        import_backup(&mut restored, &archive).unwrap();
        assert_eq!(restored.settings().unwrap().theme, "system");
    }
}
