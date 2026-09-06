pub mod ai;
pub mod comparer;
pub mod database;
pub mod decoder;
pub mod error;
pub mod event_bus;
pub mod export;
pub mod history;
pub mod http;
pub mod logging;
pub mod project;
pub mod proxy;
pub mod repeater;
pub mod scope;
pub mod settings;
pub mod state;
pub mod tls;
pub mod ui_bridge;
pub mod worker;

#[cfg(test)]
mod regression_tests;

use state::AppState;
use std::sync::atomic::Ordering;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");
    let state = AppState::new();
    {
        use tracing_subscriber::prelude::*;
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                    tracing_subscriber::EnvFilter::new("witness=info,witness_lib=debug")
                }),
            )
            .with(tracing_subscriber::fmt::layer())
            .with(logging::LogLayer::new(state.logs.clone()))
            .try_init()
            .ok();
    }
    tracing::info!(module = "app", "Witness starting");
    let bridge_state = state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state)
        .setup(move |app| {
            let app_data = app
                .path()
                .app_local_data_dir()
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            std::fs::create_dir_all(&app_data)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            let stronghold_salt = app_data.join("stronghold.salt");
            app.handle()
                .plugin(tauri_plugin_stronghold::Builder::with_argon2(&stronghold_salt).build())?;
            let credential_snapshot = app_data.join("ai-credentials.hold");
            bridge_state
                .ai_credentials_paths
                .lock()
                .map_err(|_| anyhow::anyhow!("AI credential path lock poisoned"))?
                .replace((credential_snapshot.clone(), stronghold_salt.clone()));
            let credentials = bridge_state.ai_credentials.clone();
            let credentials_ready = bridge_state.ai_credentials_ready.clone();
            let credentials_generation = bridge_state.ai_credentials_generation.clone();
            let initial_generation = credentials_generation.load(Ordering::Acquire);
            tauri::async_runtime::spawn(async move {
                let result = tauri::async_runtime::spawn_blocking(move || {
                    let mut expected_generation = initial_generation;
                    loop {
                        tracing::info!(module = "credentials", "initializing encrypted AI credential store");
                        let store = match crate::ai::AiCredentialStore::open(
                            &credential_snapshot,
                            &stronghold_salt,
                        ) {
                            Ok(store) => store,
                            Err(error) => {
                                let current_generation =
                                    credentials_generation.load(Ordering::Acquire);
                                if current_generation != expected_generation {
                                    expected_generation = current_generation;
                                    continue;
                                }
                                return Err(error.to_string());
                            }
                        };
                        let mut credentials = credentials
                            .lock()
                            .map_err(|_| "AI credential store lock poisoned".to_string())?;
                        let current_generation = credentials_generation.load(Ordering::Acquire);
                        if current_generation == expected_generation {
                            credentials.replace(store);
                            return Ok(());
                        }
                        drop(credentials);
                        tracing::info!(
                            module = "credentials",
                            "discarding credential store opened before a credential reset"
                        );
                        expected_generation = current_generation;
                    }
                })
                .await
                .map_err(|error| format!("AI credential store task failed: {error}"))
                .and_then(|result| result);
                match &result {
                    Ok(()) => tracing::info!(module = "credentials", "encrypted AI credential store ready"),
                    Err(error) => tracing::error!(module = "credentials", %error, "encrypted AI credential store failed to initialize"),
                }
                // `send` fails to retain a value when no receiver is currently
                // subscribed. Startup normally finishes before the first AI
                // command subscribes, so use `send_replace` to make readiness
                // state durable for later callers as well.
                credentials_ready.send_replace(Some(result));
            });
            ui_bridge::forward_events_to_ui(bridge_state.clone(), app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ui_bridge::get_app_snapshot,
            ui_bridge::update_settings,
            ui_bridge::ai_infer,
            ui_bridge::cancel_ai_infer,
            ui_bridge::test_ai_connection,
            ui_bridge::set_ai_api_key,
            ui_bridge::delete_ai_api_key,
            ui_bridge::get_ai_api_key_status,
            ui_bridge::get_ai_runtime_status,
            ui_bridge::generate_ca_certificate,
            ui_bridge::start_proxy,
            ui_bridge::stop_proxy,
            ui_bridge::get_recent_projects,
            ui_bridge::create_project,
            ui_bridge::create_temporary_project,
            ui_bridge::save_temporary_project,
            ui_bridge::open_project,
            ui_bridge::pick_project_file,
            ui_bridge::pick_project_save_path,
            ui_bridge::close_project,
            ui_bridge::delete_project,
            ui_bridge::query_history,
            ui_bridge::get_history_detail,
            ui_bridge::delete_history_entry,
            ui_bridge::clear_history,
            ui_bridge::create_fuzz_scan,
            ui_bridge::complete_fuzz_scan,
            ui_bridge::get_organizer,
            ui_bridge::create_organizer_folder,
            ui_bridge::update_organizer_folder,
            ui_bridge::delete_organizer_folder,
            ui_bridge::create_organizer_item,
            ui_bridge::update_organizer_item,
            ui_bridge::delete_organizer_item,
            ui_bridge::import_organizer,
            ui_bridge::export_organizer_json,
            ui_bridge::import_organizer_json,
            ui_bridge::get_identity_groups,
            ui_bridge::create_identity_group,
            ui_bridge::update_identity_group,
            ui_bridge::delete_identity_group,
            ui_bridge::create_identity,
            ui_bridge::update_identity,
            ui_bridge::delete_identity,
            ui_bridge::resolve_identity_injection,
            ui_bridge::import_identities,
            ui_bridge::export_identities_json,
            ui_bridge::import_identities_json,
            ui_bridge::resolve_interception,
            ui_bridge::get_scope,
            ui_bridge::add_scope_entry,
            ui_bridge::remove_scope_entry,
            ui_bridge::update_scope_entry,
            ui_bridge::import_scope_entries,
            ui_bridge::decoder_transform,
            ui_bridge::compare_text,
            ui_bridge::save_workspace,
            ui_bridge::get_workspace,
            ui_bridge::save_project,
            ui_bridge::import_request_file,
            ui_bridge::open_in_repeater,
            ui_bridge::send_repeater_request,
            ui_bridge::cancel_repeater_request,
            ui_bridge::export_ca_certificate,
            ui_bridge::get_log_entries,
            ui_bridge::get_traffic_stats,
            ui_bridge::clear_log_entries,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Witness");
}
