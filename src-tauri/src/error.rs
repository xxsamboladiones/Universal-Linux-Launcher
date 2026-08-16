use serde::Serialize;
#[derive(Debug, thiserror::Error)]
pub enum LauncherError {
    #[error("Executável não encontrado: {0}")]
    ExecutableNotFound(String),
    #[error("Permissão negada: {0}")]
    PermissionDenied(String),
    #[error("Argumentos inválidos: {0}")]
    InvalidArguments(String),
    #[error("Provider indisponível: {0}")]
    ProviderUnavailable(String),
    #[error("Desktop Entry inválida: {0}")]
    InvalidDesktopEntry(String),
    #[error("Erro no banco: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Falha ao iniciar: {0}")]
    LaunchFailed(String),
    #[error("Erro de I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("Item não encontrado: {0}")]
    NotFound(String),
    #[error("Tema inválido: {0}")]
    InvalidTheme(String),
    #[error("Falha ao processar arquivo compactado: {0}")]
    Archive(String),
}
impl Serialize for LauncherError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
pub type Result<T> = std::result::Result<T, LauncherError>;
