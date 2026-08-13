use super::{find_on_path, Operation};
use crate::error::{LauncherError, Result};
use serde::Deserialize;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

#[derive(Debug, Deserialize)]
struct SignedManifest {
    id: String,
    version: String,
    url: String,
    sha256: String,
    executable: String,
    archive: Option<String>,
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
            .or_else(|| find_on_path(id))
    }
    pub fn install(&self, id: &str) -> Result<()> {
        let dir = self.root.join("manifests");
        let manifest_path = dir.join(format!("{id}.json"));
        let signature = dir.join(format!("{id}.json.sig"));
        let public_key = dir.join("orbit-dependencies.pem");
        for path in [&manifest_path, &signature, &public_key] {
            if !path.is_file() {
                return Err(LauncherError::ProviderUnavailable(format!(
                    "Arquivo de confiança ausente: {}",
                    path.display()
                )));
            }
        }
        let verified = Command::new("openssl")
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
        let downloads = self.root.join("downloads");
        fs::create_dir_all(&downloads)?;
        let partial = downloads.join(format!("{id}-{}.part", manifest.version));
        let status = Command::new("curl")
            .args(["--fail", "--location", "--continue-at", "-", "--output"])
            .arg(&partial)
            .arg(&manifest.url)
            .status()?;
        if !status.success() {
            return Err(LauncherError::ProviderUnavailable(
                "Download interrompido; será retomado na próxima tentativa".into(),
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
                "SHA-256 não confere; arquivo parcial preservado para diagnóstico".into(),
            ));
        }
        let provider = self.root.join("providers").join(id);
        let staging = provider.join(format!(".staging-{}", manifest.version));
        if staging.exists() {
            fs::remove_dir_all(&staging)?
        }
        fs::create_dir_all(&staging)?;
        match manifest.archive.as_deref() {
            Some("tar.gz") => {
                let listing = Command::new("tar").args(["-tzf"]).arg(&partial).output()?;
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
                if !Command::new("tar")
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
                fs::copy(&partial, target)?;
            }
        }
        if !staging.join(&manifest.executable).is_file() {
            return Err(LauncherError::ProviderUnavailable(
                "Executável declarado não existe no pacote".into(),
            ));
        }
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
    fn refuses_install_without_trust_chain() {
        let root = tempfile::tempdir().unwrap();
        let manager = DependencyManager::new(root.path().into());
        assert!(manager.install("legendary").is_err());
        assert!(!root.path().join("providers/legendary/current").exists());
    }
}
