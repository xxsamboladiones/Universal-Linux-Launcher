use super::generate;
use crate::commands::AppState;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

/// Mantém o watcher vivo e limita tempestades de eventos gravados pelo Pywal.
pub struct PywalWatcher {
    _watcher: RecommendedWatcher,
}

impl PywalWatcher {
    pub fn install(app: AppHandle) -> Result<Self, String> {
        let Some(home) = dirs::home_dir() else {
            return Err("diretório pessoal indisponível".into());
        };
        let cache = home.join(".cache");
        if !cache.is_dir() {
            return Err("cache do usuário indisponível".into());
        }
        let last = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(10)));
        let callback_app = app.clone();
        let mut watcher = RecommendedWatcher::new(
            move |event: notify::Result<Event>| {
                let Ok(event) = event else { return };
                if !event.paths.iter().any(is_pywal_path) {
                    return;
                }
                let Ok(mut last_event) = last.lock() else {
                    return;
                };
                if last_event.elapsed() < Duration::from_millis(650) {
                    return;
                }
                *last_event = Instant::now();
                let app = callback_app.clone();
                tauri::async_runtime::spawn_blocking(move || refresh(app));
            },
            Config::default(),
        )
        .map_err(|error| error.to_string())?;
        watcher
            .watch(&cache, RecursiveMode::Recursive)
            .map_err(|error| error.to_string())?;
        Ok(Self { _watcher: watcher })
    }
}

fn is_pywal_path(path: &PathBuf) -> bool {
    path.components().any(|part| part.as_os_str() == "wal")
}
fn refresh(app: AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(settings) = state
        .database
        .lock()
        .map_err(|_| ())
        .and_then(|db| db.settings().map_err(|_| ()))
    else {
        return;
    };
    if !settings.automatic_update
        || settings.theme_mode != "automatic"
        || settings.palette_source == "native"
    {
        return;
    }
    let Ok(theme) = generate(
        settings.wallpaper_influence,
        &settings.automatic_color_mode,
        &settings.palette_source,
        settings.manual_wallpaper_path,
    ) else {
        return;
    };
    let _ = app.emit("automatic-theme-updated", theme);
}
