//! AutoPing Desktop: responde automaticamente a novos tópicos do Discord,
//! sempre DEPOIS da 1ª mensagem do criador. Quem responde primeiro fica com a
//! vaga: latência é a métrica nº 1.
//!
//! ⚠️ Self-bot: viola os Termos de Serviço do Discord. Use por sua conta e risco.

pub mod audio;
pub mod config;
pub mod core;
pub mod discord;
pub mod engine;
pub mod logger;
pub mod secrets;
mod tray;
mod window;

use config::{ConfigPatch, Status};
use discord::cache::ChannelEntry;
use engine::{Engine, Snapshot};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

type EngineState<'a> = State<'a, Arc<Engine>>;

#[tauri::command]
fn get_snapshot(engine: EngineState) -> Snapshot {
    engine.snapshot()
}

#[tauri::command]
async fn update_config(engine: EngineState<'_>, patch: ConfigPatch) -> Result<Snapshot, String> {
    let engine = engine.inner().clone();
    engine.update_config(patch).await
}

#[tauri::command]
async fn set_token(engine: EngineState<'_>, token: String) -> Result<Snapshot, String> {
    engine.inner().clone().set_token(token)
}

#[tauri::command]
async fn set_status(engine: EngineState<'_>, status: Status) -> Result<Snapshot, String> {
    Ok(engine.set_status(status))
}

#[tauri::command]
fn stop_alarm(engine: EngineState) -> Snapshot {
    engine.stop_alarm()
}

#[tauri::command]
async fn test_sound(engine: EngineState<'_>, kind: String) -> Result<(), String> {
    engine.test_sound(&kind);
    Ok(())
}

#[tauri::command]
fn list_channels(engine: EngineState) -> Vec<ChannelEntry> {
    engine.list_channels()
}

#[tauri::command]
fn hide_window(app: AppHandle) {
    window::hide(&app);
}

#[tauri::command]
fn resize_window(app: AppHandle, height: f64) {
    window::resize(&app, height.clamp(120.0, 720.0));
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| window::show(app)))
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            logger::init(dir.join("logs"));

            let engine = Engine::new(dir.join("config.json"));
            app.manage(engine.clone());
            tray::create(app.handle(), engine.clone())?;

            let handle = app.handle().clone();
            engine.set_on_change(move |snap| {
                let _ = handle.emit("state-changed", snap);
                tray::update(&handle, snap);
            });

            let token = engine.initial_token();
            let first_run = token.is_none() || engine.snapshot().channel.id.is_empty();
            let e = engine.clone();
            tauri::async_runtime::spawn(async move { e.start(token) });

            // 1º uso (sem token ou canal): abre a janela na config.
            // Configurado: inicia só na bandeja (sem WebView = RAM mínima).
            tray::update(app.handle(), &engine.snapshot());
            if first_run {
                window::show(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            update_config,
            set_token,
            set_status,
            list_channels,
            stop_alarm,
            test_sound,
            hide_window,
            resize_window
        ])
        .build(tauri::generate_context!())
        .expect("erro ao iniciar o AutoPing");

    app.run(|_app, event| {
        // Fechar/destruir a janela não encerra o app (fica na bandeja).
        if let RunEvent::ExitRequested { code: None, api, .. } = event {
            api.prevent_exit();
        }
    });
}
