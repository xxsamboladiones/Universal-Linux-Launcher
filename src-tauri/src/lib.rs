mod application;
mod commands;
mod core;
mod database;
mod error;
mod graphics;
mod platform;
mod process;
mod product;
mod providers;
mod themes;
use commands::AppState;
use tauri::Manager;
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Precisa ser literalmente a primeira inicialização: o processo gráfico
    // do WebKit herda essas opções quando a primeira webview é criada.
    let graphics = graphics::configure_before_webview();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    if graphics.nvidia_detected || graphics.software_compositing {
        tracing::info!(
            nvidia = graphics.nvidia_detected,
            dmabuf_disabled = graphics.dmabuf_disabled,
            software_compositing = graphics.software_compositing,
            "mitigação gráfica do WebKitGTK configurada"
        );
    }
    // O desktop-id precisa existir antes da criação da superfície X11/Wayland;
    // criá-lo em setup() é tarde demais para o Plasma associar a primeira janela.
    if let Err(error) = product::ensure_desktop_integration() {
        tracing::warn!(%error, "não foi possível registrar a integração desktop");
    }
    let Some(instance) =
        product::InstanceGuard::acquire().expect("failed to acquire single instance socket")
    else {
        return;
    };
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let db = database::Database::open(&data.join("orbit.db"))?;
            db.recover_operations()?;
            app.manage(AppState {
                database: std::sync::Mutex::new(db),
                process_manager: process::ProcessManager::new(data.join("orbit.db")),
                transfer_manager: commands::TransferManager::default(),
                library_sync_manager: commands::LibrarySyncManager::default(),
                data_dir: data,
            });
            if let (Some(window), Some(icon)) =
                (app.get_webview_window("main"), app.default_window_icon())
            {
                window.set_icon(icon.clone())?;
                #[cfg(target_os = "linux")]
                {
                    use gtk::prelude::GtkWindowExt;
                    // No Wayland o Plasma resolve o ícone pelo nome/app_id. O
                    // set_icon acima atende X11; set_icon_name cobre Wayland.
                    let gtk_window = window.gtk_window()?;
                    gtk_window.set_icon_name(Some("orbit-launcher"));
                    gtk_window.set_role("orbit-launcher");
                    gtk::Window::set_default_icon_name("orbit-launcher");
                }
            }
            instance.listen(app.handle().clone());
            app.manage(instance);
            let menu = tauri::menu::MenuBuilder::new(app)
                .text("show", "Abrir Orbit")
                .text("quit", "Sair")
                .build()?;
            tauri::tray::TrayIconBuilder::with_id("orbit-tray")
                .menu(&menu)
                .icon(app.default_window_icon().cloned().expect("bundle icon"))
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => product::show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            if std::env::args().any(|argument| argument == "--hidden") {
                if let Some(window) = app.get_webview_window("main") {
                    window.hide()?;
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_library,
            commands::scan_providers,
            commands::launch_item,
            commands::get_running_items,
            commands::set_favorite,
            commands::set_hidden,
            commands::update_item,
            commands::delete_item,
            commands::uninstall_item,
            commands::get_settings,
            commands::update_settings,
            commands::list_argument_presets,
            commands::save_argument_preset,
            commands::delete_argument_preset,
            commands::get_argument_preset,
            commands::get_platform_overview,
            commands::prepare_provider,
            commands::get_compatibility_overview,
            commands::create_game_prefix,
            commands::open_path,
            commands::open_compatibility_log,
            commands::rollback_dependency,
            commands::connect_provider,
            commands::store_provider_token,
            commands::queue_store_operation,
            commands::retry_operation,
            commands::remove_store_operation,
            commands::cancel_store_operation,
            commands::sync_store_library,
            commands::get_product_status,
            commands::set_autostart,
            commands::export_backup,
            commands::import_backup,
            commands::check_for_updates,
            commands::install_update,
            commands::list_themes,
            commands::get_theme,
            commands::get_active_theme,
            commands::set_active_theme,
            commands::import_theme,
            commands::remove_theme,
            commands::export_theme,
            commands::validate_theme,
            commands::detect_color_scheme_provider,
            commands::get_current_wallpaper,
            commands::generate_automatic_palette,
            commands::get_automatic_theme,
            commands::refresh_automatic_theme,
            commands::get_pywal_status
        ])
        .build(tauri::generate_context!())
        .expect("error while building Orbit Launcher");
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            if let Some(state) = app.try_state::<AppState>() {
                state.transfer_manager.cancel_all();
                state.library_sync_manager.cancel_all();
            }
        }
    });
}
