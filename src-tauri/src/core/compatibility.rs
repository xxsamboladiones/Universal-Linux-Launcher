use serde::Serialize;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    core::{launch::LaunchSpec, model::CompatibilityConfig},
    error::{LauncherError, Result},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub id: String,
    pub name: String,
    pub family: String,
    pub path: String,
    pub managed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityOverview {
    pub runtimes: Vec<RuntimeInfo>,
    pub gamemode: bool,
    pub mangohud: bool,
    pub gamescope: bool,
    pub dxvk: bool,
    pub vkd3d: bool,
    pub session_type: String,
    pub desktop: String,
    pub wayland: bool,
    pub terminal: Option<String>,
    pub prefix_root: String,
}

fn available(command: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {command}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn runtime_roots(data: &Path) -> Vec<(PathBuf, bool, &'static str)> {
    let mut roots = vec![
        (data.join("runtimes/proton"), true, "proton"),
        (data.join("runtimes/wine"), true, "wine"),
    ];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.extend([
            (
                home.join(".steam/root/compatibilitytools.d"),
                false,
                "proton",
            ),
            (
                home.join(".local/share/Steam/compatibilitytools.d"),
                false,
                "proton",
            ),
        ]);
    }
    roots
}

pub fn runtimes(data: &Path) -> Vec<RuntimeInfo> {
    let mut out = Vec::new();
    if available("wine") {
        out.push(RuntimeInfo {
            id: "system:wine".into(),
            name: "Wine do sistema".into(),
            family: "wine".into(),
            path: "wine".into(),
            managed: false,
        });
    }
    let mut seen = HashSet::new();
    for (root, managed, family) in runtime_roots(data) {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten().filter(|e| e.path().is_dir()) {
            let path = entry.path();
            let executable = if family == "proton" {
                path.join("proton")
            } else {
                path.join("bin/wine")
            };
            if !executable.is_file() || !seen.insert(executable.clone()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            out.push(RuntimeInfo {
                id: format!("{family}:{}", path.display()),
                name,
                family: family.into(),
                path: path.to_string_lossy().into_owned(),
                managed,
            });
        }
    }
    out
}

pub fn overview(data: &Path) -> CompatibilityOverview {
    let prefix_root = data.join("prefixes");
    for path in [
        &prefix_root,
        &data.join("runtimes/proton"),
        &data.join("runtimes/wine"),
        &data.join("logs/compatibility"),
    ] {
        let _ = fs::create_dir_all(path);
    }
    CompatibilityOverview {
        runtimes: runtimes(data),
        gamemode: available("gamemoderun"),
        mangohud: available("mangohud"),
        gamescope: available("gamescope"),
        dxvk: available("setup_dxvk.sh") || Path::new("/usr/share/dxvk").exists(),
        vkd3d: Path::new("/usr/share/vkd3d-proton").exists(),
        session_type: std::env::var("XDG_SESSION_TYPE").unwrap_or_default(),
        desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        wayland: std::env::var_os("WAYLAND_DISPLAY").is_some(),
        terminal: ["konsole", "kitty", "alacritty", "foot"]
            .into_iter()
            .find(|c| available(c))
            .map(str::to_string),
        prefix_root: prefix_root.to_string_lossy().into_owned(),
    }
}

pub fn create_prefix(data: &Path, item_id: &str) -> Result<String> {
    let safe = safe_item_id(item_id);
    let path = data.join("prefixes").join(safe);
    fs::create_dir_all(&path)?;
    Ok(path.to_string_lossy().into_owned())
}

fn safe_item_id(item_id: &str) -> String {
    item_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn steam_client_path(runtime_path: &Path) -> Option<PathBuf> {
    runtime_path
        .ancestors()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name == "compatibilitytools.d")
        })
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .and_then(|home| {
                    [home.join(".local/share/Steam"), home.join(".steam/root")]
                        .into_iter()
                        .find(|path| path.is_dir())
                })
        })
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

