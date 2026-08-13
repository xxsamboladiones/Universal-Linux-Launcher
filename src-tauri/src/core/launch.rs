use crate::{
    core::model::{LibraryItem, ProviderKind},
    error::{LauncherError, Result},
};
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub executable: String,
    pub args: Vec<String>,
    pub environment: std::collections::HashMap<String, String>,
    pub working_directory: Option<String>,
    pub target: LaunchTarget,
    pub terminal: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchTarget {
    Native,
    JavaArchive,
}

pub fn resolve(item: &LibraryItem) -> Result<LaunchSpec> {
    let (executable, args) = match item.provider {
        ProviderKind::Steam => {
            let executable = item.executable.clone().unwrap_or_else(|| "steam".into());
            if is_steam_client(&executable) {
                let mut arguments = vec![
                    "-applaunch".into(),
                    item.id.trim_start_matches("steam:").into(),
                ];
                arguments.extend(item.arguments.clone());
                (executable, arguments)
            } else {
                // Um executável escolhido manualmente para um item Steam é um
                // lançamento direto. `-applaunch` é argumento do cliente
                // Steam e corrompe JARs, scripts e binários Linux.
                (executable, item.arguments.clone())
            }
        }
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
        target: LaunchTarget::Native,
        terminal: item.terminal,
    };
    normalize_java_archive(&mut spec)?;
    // O terminal precisa ser o wrapper mais externo. A compatibilidade é
    // aplicada em `commands::launch_item` e chama esta etapa por último.
    Ok(spec)
}

pub fn apply_terminal(spec: &mut LaunchSpec, preferred_terminal: Option<&str>) -> Result<()> {
    if spec.terminal {
        let terminal = terminal_executable(preferred_terminal).ok_or_else(|| {
            LauncherError::ExecutableNotFound("terminal (konsole, kitty, alacritty ou foot)".into())
        })?;
        wrap_terminal(spec, terminal);
    }
    Ok(())
}

fn wrap_terminal(spec: &mut LaunchSpec, terminal: String) {
    let mut arguments = vec!["-e".into(), std::mem::take(&mut spec.executable)];
    arguments.append(&mut spec.args);
    spec.executable = terminal;
    spec.args = arguments;
}

fn is_steam_client(executable: &str) -> bool {
    Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("steam"))
}

fn normalize_java_archive(spec: &mut LaunchSpec) -> Result<()> {
    let configured_jar = Path::new(&spec.executable);
    let is_jar = configured_jar
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jar"));
    if !is_jar {
        return Ok(());
    }
    let jar = if configured_jar.is_absolute() {
        configured_jar.to_path_buf()
    } else if let Some(directory) = &spec.working_directory {
        Path::new(directory).join(configured_jar)
    } else {
        configured_jar.to_path_buf()
    };
    if !jar.is_file() {
        return Err(LauncherError::ExecutableNotFound(
            jar.to_string_lossy().into_owned(),
        ));
    }

    let java = bundled_java(&jar, spec.working_directory.as_deref())
        .or_else(|| java_home(&spec.environment))
        .or_else(|| java_from_path(&spec.environment))
        .ok_or_else(|| {
            LauncherError::ExecutableNotFound(
                "Java Runtime (instale um JRE/JDK ou configure JAVA_HOME)".into(),
            )
        })?;
    spec.executable = java;
    let jar = jar.to_string_lossy().into_owned();
    let mut arguments = vec!["-jar".into(), jar];
    arguments.append(&mut spec.args);
    spec.args = arguments;
    if spec.working_directory.is_none() {
        spec.working_directory = Path::new(&spec.args[1])
            .parent()
            .map(|path| path.to_string_lossy().into_owned());
    }
    spec.target = LaunchTarget::JavaArchive;
    Ok(())
}

fn bundled_java(jar: &Path, working_directory: Option<&str>) -> Option<String> {
    let roots = working_directory
        .map(PathBuf::from)
        .into_iter()
        .chain(jar.parent().map(Path::to_path_buf));
    let candidates = roots.flat_map(|root| {
        [
            root.join("jre/bin/java"),
            root.join("runtime/bin/java"),
            root.join("java/bin/java"),
        ]
    });
    first_executable(candidates)
}

fn java_home(environment: &std::collections::HashMap<String, String>) -> Option<String> {
    environment
        .get("JAVA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("JAVA_HOME").map(PathBuf::from))
        .and_then(|home| first_executable(std::iter::once(home.join("bin/java"))))
}

fn java_from_path(environment: &std::collections::HashMap<String, String>) -> Option<String> {
    let from_path = environment
        .get("PATH")
        .map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("PATH"))
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|path| path.join("java"));
    first_executable(from_path.chain([
        PathBuf::from("/usr/lib/jvm/default-runtime/bin/java"),
        PathBuf::from("/usr/lib/jvm/default/bin/java"),
        PathBuf::from("/usr/bin/java"),
    ]))
}

