use super::{find_on_path, Operation};
use crate::error::{LauncherError, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

#[derive(Clone, Debug, Deserialize)]
struct SignedManifest {
    id: String,
    version: String,
    url: String,
    sha256: String,
    executable: String,
    archive: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyProgress {
    pub dependency: String,
    pub stage: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

/// Byte-level progress reported by a store client while it transfers a game.
///
/// Keeping this separate from [`DependencyProgress`] lets the operation queue
/// consume provider output without coupling the parser to Tauri events or the
/// database. The UI derives the percentage from these two integer values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TransferProgress {
    pub(crate) downloaded_bytes: u64,
    pub(crate) total_bytes: u64,
}

/// Parses one output line emitted by a managed provider.
///
/// SteamCMD reports authoritative byte counts alongside its display
/// percentage, for example:
/// `Update state (...) downloading, progress: 29.07 (3299539690 / 11348895985)`.
/// We intentionally parse the byte pair instead of the decimal percentage so
/// progress remains precise and locale-independent. Providers without a
/// stable byte-level format return `None` until a dedicated parser is added.
pub(crate) fn parse_transfer_progress(provider: &str, line: &str) -> Option<TransferProgress> {
    if !provider.eq_ignore_ascii_case("steam") && !provider.eq_ignore_ascii_case("steamcmd") {
        return None;
    }

    parse_steamcmd_transfer_progress(line)
}

fn parse_steamcmd_transfer_progress(line: &str) -> Option<TransferProgress> {
    let line = strip_ansi_sequences(line);
    let normalized = line.to_ascii_lowercase();
    if !normalized.contains("update state") || !normalized.contains("downloading") {
        return None;
    }

    // Starting after `progress:` prevents the state code's `(0x61)` from
    // being mistaken for the byte pair.
    let progress_marker = normalized.find("progress:")? + "progress:".len();
    let remainder = &line[progress_marker..];
    let pair_start = remainder.find('(')? + 1;
    let pair_end = remainder[pair_start..].find(')')? + pair_start;
    let (downloaded, total) = remainder[pair_start..pair_end].split_once('/')?;
    let downloaded_bytes = downloaded.trim().parse::<u64>().ok()?;
    let total_bytes = total.trim().parse::<u64>().ok()?;

    (total_bytes > 0 && downloaded_bytes <= total_bytes).then_some(TransferProgress {
        downloaded_bytes,
        total_bytes,
    })
}

pub struct DependencyManager {
    root: PathBuf,
}
impl DependencyManager {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn executable(&self, id: &str) -> Option<PathBuf> {
        let bin = self.root.join("providers").join(id).join("current/bin");
        let candidates: &[&str] = match id {
            "wine-ge" => &["wine", "wine64"],
            "battlenet-client" => &["Battle.net-Setup.exe"],
            _ => &[id],
        };
        candidates
            .iter()
            .map(|name| bin.join(name))
            .find(|path| path.is_file())
            .or_else(|| match id {
                "wine-ge" => find_on_path("wine").or_else(|| find_on_path("wine64")),
                _ => find_on_path(id),
            })
    }
    pub fn installed_version(&self, id: &str) -> Option<String> {
        let provider = self.root.join("providers").join(id).join("current");
        let recorded = fs::read_to_string(provider.join(".orbit-version"))
            .ok()
            .map(|version| version.trim().to_string())
            .filter(|version| !version.is_empty());
        if recorded.is_some() {
            return recorded;
        }
        // Instalações criadas por versões anteriores do Orbit não tinham o
        // marcador. Retornar a versão da receita é seguro e, principalmente,
        // não executa CLIs interativas (SteamCMD não suporta `--version`).
        if provider.is_dir() {
            return builtin_manifest(id).ok().map(|manifest| manifest.version);
        }
        None
    }
    pub fn install_with_progress<F>(&self, id: &str, mut notify: F) -> Result<()>
    where
        F: FnMut(DependencyProgress),
    {
        if self.executable(id).is_some() {
            notify(progress(id, "completed", 0, 0));
            return Ok(());
        }
        notify(progress(id, "resolving", 0, 0));
        let manifest = self.manifest(id)?;
        self.download_and_install_with_progress(&manifest, &mut notify)
    }

    fn manifest(&self, id: &str) -> Result<SignedManifest> {
        let dir = self.root.join("manifests");
        let manifest_path = dir.join(format!("{id}.json"));
        let signature = dir.join(format!("{id}.json.sig"));
        let public_key = dir.join("orbit-dependencies.pem");

        // A recipe shipped in the compiled application is itself part of the
        // trusted release. Signed files in the data directory are optional
        // overrides for updating the catalog without weakening that trust.
        if !manifest_path.exists() && !signature.exists() {
            return builtin_manifest(id);
        }
        for path in [&manifest_path, &signature, &public_key] {
            if !path.is_file() {
                return Err(LauncherError::ProviderUnavailable(format!(
                    "Arquivo de confiança ausente: {}",
                    path.display()
                )));
            }
        }
        let verified = external_command("openssl")
            .args(["dgst", "-sha256", "-verify"])
            .arg(&public_key)
            .args(["-signature"])
            .arg(&signature)
            .arg(&manifest_path)
            .status()?
            .success();
        if !verified {
            return Err(LauncherError::ProviderUnavailable(
                "Assinatura do manifesto inválida".into(),
            ));
        }
        let manifest: SignedManifest = serde_json::from_slice(&fs::read(&manifest_path)?)
            .map_err(|e| LauncherError::ProviderUnavailable(e.to_string()))?;
        if manifest.id != id
            || !manifest.sha256.chars().all(|c| c.is_ascii_hexdigit())
            || manifest.sha256.len() != 64
        {
            return Err(LauncherError::ProviderUnavailable(
                "Manifesto inválido".into(),
            ));
        }
        Ok(manifest)
    }

    fn download_and_install_with_progress(
        &self,
        manifest: &SignedManifest,
        notify: &mut dyn FnMut(DependencyProgress),
    ) -> Result<()> {
        let id = manifest.id.as_str();
        let downloads = self.root.join("downloads");
        fs::create_dir_all(&downloads)?;
        let partial = downloads.join(format!("{id}-{}.part", manifest.version));
        notify(progress(
            id,
            "downloading",
            file_size(&partial),
            manifest.size.unwrap_or(0),
        ));
        let mut child = external_command("curl")
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--continue-at",
                "-",
                "--connect-timeout",
                "15",
                "--speed-limit",
                "1024",
                "--speed-time",
                "30",
                "--max-time",
                "600",
                "--retry",
                "3",
                "--retry-delay",
                "1",
                "--output",
            ])
            .arg(&partial)
            .arg(&manifest.url)
            .spawn()?;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            notify(progress(
                id,
                "downloading",
                file_size(&partial),
                manifest.size.unwrap_or(0),
            ));
            thread::sleep(Duration::from_millis(250));
        };
        if !status.success() {
            return Err(LauncherError::ProviderUnavailable(
                "Download interrompido; será retomado na próxima tentativa".into(),
            ));
        }
        notify(progress(
            id,
            "verifying",
            file_size(&partial),
            manifest.size.unwrap_or(0),
        ));
        let output = external_command("sha256sum").arg(&partial).output()?;
        let actual = String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if actual != manifest.sha256.to_ascii_lowercase() {
            let _ = fs::remove_file(&partial);
            return Err(LauncherError::ProviderUnavailable(
                "SHA-256 não confere; o download inválido foi descartado para permitir nova tentativa".into(),
            ));
        }
        let provider = self.root.join("providers").join(id);
        let staging = provider.join(format!(".staging-{}", manifest.version));
        notify(progress(
            id,
            "installing",
            file_size(&partial),
            manifest.size.unwrap_or(0),
        ));
        if staging.exists() {
            fs::remove_dir_all(&staging)?
        }
        fs::create_dir_all(&staging)?;
        match manifest.archive.as_deref() {
            Some("tar.gz") => {
                let listing = external_command("tar")
                    .args(["-tzf"])
                    .arg(&partial)
                    .output()?;
                let unsafe_entry = String::from_utf8_lossy(&listing.stdout)
                    .lines()
                    .any(|entry| {
                        let path = std::path::Path::new(entry);
                        path.is_absolute()
                            || path
                                .components()
                                .any(|part| matches!(part, std::path::Component::ParentDir))
                    });
                if !listing.status.success() || unsafe_entry {
                    return Err(LauncherError::ProviderUnavailable(
                        "Pacote contém caminhos inseguros".into(),
                    ));
                }
                if !external_command("tar")
                    .args(["-xzf"])
                    .arg(&partial)
                    .arg("-C")
                    .arg(&staging)
                    .status()?
                    .success()
                {
                    return Err(LauncherError::ProviderUnavailable(
                        "Falha ao extrair pacote".into(),
                    ));
                }
            }
            Some(kind) => {
                return Err(LauncherError::ProviderUnavailable(format!(
                    "Formato não permitido: {kind}"
                )))
            }
            None => {
                let target = staging.join(&manifest.executable);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?
                }
                fs::copy(&partial, &target)?;
                fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
            }
        }
        prepare_entrypoint(id, &staging)?;
        if !staging.join(&manifest.executable).is_file() {
            return Err(LauncherError::ProviderUnavailable(
                "Executável declarado não existe no pacote".into(),
            ));
        }
        fs::write(staging.join(".orbit-version"), &manifest.version)?;
        let current = provider.join("current");
        let backup = provider.join("rollback");
        if backup.exists() {
            fs::remove_dir_all(&backup)?
        }
        if current.exists() {
            fs::rename(&current, &backup)?
        }
        if let Err(error) = fs::rename(&staging, &current) {
            if backup.exists() {
                let _ = fs::rename(&backup, &current);
            }
            return Err(error.into());
        }
        notify(progress(
            id,
            "completed",
            file_size(&partial),
            manifest.size.unwrap_or(0),
        ));
        Ok(())
    }
    pub fn rollback(&self, id: &str) -> Result<()> {
        let provider = self.root.join("providers").join(id);
        let current = provider.join("current");
        let backup = provider.join("rollback");
        if !backup.exists() {
            return Err(LauncherError::NotFound(format!("rollback de {id}")));
        }
        let failed = provider.join("failed-current");
        if failed.exists() {
            fs::remove_dir_all(&failed)?
        }
        if current.exists() {
            fs::rename(&current, &failed)?
        }
        fs::rename(backup, current)?;
        Ok(())
    }
}

