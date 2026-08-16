use super::generate;
use crate::commands::AppState;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::Duration,
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
        let wal_dir = cache.join("wal");
        if !cache.is_dir() {
            return Err("cache do usuário indisponível".into());
        }
        let generation = Arc::new(AtomicU64::new(0));
        let callback_app = app.clone();
        let mut watcher = RecommendedWatcher::new(
            move |event: notify::Result<Event>| {
                let Ok(event) = event else { return };
                if !event.paths.iter().any(|path| is_pywal_path(path)) {
                    return;
                }
                let sequence = generation.fetch_add(1, Ordering::AcqRel) + 1;
                let generation = generation.clone();
                let app = callback_app.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(800));
                    if generation.load(Ordering::Acquire) == sequence {
                        refresh(app);
                    }
                });
            },
            Config::default(),
        )
        .map_err(|error| error.to_string())?;
        watcher
            .watch(
                if wal_dir.is_dir() { &wal_dir } else { &cache },
                RecursiveMode::Recursive,
            )
            .map_err(|error| error.to_string())?;
        Ok(Self { _watcher: watcher })
    }
}

fn is_pywal_path(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "colors.json")
        && path.components().any(|part| part.as_os_str() == "wal")
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