fn first_executable(candidates: impl IntoIterator<Item = std::path::PathBuf>) -> Option<String> {
    candidates
        .into_iter()
        .find(|candidate| is_executable_file(candidate))
        .map(|candidate| candidate.to_string_lossy().into_owned())
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    true
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
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn steam_is_structured() {
        let i = LibraryItem::new(
            "steam:730".into(),
            "Counter-Strike".into(),
            ItemKind::Game,
            ProviderKind::Steam,
        );
        let s = resolve(&i).unwrap();
        assert_eq!(s.executable, "steam");
        assert_eq!(s.args, ["-applaunch", "730"])
    }

    #[test]
    fn direct_steam_override_does_not_receive_applaunch() {
        let mut item = LibraryItem::new(
            "steam:42".into(),
            "Direct".into(),
            ItemKind::Game,
            ProviderKind::Steam,
        );
        item.executable = Some("/games/direct-launcher".into());
        item.arguments = vec!["--windowed".into()];

        let spec = resolve(&item).unwrap();

        assert_eq!(spec.executable, "/games/direct-launcher");
        assert_eq!(spec.args, ["--windowed"]);
    }

    #[cfg(unix)]
    #[test]
    fn jar_prefers_the_games_bundled_java_and_preserves_arguments() {
        let directory = tempfile::tempdir().unwrap();
        let jar = directory.path().join("game.JAR");
        let java = directory.path().join("jre/bin/java");
        fs::create_dir_all(java.parent().unwrap()).unwrap();
        fs::write(&jar, b"fixture").unwrap();
        fs::write(&java, b"#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(&java).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&java, permissions).unwrap();
        let mut item = LibraryItem::new(
            "steam:1050280".into(),
            "Kaion".into(),
            ItemKind::Game,
            ProviderKind::Steam,
        );
        item.executable = Some(jar.to_string_lossy().into_owned());
        item.working_directory = Some(directory.path().to_string_lossy().into_owned());
        item.arguments = vec!["--windowed".into()];
        item.environment.insert("GAME_TEST".into(), "1".into());

        let spec = resolve(&item).unwrap();

        assert_eq!(spec.executable, java.to_string_lossy());
        assert_eq!(
            spec.args,
            ["-jar", jar.to_string_lossy().as_ref(), "--windowed"]
        );
        assert_eq!(spec.target, LaunchTarget::JavaArchive);
        assert_eq!(
            spec.environment.get("GAME_TEST").map(String::as_str),
            Some("1")
        );
        assert_eq!(spec.working_directory.as_deref(), directory.path().to_str());
    }

    #[cfg(unix)]
    #[test]
    fn jar_launch_executes_the_bundled_java_with_cwd_env_and_arguments() {
        let directory = tempfile::tempdir().unwrap();
        let jar = directory.path().join("game.jar");
        let java = directory.path().join("jre/bin/java");
        let capture = directory.path().join("capture.txt");
        fs::create_dir_all(java.parent().unwrap()).unwrap();
        fs::write(&jar, b"fixture").unwrap();
        fs::write(
            &java,
            b"#!/bin/sh\nprintf '%s\\n%s\\n%s\\n' \"$PWD\" \"$GAME_TEST\" \"$*\" > \"$CAPTURE\"\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&java).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&java, permissions).unwrap();

        let mut item = LibraryItem::new(
            "custom:java".into(),
            "Java".into(),
            ItemKind::Game,
            ProviderKind::Custom,
        );
        item.executable = Some(jar.to_string_lossy().into_owned());
        item.working_directory = Some(directory.path().to_string_lossy().into_owned());
        item.arguments = vec!["--windowed".into()];
        item.environment
            .insert("GAME_TEST".into(), "enabled".into());
        item.environment
            .insert("CAPTURE".into(), capture.to_string_lossy().into_owned());

        let spec = resolve(&item).unwrap();
        let status = spawn(&spec, None).unwrap().wait().unwrap();

        assert!(status.success());
        let output = fs::read_to_string(capture).unwrap();
        let mut lines = output.lines();
        assert_eq!(lines.next(), directory.path().to_str());
        assert_eq!(lines.next(), Some("enabled"));
        assert_eq!(
            lines.next(),
            Some(format!("-jar {} --windowed", jar.display()).as_str())
        );
    }

    #[cfg(unix)]
    #[test]
    fn terminal_is_the_outermost_wrapper_for_a_java_archive() {
        let directory = tempfile::tempdir().unwrap();
        let jar = directory.path().join("game.jar");
        let java = directory.path().join("jre/bin/java");
        fs::create_dir_all(java.parent().unwrap()).unwrap();
        fs::write(&jar, b"fixture").unwrap();
        fs::write(&java, b"#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(&java).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&java, permissions).unwrap();
        let mut item = LibraryItem::new(
            "custom:java".into(),
            "Java".into(),
            ItemKind::Game,
            ProviderKind::Custom,
        );
        item.executable = Some(jar.to_string_lossy().into_owned());
        item.working_directory = Some(directory.path().to_string_lossy().into_owned());
        item.terminal = true;

        let mut spec = resolve(&item).unwrap();
        wrap_terminal(&mut spec, "test-terminal".into());

        assert_eq!(spec.executable, "test-terminal");
        assert_eq!(
            spec.args,
            [
                "-e",
                java.to_string_lossy().as_ref(),
                "-jar",
                jar.to_string_lossy().as_ref()
            ]
        );
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
        assert!(resolve(&i).is_err())
    }
}
