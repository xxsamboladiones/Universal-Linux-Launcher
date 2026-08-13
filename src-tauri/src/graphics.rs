use std::{fs, path::Path};

#[derive(Debug, Default)]
pub struct GraphicsWorkarounds {
    pub nvidia_detected: bool,
    pub dmabuf_disabled: bool,
    pub software_compositing: bool,
}

/// Configura o WebKitGTK antes que GTK, WebKit ou a primeira janela existam.
/// Variáveis definidas depois de `tauri::Builder::default()` chegam tarde
/// demais porque o processo gráfico do WebKit já pode ter sido inicializado.
pub fn configure_before_webview() -> GraphicsWorkarounds {
    #[cfg(target_os = "linux")]
    {
        configure_linux()
    }
    #[cfg(not(target_os = "linux"))]
    {
        GraphicsWorkarounds::default()
    }
}

#[cfg(target_os = "linux")]
fn configure_linux() -> GraphicsWorkarounds {
    let nvidia_detected = nvidia_detected_at(
        Path::new("/proc/driver/nvidia/version"),
        Path::new("/sys/module/nvidia"),
        Path::new("/sys/class/drm"),
    );
    let allow_dmabuf = env_truthy("ORBIT_ENABLE_DMABUF_RENDERER");
    let force_software = env_truthy("ORBIT_WEBKIT_SOFTWARE");

    // WebKitGTK pode criar o DOM normalmente, mas falhar ao importar o
    // framebuffer DMA-BUF fornecido pelo driver NVIDIA. O resultado é uma
    // janela branca/invisível. Respeitamos tanto uma configuração explícita
    // do usuário quanto a opção de reativar o caminho rápido para diagnóstico.
    if nvidia_detected
        && !allow_dmabuf
        && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    // Último recurso documentado: desliga a composição acelerada inteira.
    // Não é aplicado por padrão porque o fallback DMA-BUF preserva mais do
    // pipeline gráfico e é suficiente para o caso NVIDIA conhecido.
    if force_software && std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_none() {
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }

    GraphicsWorkarounds {
        nvidia_detected,
        dmabuf_disabled: std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some(),
        software_compositing: std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_some(),
    }
}

#[cfg(target_os = "linux")]
fn env_truthy(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[cfg(target_os = "linux")]
fn nvidia_detected_at(proc_version: &Path, module: &Path, drm: &Path) -> bool {
    if proc_version.is_file() || module.exists() {
        return true;
    }
    let Ok(entries) = fs::read_dir(drm) else {
        return false;
    };
    entries.filter_map(std::result::Result::ok).any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("card") || name.starts_with("renderD")) || name.contains('-') {
            return false;
        }
        fs::read_to_string(entry.path().join("device/vendor"))
            .is_ok_and(|vendor| vendor.trim().eq_ignore_ascii_case("0x10de"))
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn detects_nvidia_by_drm_vendor_without_external_commands() {
        let root = tempfile::tempdir().unwrap();
        let drm = root.path().join("drm");
        let device = drm.join("card1/device");
        fs::create_dir_all(&device).unwrap();
        fs::write(device.join("vendor"), "0x10de\n").unwrap();

        assert!(nvidia_detected_at(
            &root.path().join("missing-proc"),
            &root.path().join("missing-module"),
            &drm,
        ));
    }

    #[test]
    fn does_not_classify_other_drm_vendors_as_nvidia() {
        let root = tempfile::tempdir().unwrap();
        let drm = root.path().join("drm");
        let device = drm.join("card0/device");
        fs::create_dir_all(&device).unwrap();
        fs::write(device.join("vendor"), "0x1002\n").unwrap();

        assert!(!nvidia_detected_at(
            &root.path().join("missing-proc"),
            &root.path().join("missing-module"),
            &drm,
        ));
    }
}
