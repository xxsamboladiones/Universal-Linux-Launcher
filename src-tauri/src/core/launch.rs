use crate::{
    core::model::{LibraryItem, ProviderKind},
    error::{LauncherError, Result},
};
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    process::{Child, Command, Stdio},
};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub executable: String,
    pub args: Vec<String>,
    pub environment: std::collections::HashMap<String, String>,
    pub working_directory: Option<String>,
}
pub fn resolve(item: &LibraryItem, preferred_terminal: Option<&str>) -> Result<LaunchSpec> {
    let (executable, args) = match item.provider {
        ProviderKind::Steam => (item.executable.clone().unwrap_or_else(|| "steam".into()), {
            let mut a = vec![
                "-applaunch".into(),
                item.id.trim_start_matches("steam:").into(),
            ];
            a.extend(item.arguments.clone());
            a
        }),
        ProviderKind::Epic => (
            item.executable
                .clone()
                .unwrap_or_else(|| "legendary".into()),
            {
                let mut arguments =
                    vec!["launch".into(), item.id.trim_start_matches("epic:").into()];
                arguments.extend(item.arguments.clone());
                arguments
            },
        ),
        _ => (
            item.executable
                .clone()
                .ok_or_else(|| LauncherError::ExecutableNotFound(item.name.clone()))?,
            item.arguments.clone(),
        ),
    };
    if executable.contains('\0') || args.iter().any(|a| a.contains('\0')) {
        return Err(LauncherError::InvalidArguments("caractere NUL".into()));
    }
    let mut spec = LaunchSpec {
        executable,
        args,
        environment: item.environment.clone(),
        working_directory: item.working_directory.clone(),
    };
    if item.terminal {
        let terminal = terminal_executable(preferred_terminal).ok_or_else(|| {
            LauncherError::ExecutableNotFound("terminal (konsole, kitty, alacritty ou foot)".into())
        })?;
        let mut arguments = vec!["-e".into(), spec.executable];
        arguments.extend(spec.args);
        spec.executable = terminal;
        spec.args = arguments;
    }
    Ok(spec)
}

fn terminal_executable(preferred: Option<&str>) -> Option<String> {
    let candidates = preferred
        .map(str::to_string)
        .into_iter()
        .chain(std::env::var("TERMINAL").ok())
        .chain([
            "konsole".into(),
            "kitty".into(),
            "alacritty".into(),
            "foot".into(),
        ]);
    candidates.into_iter().find(|candidate| {
        std::process::Command::new(candidate)
            .arg("--version")
            .output()
            .is_ok()
    })
}
pub fn spawn(spec: &LaunchSpec, log_path: Option<&Path>) -> Result<Child> {
    if spec.executable.contains('/') && !Path::new(&spec.executable).exists() {
        return Err(LauncherError::ExecutableNotFound(spec.executable.clone()));
    }
    let mut cmd = Command::new(&spec.executable);
    // AppRun/linuxdeploy altera estas variáveis para carregar as bibliotecas do
    // próprio AppImage. Elas não podem vazar para Proton, Wine ou jogos nativos:
    // Proton, por exemplo, usa Python e deixa de encontrar `encodings` quando
    // herda PYTHONHOME/PYTHONPATH do bundle.
    for variable in [
        "PYTHONHOME",
        "PYTHONPATH",
        "LD_LIBRARY_PATH",
        "GI_TYPELIB_PATH",
        "GSETTINGS_SCHEMA_DIR",
        "GTK_PATH",
        "GTK_EXE_PREFIX",
        "GTK_DATA_PREFIX",
        "GDK_PIXBUF_MODULE_FILE",
    ] {
        cmd.env_remove(variable);
    }
    // Variáveis explícitas do item são aplicadas depois da limpeza. Isso mantém
    // LD_PRELOAD do Steam Overlay e qualquer configuração feita pelo usuário.
    cmd.args(&spec.args).envs(&spec.environment);
    if let Some(dir) = &spec.working_directory {
        cmd.current_dir(dir);
    }
    if let Some(path) = log_path {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let log = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(
            &log,
            "\n=== {} ===\n{} {:?}",
            chrono::Utc::now().to_rfc3339(),
            spec.executable,
            spec.args
        )
        .ok();
        cmd.stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
    }
    cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => LauncherError::ExecutableNotFound(spec.executable.clone()),
        std::io::ErrorKind::PermissionDenied => {
            LauncherError::PermissionDenied(spec.executable.clone())
        }
        _ => LauncherError::LaunchFailed(e.to_string()),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::*;
    #[test]
    fn steam_is_structured() {
        let i = LibraryItem::new(
            "steam:730".into(),
            "Counter-Strike".into(),
            ItemKind::Game,
            ProviderKind::Steam,
        );
        let s = resolve(&i, None).unwrap();
        assert_eq!(s.executable, "steam");
        assert_eq!(s.args, ["-applaunch", "730"])
    }
    #[test]
    fn rejects_nul() {
        let mut i = LibraryItem::new(
            "custom:x".into(),
            "x".into(),
            ItemKind::Custom,
            ProviderKind::Custom,
        );
        i.executable = Some("bad\0exe".into());
        assert!(resolve(&i, None).is_err())
    }
}
