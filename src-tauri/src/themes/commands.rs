use std::path::PathBuf;

use tauri::State;

use crate::{
    commands::AppState,
    error::{LauncherError, Result},
    themes::{ThemeDetails, ThemeManager, ThemeSummary},
};

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
    settings.last_manual_theme_id = theme.summary.id.clone();
    settings.theme_mode = "manual".into();
    // Mantém consumidores antigos da preferência `theme` funcionais durante a migração.
    settings.theme = match theme.summary.theme_type {
        super::manifest::ThemeType::Light => "system",
        super::manifest::ThemeType::Dark => "dark",
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
    let mut changed = false;
    if settings.active_theme_id == id {
        settings.active_theme_id = "orbit-dark".into();
        settings.theme = "dark".into();
        changed = true;
    }
    if settings.last_manual_theme_id == id {
        settings.last_manual_theme_id = "orbit-dark".into();
        changed = true;
    }
    if changed {
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
pub async fn detect_color_scheme_provider() -> Result<super::automatic::ProviderStatus> {
    tauri::async_runtime::spawn_blocking(super::automatic::detect_provider)
        .await
        .map_err(|error| LauncherError::InvalidTheme(error.to_string()))
}

#[tauri::command]
pub async fn get_pywal_status() -> Result<super::automatic::ProviderStatus> {
    detect_color_scheme_provider().await
}

#[tauri::command]
pub async fn get_current_wallpaper() -> Result<String> {
    tauri::async_runtime::spawn_blocking(super::automatic::current_wallpaper)
        .await
        .map_err(|error| LauncherError::InvalidTheme(error.to_string()))?
}

#[tauri::command]
pub async fn generate_automatic_palette(
    state: State<'_, AppState>,
) -> Result<super::automatic::AutomaticTheme> {
    let settings = state
        .database
        .lock()
        .expect("database lock poisoned")
        .settings()?;
    tauri::async_runtime::spawn_blocking(move || {
        super::automatic::generate(
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
) -> Result<super::automatic::AutomaticTheme> {
    generate_automatic_palette(state).await
}

#[tauri::command]
pub async fn refresh_automatic_theme(
    state: State<'_, AppState>,
) -> Result<super::automatic::AutomaticTheme> {
    generate_automatic_palette(state).await
}
