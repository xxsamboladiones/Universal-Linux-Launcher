use super::{find_on_path, Operation};
use crate::error::{LauncherError, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
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
            "battlenet-client" => &["Battle.net.exe"],
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
        "battlenet-client" => {
            return Err(LauncherError::ProviderUnavailable(
                "O instalador oficial do Battle.net muda sem publicar checksum estável; a instalação automática segura ainda não está disponível".into(),
            ))
        }
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
            "steam" => Ok((
                deps.executable("steamcmd")
                    .ok_or_else(|| LauncherError::ExecutableNotFound("steamcmd".into()))?
                    .to_string_lossy()
                    .into_owned(),
                vec!["+login".into(), user.unwrap_or("anonymous").into()],
            )),
            "gog" => Ok((
                deps.executable("gogdl")
                    .ok_or_else(|| LauncherError::ExecutableNotFound("gogdl".into()))?
                    .to_string_lossy()
                    .into_owned(),
                vec!["auth".into()],
            )),
            "battlenet" => Ok((
                "wine".into(),
                vec![self
                    .root
                    .join("providers/battlenet/Battle.net-Setup.exe")
                    .to_string_lossy()
                    .into_owned()],
            )),
            _ => Err(LauncherError::ProviderUnavailable(provider.into())),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
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
        for id in ["steamcmd", "legendary", "gogdl"] {
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
}