fn expand_steam_options(spec: &mut LaunchSpec) -> Result<bool> {
    let Some(index) = spec.args.iter().position(|arg| arg.contains("%command%")) else {
        return Ok(false);
    };
    let option = spec.args.remove(index);
    let (before, after) = option
        .split_once("%command%")
        .ok_or_else(|| LauncherError::InvalidArguments(option.clone()))?;
    for assignment in before.split_whitespace() {
        let (key, value) = assignment.split_once('=').ok_or_else(|| {
            LauncherError::InvalidArguments(format!("opção Steam inválida: {assignment}"))
        })?;
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(LauncherError::InvalidArguments(format!(
                "variável inválida: {key}"
            )));
        }
        spec.environment.insert(key.into(), unquote(value));
    }
    let trailing = after.split_whitespace().map(unquote).collect::<Vec<_>>();
    spec.args.splice(index..index, trailing);
    Ok(true)
}

fn steam_root() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    [
        home.join(".local/share/Steam"),
        home.join(".steam/steam"),
        home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
    ]
    .into_iter()
    .find(|root| root.join("ubuntu12_64/gameoverlayrenderer.so").is_file())
}

fn ensure_steam_running() -> Result<()> {
    let running = || {
        Command::new("pidof")
            .arg("steam")
            .output()
            .is_ok_and(|output| output.status.success())
    };
    if running() {
        return Ok(());
    }
    Command::new("steam")
        .arg("-silent")
        .spawn()
        .map_err(|error| LauncherError::LaunchFailed(error.to_string()))?;
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        if running() {
            return Ok(());
        }
    }
    Err(LauncherError::LaunchFailed(
        "A Steam não ficou pronta para o Overlay".into(),
    ))
}

fn enable_steam_overlay(spec: &mut LaunchSpec) -> Result<()> {
    ensure_steam_running()?;
    let root = steam_root().ok_or_else(|| {
        LauncherError::ExecutableNotFound("Steam nativa com gameoverlayrenderer.so".into())
    })?;
    let preload = [
        root.join("ubuntu12_32/gameoverlayrenderer.so"),
        root.join("ubuntu12_64/gameoverlayrenderer.so"),
    ]
    .into_iter()
    .filter(|path| path.is_file())
    .map(|path| path.to_string_lossy().into_owned())
    .collect::<Vec<_>>()
    .join(":");
    if preload.is_empty() {
        return Err(LauncherError::ExecutableNotFound(
            "bibliotecas do Steam Overlay".into(),
        ));
    }
    if let Some(existing) = spec.environment.get("LD_PRELOAD") {
        if !existing.is_empty() {
            spec.environment
                .insert("LD_PRELOAD".into(), format!("{preload}:{existing}"));
        }
    } else {
        spec.environment.insert("LD_PRELOAD".into(), preload);
    }
    spec.environment
        .insert("ENABLE_VK_LAYER_VALVE_steam_overlay_1".into(), "1".into());
    spec.environment
        .insert("SteamOverlayGameId".into(), "480".into());
    Ok(())
}

fn wrap(spec: &mut LaunchSpec, executable: String, mut prefix: Vec<String>) {
    prefix.push(spec.executable.clone());
    prefix.append(&mut spec.args);
    spec.executable = executable;
    spec.args = prefix;
}

