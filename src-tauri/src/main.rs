#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![recursion_limit = "256"]

use std::sync::Arc;

use hellas_gate::commands;
use hellas_gate::state::AppState;
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

    let result = tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            create_private_directory(&data_dir)?;
            let state = Arc::new(tauri::async_runtime::block_on(AppState::open(&data_dir))?);
            app.manage(state.clone());

            install_tray(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::save_listen_port,
            commands::retry_open,
            commands::restart_gate,
            commands::initialize_software,
            commands::preview_apply,
            commands::apply_document,
            commands::set_quota,
            commands::get_quota,
            commands::discard_apply,
            commands::issue_admin_access,
            commands::rotate_gateway_key,
            commands::recover_software,
            commands::bind_provider,
            commands::provider_view,
            commands::configure_provider,
            commands::configure_resource,
            commands::set_provider,
            commands::set_gateway,
            commands::gateway_address,
            commands::list_gateway_keys,
            commands::issue_gateway_key,
            commands::reconcile_gateway_key,
            commands::revoke_gateway_key,
            commands::save_paid_pool,
            commands::paid_client_peer,
            commands::configure_paid_serve,
            commands::provision_paid,
            commands::run_request,
            commands::cancel_run,
            commands::list_contacts,
            commands::add_contact,
            commands::list_shares,
            commands::share_access,
            commands::export_access,
            commands::import_access,
            commands::list_access,
            commands::revoke_share,
            commands::pause_allowance,
            commands::list_trust,
            commands::approve_trust,
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
        .run(tauri::generate_context!());
    if let Err(error) = result {
        tracing::error!(%error, "Gate failed");
        std::process::exit(1);
    }
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
            "quit" => {
                let app = app.clone();
                let state = app.state::<Arc<AppState>>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    match state.shutdown().await {
                        Ok(false) => {}
                        Ok(true) => app.exit(0),
                        Err(error) => {
                            tracing::error!(%error, "Gate shutdown failed");
                            app.exit(1);
                        }
                    }
                });
            }
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