fn builtin_manifest(id: &str) -> Result<SignedManifest> {
    if std::env::consts::ARCH != "x86_64" {
        return Err(LauncherError::ProviderUnavailable(format!(
            "{id} ainda não possui pacote automático para {}",
            std::env::consts::ARCH
        )));
    }
    let manifest = match id {
        "steamcmd" => SignedManifest {
            id: id.into(),
            version: "2026-08-13".into(),
            url: "https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz".into(),
            sha256: "cebf0046bfd08cf45da6bc094ae47aa39ebf4155e5ede41373b579b8f1071e7c".into(),
            executable: "bin/steamcmd".into(),
            archive: Some("tar.gz".into()),
            size: Some(2_428_561),
        },
        "legendary" => SignedManifest {
            id: id.into(),
            version: "0.21.0".into(),
            url: "https://github.com/legendary-gl/legendary/releases/download/0.21.0/legendary_linux_x64".into(),
            sha256: "c83d1595a9e2cbae4e66b69ecaa1f8649da99b617a72f93312d342e6a5a799c7".into(),
            executable: "bin/legendary".into(),
            archive: None,
            size: Some(14_022_624),
        },
        "gogdl" => SignedManifest {
            id: id.into(),
            version: "1.3.0".into(),
            url: "https://github.com/Heroic-Games-Launcher/heroic-gogdl/releases/download/v1.3.0/gogdl_linux_x86_64".into(),
            sha256: "cba013d42767c808237c437335ab1d56f58405d07e8f37b3324d264ea5c49655".into(),
            executable: "bin/gogdl".into(),
            archive: None,
            size: Some(1_563_756),
        },
        "wine-ge" => {
            return Err(LauncherError::ProviderUnavailable(
                "Wine não foi encontrado. No CachyOS, instale o pacote wine ou wine-staging e tente novamente".into(),
            ))
        }
        "battlenet-client" => SignedManifest {
            id: id.into(),
            version: "1.0.66".into(),
            url: "https://downloader.battle.net/download/installer/win/1.0.66/Battle.net-Setup.exe".into(),
            sha256: "de5d32d4ea5eed5a9e120027fb68b370976dbfecc8f2a8f91305977f0b87fcaf".into(),
            executable: "bin/Battle.net-Setup.exe".into(),
            archive: None,
            size: Some(4_896_464),
        },
        _ => {
            return Err(LauncherError::ProviderUnavailable(format!(
                "Componente desconhecido: {id}"
            )))
        }
    };
    Ok(manifest)
}

fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map_or(0, |metadata| metadata.len())
}

fn progress(id: &str, stage: &str, downloaded_bytes: u64, total_bytes: u64) -> DependencyProgress {
    DependencyProgress {
        dependency: id.into(),
        stage: stage.into(),
        downloaded_bytes,
        total_bytes,
    }
}

fn prepare_entrypoint(id: &str, staging: &Path) -> Result<()> {
    if id != "steamcmd" {
        return Ok(());
    }
    let script = staging.join("steamcmd.sh");
    if !script.is_file() {
        return Err(LauncherError::ProviderUnavailable(
            "O pacote SteamCMD não contém steamcmd.sh".into(),
        ));
    }
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
    let bin = staging.join("bin");
    fs::create_dir_all(&bin)?;
    let wrapper = bin.join("steamcmd");
    fs::write(
        &wrapper,
        "#!/bin/sh\nroot=$(CDPATH= cd -- \"$(dirname -- \"$0\")/..\" && pwd)\nexec \"$root/steamcmd.sh\" \"$@\"\n",
    )?;
    fs::set_permissions(wrapper, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn external_command(program: &str) -> Command {
    let mut command = Command::new(program);
    // AppImages inject library paths that can make host utilities such as
    // curl, tar and openssl load incompatible bundled libraries.
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
    command
}

pub enum CredentialVault {
    SecretService,
    KWallet,
    Unavailable,
}
impl CredentialVault {
    pub fn detect() -> Self {
        if find_on_path("secret-tool").is_some() {
            Self::SecretService
        } else if find_on_path("kwallet-query").is_some() {
            Self::KWallet
        } else {
            Self::Unavailable
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::SecretService => "Secret Service",
            Self::KWallet => "KWallet",
            Self::Unavailable => "Indisponível",
        }
    }
    pub fn store(&self, provider: &str, token: &str) -> Result<()> {
        match self {
            Self::SecretService => {
                let mut child = Command::new("secret-tool")
                    .args([
                        "store",
                        "--label=Orbit Launcher",
                        "application",
                        "orbit-launcher",
                        "provider",
                        provider,
                    ])
                    .stdin(Stdio::piped())
                    .spawn()?;
                child
                    .stdin
                    .take()
                    .ok_or_else(|| LauncherError::LaunchFailed("stdin indisponível".into()))?
                    .write_all(token.as_bytes())?;
                if !child.wait()?.success() {
                    return Err(LauncherError::LaunchFailed(
                        "Secret Service recusou o token".into(),
                    ));
                }
                Ok(())
            }
            Self::KWallet => {
                let wallet = ["kdewallet6", "kdewallet"]
                    .into_iter()
                    .find(|name| {
                        Command::new("kwallet-query")
                            .args(["-l", name])
                            .output()
                            .is_ok_and(|output| output.status.success())
                    })
                    .ok_or_else(|| {
                        LauncherError::ProviderUnavailable("Nenhuma carteira KDE disponível".into())
                    })?;
                let mut child = Command::new("kwallet-query")
                    .args(["-w", provider, "-f", "Orbit Launcher", wallet])
                    .stdin(Stdio::piped())
                    .spawn()?;
                child
                    .stdin
                    .take()
                    .ok_or_else(|| LauncherError::LaunchFailed("stdin indisponível".into()))?
                    .write_all(token.as_bytes())?;
                if !child.wait()?.success() {
                    return Err(LauncherError::LaunchFailed(
                        "KWallet recusou o token".into(),
                    ));
                }
                Ok(())
            }
            Self::Unavailable => Err(LauncherError::ProviderUnavailable(
                "Secret Service/KWallet indisponível".into(),
            )),
        }
    }
}

pub struct ProviderManager {
    root: PathBuf,
}
const GOG_CLIENT_ID: &str = "46899977096215655";
pub const GOG_LOGIN_URL: &str = "https://auth.gog.com/auth?client_id=46899977096215655&redirect_uri=https%3A%2F%2Fembed.gog.com%2Fon_login_success%3Forigin%3Dclient&response_type=code&layout=galaxy";

impl ProviderManager {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn authenticate_command(
        &self,
        provider: &str,
        user: Option<&str>,
    ) -> Result<(String, Vec<String>)> {
        let deps = DependencyManager::new(self.root.clone());
        match provider {
            "epic" => Ok((
                deps.executable("legendary")
                    .ok_or_else(|| LauncherError::ExecutableNotFound("legendary".into()))?
                    .to_string_lossy()
                    .into_owned(),
                vec!["auth".into()],
            )),
            "steam" => {
                let user = valid_named_steam_account(user.ok_or_else(|| {
                    LauncherError::InvalidArguments(
                        "Informe o nome de usuário da Steam para conectar a conta".into(),
                    )
                })?)?;
                Ok((
                    deps.executable("steamcmd")
                        .ok_or_else(|| LauncherError::ExecutableNotFound("steamcmd".into()))?
                        .to_string_lossy()
                        .into_owned(),
                    vec!["+login".into(), user, "+info".into(), "+quit".into()],
                ))
            }
            "gog" => {
                let code = valid_gog_authorization_code(user.ok_or_else(|| {
                    LauncherError::InvalidArguments(
                        "Conclua o login no navegador e cole a URL final do GOG".into(),
                    )
                })?)?;
                Ok((
                    deps.executable("gogdl")
                        .ok_or_else(|| LauncherError::ExecutableNotFound("gogdl".into()))?
                        .to_string_lossy()
                        .into_owned(),
                    vec![
                        "--auth-config-path".into(),
                        self.gog_auth_path().to_string_lossy().into_owned(),
                        "auth".into(),
                        "--code".into(),
                        code,
                    ],
                ))
            }
            "battlenet" => Ok((
                deps.executable("wine-ge")
                    .ok_or_else(|| LauncherError::ExecutableNotFound("wine".into()))?
                    .to_string_lossy()
                    .into_owned(),
                vec![deps
                    .executable("battlenet-client")
                    .ok_or_else(|| {
                        LauncherError::ExecutableNotFound("Battle.net-Setup.exe".into())
                    })?
                    .to_string_lossy()
                    .into_owned()],
            )),
            _ => Err(LauncherError::ProviderUnavailable(provider.into())),
        }
    }

    pub fn gog_auth_path(&self) -> PathBuf {
        self.root.join("providers/gog/auth.json")
    }

    pub fn gog_effective_auth_path(&self) -> Option<PathBuf> {
        let own = self.gog_auth_path();
        if valid_gog_auth_file(&own) {
            return Some(own);
        }
        let heroic = dirs::home_dir()?.join(".config/heroic/gog_store/auth.json");
        valid_gog_auth_file(&heroic).then_some(heroic)
    }

    pub fn gog_authenticated(&self) -> bool {
        self.gog_effective_auth_path().is_some()
    }

    pub fn battlenet_prefix_path(&self) -> PathBuf {
        self.root.join("prefixes/battlenet")
    }

    pub fn battlenet_launcher_path(&self) -> Option<PathBuf> {
        let root = self
            .battlenet_prefix_path()
            .join("drive_c/Program Files (x86)/Battle.net");
        ["Battle.net Launcher.exe", "Battle.net.exe"]
            .into_iter()
            .map(|name| root.join(name))
            .find(|path| valid_windows_executable(path))
    }

    pub fn battlenet_installed(&self) -> bool {
        self.battlenet_launcher_path().is_some()
    }

    /// Console transcript maintained by the managed SteamCMD runtime.
    /// Passwords are read by SteamCMD from the terminal and Orbit must never
    /// copy this file into errors or application logs.
    pub fn steam_log_path(&self) -> PathBuf {
        self.root
            .join("providers/steamcmd/current/linux32/logs/console_log.txt")
    }

    pub fn steam_log_len(&self) -> u64 {
        fs::metadata(self.steam_log_path()).map_or(0, |metadata| metadata.len())
    }

    /// Reads only the portion appended by the authentication attempt. This
    /// prevents an old successful attempt from masking a new failed attempt.
    pub fn steam_account_from_log_since(&self, offset: u64) -> Option<String> {
        let contents = fs::read(self.steam_log_path()).ok()?;
        let start = usize::try_from(offset)
            .ok()
            .filter(|offset| *offset <= contents.len())
            .unwrap_or(0);
        steam_account_from_output(&String::from_utf8_lossy(&contents[start..]))
    }

    /// Reads the current Orbit transcript and legacy transcript locations.
    /// The fallbacks allow accounts authenticated by development builds to be
    /// migrated without inspecting or copying Steam's persisted auth token.
    pub fn steam_account_from_log(&self) -> Option<String> {
        let current = self.steam_log_path();
        let provider_root = self.root.join("providers/steamcmd");
        let legacy_root = provider_root.join("current");
        [
            current,
            provider_root.join("authentication.log"),
            legacy_root.join("authentication.log"),
            legacy_root.join("logs/authentication.log"),
        ]
        .into_iter()
        .find_map(|path| {
            fs::read(path)
                .ok()
                .and_then(|bytes| steam_account_from_output(&String::from_utf8_lossy(&bytes)))
        })
    }

    /// Checks whether Steam still has a persisted named-login cache without
    /// returning, logging or copying any cache key or token value.
    pub fn steam_login_cache_exists(&self) -> bool {
        let Some(home) = dirs::home_dir() else {
            return false;
        };
        [
            home.join(".local/share/Steam/config/config.vdf"),
            home.join(".steam/root/config/config.vdf"),
            home.join(".steam/steam/config/config.vdf"),
            home.join(".steam/steamcmd/config/config.vdf"),
            home.join("Steam/config/config.vdf"),
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam/config/config.vdf"),
            self.root
                .join("providers/steamcmd/current/config/config.vdf"),
        ]
        .into_iter()
        .any(|path| {
            fs::read(path)
                .ok()
                .is_some_and(|contents| vdf_section_has_entry(&contents, "ConnectCache"))
        })
    }
    pub fn operation(provider: &str, item_id: &str, action: &str) -> Operation {
        let now = chrono::Utc::now().to_rfc3339();
        Operation {
            id: uuid::Uuid::new_v4().to_string(),
            provider: provider.into(),
            item_id: item_id.into(),
            action: action.into(),
            state: "queued".into(),
            downloaded_bytes: 0,
            total_bytes: 0,
            bytes_per_second: 0,
            error: None,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

fn valid_gog_authorization_code(input: &str) -> Result<String> {
    let input = input.trim();
    let code = if input.starts_with("https://") {
        let url = tauri::Url::parse(input).map_err(|_| {
            LauncherError::InvalidArguments("A URL de retorno do GOG é inválida".into())
        })?;
        if url.scheme() != "https"
            || url.host_str() != Some("embed.gog.com")
            || url.path() != "/on_login_success"
        {
            return Err(LauncherError::InvalidArguments(
                "Cole somente a URL final iniciada por https://embed.gog.com/on_login_success"
                    .into(),
            ));
        }
        url.query_pairs()
            .find(|(name, _)| name == "code")
            .map(|(_, value)| value.into_owned())
            .ok_or_else(|| {
                LauncherError::InvalidArguments(
                    "A URL de retorno do GOG não contém o código de autorização".into(),
                )
            })?
    } else {
        input.to_owned()
    };
    if !(8..=2048).contains(&code.len())
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
    {
        return Err(LauncherError::InvalidArguments(
            "O código de autorização do GOG é inválido".into(),
        ));
    }
    Ok(code)
}

fn valid_gog_auth_file(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > 64 * 1024
    {
        return false;
    }
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    // GOGDL 1.1 stores credentials under the OAuth client id, while older
    // releases and Heroic installations may keep the fields at the root.
    valid_gog_auth_value(&value) || value.get(GOG_CLIENT_ID).is_some_and(valid_gog_auth_value)
}

fn valid_gog_auth_value(value: &serde_json::Value) -> bool {
    ["access_token", "refresh_token", "user_id"]
        .into_iter()
        .all(|field| {
            value
                .get(field)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| !value.is_empty() && value.len() <= 8192)
        })
}

fn valid_windows_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() < 64 * 1024
        || metadata.len() > 512 * 1024 * 1024
    {
        return false;
    }
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut signature = [0_u8; 2];
    file.read_exact(&mut signature).is_ok() && signature == *b"MZ"
}

fn valid_named_steam_account(user: &str) -> Result<String> {
    let user = user.trim();
    if user.is_empty()
        || user.eq_ignore_ascii_case("anonymous")
        || user.len() > 64
        || user.starts_with('+')
        || user.chars().any(char::is_whitespace)
        || user.chars().any(char::is_control)
    {
        return Err(LauncherError::InvalidArguments(
            "Informe um nome de usuário válido da Steam; login anônimo não conecta uma conta"
                .into(),
        ));
    }
    Ok(user.to_string())
}

/// Extracts the last named Steam account whose login was positively confirmed.
///
/// New transcripts are confirmed by the `info` pair documented by Valve:
/// `Account: <name>` and `Logon state: Logged On`. The older
/// `Logging in user ... / OK / Waiting for user info... / OK` sequence
/// is accepted only to migrate authentication performed by earlier Orbit
/// builds. Anonymous sessions are intentionally ignored in both formats.
fn steam_account_from_output(output: &str) -> Option<String> {
    let output = strip_ansi_sequences(output);
    let mut info_account = None;
    let mut legacy_account = None;
    let mut legacy_logged_in = false;
    let mut waiting_for_user_info = false;
    let mut last_confirmation: Option<(usize, String)> = None;

    for (line_number, raw_line) in output.lines().enumerate() {
        let line = raw_line.trim();

        if let Some(account) = value_after_label(line, "Account:") {
            info_account = valid_named_steam_account(account).ok();
            last_confirmation = None;
        }

        if let Some(state) = value_after_label(line, "Logon state:") {
            if state.eq_ignore_ascii_case("Logged On") {
                if let Some(account) = info_account.take() {
                    last_confirmation = Some((line_number, account));
                }
            } else {
                info_account = None;
                last_confirmation = None;
            }
        }

        if let Some(account) = legacy_login_account(line) {
            legacy_account = valid_named_steam_account(account).ok();
            last_confirmation = None;
            // The real SteamCMD transcript reports login success as a plain
            // timestamped `OK` line, not as `Logged in OK`. Confirmation is
            // completed only after the later `Waiting for user info... / OK`.
            legacy_logged_in = legacy_account.is_some();
            waiting_for_user_info = false;
        }
        if legacy_logged_in && line.contains("Waiting for user info...") {
            waiting_for_user_info = true;
        }
        if legacy_logged_in
            && waiting_for_user_info
            && (line.ends_with("...OK") || line_ends_with_ok(line))
        {
            if let Some(account) = legacy_account.take() {
                last_confirmation = Some((line_number, account));
            }
            legacy_logged_in = false;
            waiting_for_user_info = false;
        }
        if line.contains("FAILED") || line.contains("Login Failure") {
            legacy_account = None;
            legacy_logged_in = false;
            waiting_for_user_info = false;
            last_confirmation = None;
        }
    }

    last_confirmation.map(|(_, account)| account)
}

fn value_after_label<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let value = line.split_once(label)?.1.trim();
    (!value.is_empty()).then_some(value)
}

fn legacy_login_account(line: &str) -> Option<&str> {
    let value = line.split_once("Logging in user '")?.1;
    value
        .split_once('\'')
        .map(|(account, _)| account)
        .filter(|account| !account.is_empty())
}

fn line_ends_with_ok(line: &str) -> bool {
    line.rsplit_once(']')
        .map_or_else(|| line.trim() == "OK", |(_, suffix)| suffix.trim() == "OK")
}

fn strip_ansi_sequences(value: &str) -> String {
    let mut clean = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            clean.push(character);
            continue;
        }
        if characters.next_if_eq(&'[').is_some() {
            for next in characters.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        }
    }
    clean
}

