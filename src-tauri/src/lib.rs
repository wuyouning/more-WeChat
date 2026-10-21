mod engine;
pub mod model;
pub mod platform;
mod service;
mod storage;

use model::{AppError, LaunchResult, Snapshot};
use service::Service;
use std::{path::PathBuf, sync::Arc};
use tauri::{Emitter, Manager, State};

type Shared = Arc<Service>;
fn worker_error(error: impl std::fmt::Display) -> AppError {
    AppError::new("worker_failed", format!("操作未完成：{error}"))
}

#[tauri::command]
async fn get_snapshot(state: State<'_, Shared>) -> Result<Snapshot, AppError> {
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.snapshot())
        .await
        .map_err(worker_error)?
}

#[tauri::command]
async fn select_installation(state: State<'_, Shared>, path: String) -> Result<Snapshot, AppError> {
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.select_installation(path))
        .await
        .map_err(worker_error)?
}

#[tauri::command]
async fn save_target(state: State<'_, Shared>, target: u8) -> Result<(), AppError> {
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.save_target(target))
        .await
        .map_err(worker_error)?
}

#[tauri::command]
async fn launch(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    target: u8,
) -> Result<LaunchResult, AppError> {
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        service.launch(target, &|progress| {
            let _ = app.emit("launch-progress", progress);
        })
    })
    .await
    .map_err(worker_error)?
}

#[tauri::command]
async fn export_diagnostics(state: State<'_, Shared>, path: String) -> Result<String, AppError> {
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.export(&PathBuf::from(path)))
        .await
        .map_err(worker_error)?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let service = Arc::new(Service::new(app.path().app_data_dir()?));
            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            select_installation,
            save_target,
            launch,
            export_diagnostics
        ])
        .run(tauri::generate_context!())
        .expect("more-WeChat 启动失败");
}
