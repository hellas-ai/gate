#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use hellas_gate::state::AppState;
use hellas_gate::{commands, host_control};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hellas_gate=info".into()),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            create_private_directory(&data_dir)?;
            let state = Arc::new(AppState::open(&data_dir)?);
            app.manage(state.clone());

            #[cfg(any(unix, windows))]
            tauri::async_runtime::spawn(async move {
                if let Err(error) = host_control::serve(state).await {
                    tracing::error!(%error, "local-control listener stopped");
                }
            });

            install_tray(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::set_provider_enabled,
            commands::set_gateway_enabled,
            commands::get_gateway_access,
            commands::run_request,
            commands::list_history,
            commands::delete_history,
            commands::clear_history,
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    tracing::warn!(%error, "failed to hide Gate window");
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run Hellas Gate");
}

fn install_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Gate", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("Hellas Gate")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Mode 0700 on Unix; on Windows an owner-only DACL that everything Gate
/// writes inside (identities, history, the gateway credential) inherits.
fn create_private_directory(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    hellas_private::restrict_directory(path)
}