#[derive(Debug, PartialEq)]
enum VdfToken {
    Text(Vec<u8>),
    Open,
    Close,
}

/// Checks for at least one key/value entry inside a VDF section. Token bytes
/// remain local to this function and are never converted to displayable text,
/// which keeps Steam's cached authentication material out of logs and errors.
fn vdf_section_has_entry(contents: &[u8], section: &str) -> bool {
    let tokens = vdf_tokens(contents);
    let section = section.as_bytes();
    let mut index = 0;
    while index + 1 < tokens.len() {
        if matches!(&tokens[index], VdfToken::Text(value) if value.eq_ignore_ascii_case(section))
            && tokens[index + 1] == VdfToken::Open
        {
            let mut depth = 1;
            let mut direct_text_tokens = 0;
            for token in &tokens[index + 2..] {
                match token {
                    VdfToken::Open => depth += 1,
                    VdfToken::Close => {
                        depth -= 1;
                        if depth == 0 {
                            return direct_text_tokens >= 2;
                        }
                    }
                    VdfToken::Text(_) if depth == 1 => direct_text_tokens += 1,
                    VdfToken::Text(_) => {}
                }
            }
            return false;
        }
        index += 1;
    }
    false
}

fn vdf_tokens(contents: &[u8]) -> Vec<VdfToken> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < contents.len() {
        match contents[index] {
            b'{' => {
                tokens.push(VdfToken::Open);
                index += 1;
            }
            b'}' => {
                tokens.push(VdfToken::Close);
                index += 1;
            }
            b'"' => {
                index += 1;
                let mut value = Vec::new();
                while index < contents.len() {
                    match contents[index] {
                        b'\\' if index + 1 < contents.len() => {
                            value.push(contents[index + 1]);
                            index += 2;
                        }
                        b'"' => {
                            index += 1;
                            break;
                        }
                        character => {
                            value.push(character);
                            index += 1;
                        }
                    }
                }
                tokens.push(VdfToken::Text(value));
            }
            b'/' if contents.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < contents.len() && contents[index] != b'\n' {
                    index += 1;
                }
            }
            character if character.is_ascii_whitespace() => index += 1,
            _ => {
                let start = index;
                while index < contents.len()
                    && !contents[index].is_ascii_whitespace()
                    && !matches!(contents[index], b'{' | b'}')
                {
                    index += 1;
                }
                tokens.push(VdfToken::Text(contents[start..index].to_vec()));
            }
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_with_steamcmd(root: &Path) -> ProviderManager {
        let executable = root.join("providers/steamcmd/current/bin/steamcmd");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"#!/bin/sh\n").unwrap();
        ProviderManager::new(root.to_path_buf())
    }

    fn provider_with_gogdl(root: &Path) -> ProviderManager {
        let executable = root.join("providers/gogdl/current/bin/gogdl");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"#!/bin/sh\n").unwrap();
        ProviderManager::new(root.to_path_buf())
    }

    fn provider_with_battlenet_dependencies(root: &Path) -> ProviderManager {
        for (dependency, executable, contents) in [
            ("wine-ge", "wine", b"#!/bin/sh\n".as_slice()),
            ("battlenet-client", "Battle.net-Setup.exe", b"MZ".as_slice()),
        ] {
            let path = root
                .join("providers")
                .join(dependency)
                .join("current/bin")
                .join(executable);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        ProviderManager::new(root.to_path_buf())
    }

    #[test]
    fn refuses_unsigned_external_override() {
        let root = tempfile::tempdir().unwrap();
        let manifests = root.path().join("manifests");
        fs::create_dir_all(&manifests).unwrap();
        fs::write(manifests.join("legendary.json"), b"{}").unwrap();
        let manager = DependencyManager::new(root.path().into());
        assert!(manager.install_with_progress("legendary", |_| {}).is_err());
        assert!(!root.path().join("providers/legendary/current").exists());
    }

    #[test]
    fn has_builtin_recipes_for_managed_store_tools() {
        for id in ["steamcmd", "legendary", "gogdl", "battlenet-client"] {
            let manifest = builtin_manifest(id).unwrap();
            assert_eq!(manifest.id, id);
            assert_eq!(manifest.sha256.len(), 64);
            assert!(manifest.url.starts_with("https://"));
        }
    }

    #[test]
    fn installs_and_marks_direct_artifact_executable() {
        let source = tempfile::NamedTempFile::new().unwrap();
        fs::write(source.path(), b"#!/bin/sh\nexit 0\n").unwrap();
        let root = tempfile::tempdir().unwrap();
        let manager = DependencyManager::new(root.path().into());
        let manifest = SignedManifest {
            id: "legendary".into(),
            version: "test".into(),
            url: format!("file://{}", source.path().display()),
            sha256: "306c6ca7407560340797866e077e053627ad409277d1b9da58106fce4cf717cb".into(),
            executable: "bin/legendary".into(),
            archive: None,
            size: Some(18),
        };
        let mut stages = Vec::new();
        manager
            .download_and_install_with_progress(&manifest, &mut |event| stages.push(event.stage))
            .unwrap();
        assert!(stages.iter().any(|stage| stage == "downloading"));
        assert!(stages.iter().any(|stage| stage == "verifying"));
        assert!(stages.iter().any(|stage| stage == "installing"));
        assert_eq!(stages.last().map(String::as_str), Some("completed"));
        let executable = manager.executable("legendary").unwrap();
        assert_eq!(
            fs::metadata(executable).unwrap().permissions().mode() & 0o111,
            0o111
        );
    }

    #[test]
    fn creates_steamcmd_wrapper_next_to_extracted_runtime() {
        let staging = tempfile::tempdir().unwrap();
        fs::write(staging.path().join("steamcmd.sh"), b"#!/bin/sh\n").unwrap();
        prepare_entrypoint("steamcmd", staging.path()).unwrap();
        let wrapper = staging.path().join("bin/steamcmd");
        assert!(wrapper.is_file());
        assert_ne!(
            fs::metadata(wrapper).unwrap().permissions().mode() & 0o111,
            0
        );
    }

    #[test]
    fn parses_real_steamcmd_download_progress_in_bytes() {
        assert_eq!(
            parse_transfer_progress(
                "steam",
                "Update state (0x61) downloading, progress: 29.07 (3299539690 / 11348895985)",
            ),
            Some(TransferProgress {
                downloaded_bytes: 3_299_539_690,
                total_bytes: 11_348_895_985,
            })
        );
    }

    #[test]
    fn parses_timestamped_and_colored_steamcmd_progress() {
        assert_eq!(
            parse_transfer_progress(
                "STEAMCMD",
                "\x1b[32m[2026-08-13 15:16:00] Update state (0x61) DOWNLOADING, PROGRESS: 4.04 (458412985 / 11348895985)\x1b[0m\r",
            ),
            Some(TransferProgress {
                downloaded_bytes: 458_412_985,
                total_bytes: 11_348_895_985,
            })
        );
    }

    #[test]
    fn ignores_non_download_and_malformed_provider_progress() {
        for line in [
            "Update state (0x81) verifying update, progress: 29.07 (3299539690 / 11348895985)",
            "Update state (0x61) downloading, progress: 29.07 (3299539690 / 0)",
            "Update state (0x61) downloading, progress: 29.07 (11348895986 / 11348895985)",
            "Update state (0x61) downloading, progress: 29.07 (not-a-number / 11348895985)",
            "Update state (0x61) downloading, progress: 29.07",
        ] {
            assert_eq!(parse_transfer_progress("steam", line), None);
        }

        assert_eq!(
            parse_transfer_progress(
                "epic",
                "Update state (0x61) downloading, progress: 29.07 (3299539690 / 11348895985)",
            ),
            None
        );
    }

    #[test]
    fn reads_installed_version_without_starting_provider() {
        let root = tempfile::tempdir().unwrap();
        let current = root.path().join("providers/steamcmd/current/bin");
        fs::create_dir_all(&current).unwrap();
        fs::write(
            current.join("steamcmd"),
            b"#!/bin/sh\ntouch should-not-run\n",
        )
        .unwrap();
        fs::write(
            root.path()
                .join("providers/steamcmd/current/.orbit-version"),
            b"test-version\n",
        )
        .unwrap();
        let manager = DependencyManager::new(root.path().into());
        assert_eq!(
            manager.installed_version("steamcmd").as_deref(),
            Some("test-version")
        );
        assert!(!root.path().join("should-not-run").exists());
    }

    #[test]
    fn steam_authentication_requires_a_named_account_and_exits_after_info() {
        let root = tempfile::tempdir().unwrap();
        let provider = provider_with_steamcmd(root.path());

        for invalid in [
            None,
            Some(""),
            Some("   "),
            Some("anonymous"),
            Some("+quit"),
        ] {
            assert!(provider.authenticate_command("steam", invalid).is_err());
        }

        let (_, arguments) = provider
            .authenticate_command("steam", Some("orbit_user"))
            .unwrap();
        assert_eq!(arguments, ["+login", "orbit_user", "+info", "+quit"]);
    }

    #[test]
    fn gog_authentication_requires_a_trusted_callback_code() {
        let root = tempfile::tempdir().unwrap();
        let provider = provider_with_gogdl(root.path());
        for invalid in [
            None,
            Some("short"),
            Some("https://evil.example/on_login_success?code=trusted-code"),
            Some("https://embed.gog.com/on_login_success?error=cancelled"),
        ] {
            assert!(provider.authenticate_command("gog", invalid).is_err());
        }

        let (_, arguments) = provider
            .authenticate_command(
                "gog",
                Some("https://embed.gog.com/on_login_success?origin=client&code=trusted-code_123"),
            )
            .unwrap();
        assert_eq!(arguments[0], "--auth-config-path");
        assert!(arguments[1].ends_with("providers/gog/auth.json"));
        assert_eq!(arguments[2..], ["auth", "--code", "trusted-code_123"]);
    }

    #[test]
    fn gog_connection_requires_a_valid_bounded_credentials_file() {
        let root = tempfile::tempdir().unwrap();
        let provider = ProviderManager::new(root.path().to_path_buf());
        let path = provider.gog_auth_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"null").unwrap();
        assert!(!provider.gog_authenticated());

        fs::write(
            &path,
            br#"{"access_token":"access","refresh_token":"refresh","user_id":"user"}"#,
        )
        .unwrap();
        assert!(provider.gog_authenticated());

        fs::write(
            &path,
            br#"{"46899977096215655":{"access_token":"access","refresh_token":"refresh","user_id":"user","expires_in":3600}}"#,
        )
        .unwrap();
        assert!(provider.gog_authenticated());

        fs::write(
            &path,
            br#"{"untrusted-client":{"access_token":"access","refresh_token":"refresh","user_id":"user"}}"#,
        )
        .unwrap();
        assert!(!provider.gog_authenticated());
    }

    #[test]
    fn battlenet_authentication_uses_managed_dependencies() {
        let root = tempfile::tempdir().unwrap();
        let provider = provider_with_battlenet_dependencies(root.path());

        let (command, arguments) = provider.authenticate_command("battlenet", None).unwrap();

        assert!(command.ends_with("providers/wine-ge/current/bin/wine"));
        assert_eq!(arguments.len(), 1);
        assert!(
            arguments[0].ends_with("providers/battlenet-client/current/bin/Battle.net-Setup.exe")
        );
    }

    #[test]
    fn battlenet_connection_requires_a_real_launcher() {
        let root = tempfile::tempdir().unwrap();
        let provider = ProviderManager::new(root.path().to_path_buf());
        let launcher = provider
            .battlenet_prefix_path()
            .join("drive_c/Program Files (x86)/Battle.net/Battle.net Launcher.exe");
        fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        fs::write(&launcher, b"not a PE executable").unwrap();
        assert!(!provider.battlenet_installed());

        let mut executable = vec![0_u8; 64 * 1024];
        executable[..2].copy_from_slice(b"MZ");
        fs::write(&launcher, executable).unwrap();
        assert_eq!(
            provider.battlenet_launcher_path().as_deref(),
            Some(launcher.as_path())
        );
    }

    #[test]
    fn parses_named_logged_on_account_from_info_output() {
        let output = "\
Steam>info\n\
Account: orbit_user\n\
SteamID: [U:1:123]\n\
Logon state: Logged On\n\
Language: brazilian\n";
        assert_eq!(
            steam_account_from_output(output).as_deref(),
            Some("orbit_user")
        );
    }

    #[test]
    fn rejects_anonymous_incomplete_and_logged_off_sessions() {
        assert_eq!(
            steam_account_from_output("Account: anonymous\nLogon state: Logged On\n"),
            None
        );
        assert_eq!(
            steam_account_from_output("Account: orbit_user\nLogon state: Logged Off\n"),
            None
        );
        assert_eq!(
            steam_account_from_output(
                "Account: old_user\nLogon state: Logged On\n\
                 Account: new_user\nLogon state: Logged Off\n",
            ),
            None
        );
        assert_eq!(steam_account_from_output("Logged in OK\n"), None);
    }

    #[test]
    fn parses_legacy_success_for_existing_login_migration() {
        let output = "\
Logging in user 'legacy_user' [U:1:329000000] to Steam Public...\n\
[2026-08-13 14:51:15] OK\n\
[2026-08-13 14:51:15] Waiting for user info...\n\
[2026-08-13 14:51:16] OK\n";
        assert_eq!(
            steam_account_from_output(output).as_deref(),
            Some("legacy_user")
        );
    }

    #[test]
    fn reads_only_new_authentication_log_segment() {
        let root = tempfile::tempdir().unwrap();
        let provider = ProviderManager::new(root.path().to_path_buf());
        let log = provider.steam_log_path();
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        let old = "Account: old_user\nLogon state: Logged On\n";
        fs::write(&log, old).unwrap();
        let offset = provider.steam_log_len();
        fs::OpenOptions::new()
            .append(true)
            .open(&log)
            .unwrap()
            .write_all(b"Account: new_user\nLogon state: Logged Off\n")
            .unwrap();

        assert_eq!(provider.steam_account_from_log_since(offset), None);
        assert_eq!(provider.steam_account_from_log(), None);
    }

    #[test]
    fn reads_legacy_authentication_log_location() {
        let root = tempfile::tempdir().unwrap();
        let provider = ProviderManager::new(root.path().to_path_buf());
        let legacy = root
            .path()
            .join("providers/steamcmd/current/authentication.log");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(
            legacy,
            b"Logging in user 'migrated_user' to Steam Public...\nOK\nWaiting for user info...OK\n",
        )
        .unwrap();

        assert_eq!(
            provider.steam_account_from_log().as_deref(),
            Some("migrated_user")
        );
    }

    #[test]
    fn detects_only_nonempty_connect_cache_sections() {
        assert!(vdf_section_has_entry(
            br#""InstallConfigStore"
            {
                "Software" { "Valve" { "Steam" {
                    "ConnectCache" { "fixture_user" "fake-token-for-test" }
                } } }
            }"#,
            "ConnectCache"
        ));
        assert!(!vdf_section_has_entry(
            br#""InstallConfigStore" { "ConnectCache" { } }"#,
            "ConnectCache"
        ));
        assert!(!vdf_section_has_entry(
            br#""Unrelated" { "fixture_user" "fake-token-for-test" }"#,
            "ConnectCache"
        ));
    }
}