pub fn apply(
    spec: &mut LaunchSpec,
    config: &CompatibilityConfig,
    data: &Path,
    item_id: &str,
) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    if expand_steam_options(spec)? {
        notes.push("Opções no formato Steam convertidas em variáveis de ambiente".into());
    }
    if config.steam_overlay {
        enable_steam_overlay(spec)?;
        notes.push("Steam Overlay injetado diretamente no processo do Proton".into());
    }
    if let Some(prefix) = &config.prefix_path {
        spec.environment.insert("WINEPREFIX".into(), prefix.clone());
    }
    if config.dxvk {
        spec.environment
            .insert("DXVK_LOG_LEVEL".into(), "info".into());
        notes.push("DXVK solicitado; use um prefixo que contenha DXVK".into());
    }
    if config.vkd3d {
        spec.environment.insert("VKD3D_DEBUG".into(), "warn".into());
        notes.push("VKD3D solicitado; use um prefixo que contenha VKD3D-Proton".into());
    }
    if let Some(id) = &config.runtime_id {
        let runtime = runtimes(data)
            .into_iter()
            .find(|r| &r.id == id)
            .ok_or_else(|| LauncherError::ExecutableNotFound(format!("runtime {id}")))?;
        if runtime.family == "proton" {
            let prefix = config.prefix_path.clone().unwrap_or_else(|| {
                data.join("prefixes")
                    .join(safe_item_id(item_id))
                    .to_string_lossy()
                    .into_owned()
            });
            fs::create_dir_all(&prefix)?;
            spec.environment
                .insert("STEAM_COMPAT_DATA_PATH".into(), prefix);
            let client_path = steam_client_path(Path::new(&runtime.path)).ok_or_else(|| {
                LauncherError::ExecutableNotFound("diretório do cliente Steam".into())
            })?;
            spec.environment.insert(
                "STEAM_COMPAT_CLIENT_INSTALL_PATH".into(),
                client_path.to_string_lossy().into_owned(),
            );
            wrap(
                spec,
                Path::new(&runtime.path)
                    .join("proton")
                    .to_string_lossy()
                    .into_owned(),
                vec!["run".into()],
            );
        } else {
            let exe = if runtime.id == "system:wine" {
                "wine".into()
            } else {
                Path::new(&runtime.path)
                    .join("bin/wine")
                    .to_string_lossy()
                    .into_owned()
            };
            wrap(spec, exe, vec![]);
        }
        notes.push(format!("Runtime: {}", runtime.name));
    }
    if config.mangohud {
        if !available("mangohud") {
            return Err(LauncherError::ExecutableNotFound("mangohud".into()));
        }
        wrap(spec, "mangohud".into(), vec![]);
    }
    if config.gamescope.enabled {
        if !available("gamescope") {
            return Err(LauncherError::ExecutableNotFound("gamescope".into()));
        }
        let mut a = Vec::new();
        for (flag, value) in [
            ("-w", config.gamescope.width),
            ("-h", config.gamescope.height),
            ("-W", config.gamescope.output_width),
            ("-H", config.gamescope.output_height),
            ("-r", config.gamescope.fps),
        ] {
            if let Some(v) = value {
                a.extend([flag.into(), v.to_string()]);
            }
        }
        if config.gamescope.fullscreen {
            a.push("-f".into())
        }
        if let Some(u) = &config.gamescope.upscaler {
            a.extend(["-U".into(), u.clone()])
        }
        a.push("--".into());
        wrap(spec, "gamescope".into(), a);
    }
    if config.gamemode {
        if !available("gamemoderun") {
            return Err(LauncherError::ExecutableNotFound("gamemoderun".into()));
        }
        wrap(spec, "gamemoderun".into(), vec![]);
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn spec(args: Vec<&str>) -> LaunchSpec {
        LaunchSpec {
            executable: "/games/test.exe".into(),
            args: args.into_iter().map(str::to_string).collect(),
            environment: HashMap::new(),
            working_directory: None,
        }
    }

    #[test]
    fn converts_steam_environment_launch_option() {
        let mut launch = spec(vec![
            "WINEDLLOVERRIDES=\"OnlineFix64=n,b;winhttp=n,b\" %command%",
        ]);
        assert!(expand_steam_options(&mut launch).unwrap());
        assert_eq!(
            launch.environment.get("WINEDLLOVERRIDES").unwrap(),
            "OnlineFix64=n,b;winhttp=n,b"
        );
        assert!(launch.args.is_empty());
    }

    #[test]
    fn preserves_arguments_after_command_marker() {
        let mut launch = spec(vec!["MANGOHUD=1 %command% -windowed"]);
        expand_steam_options(&mut launch).unwrap();
        assert_eq!(launch.args, ["-windowed"]);
        assert_eq!(launch.environment.get("MANGOHUD").unwrap(), "1");
    }
}
