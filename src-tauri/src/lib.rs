pub mod sync;

#[cfg(feature = "desktop")]
use serde::Serialize;
#[cfg(feature = "desktop")]
use sync::{
    execute_sync, open_pool, push_log, start_sync_worker, DbConfig, LogEntry, LogHistory,
    LogLevel, SyncConfig, SyncHandle,
};
#[cfg(feature = "desktop")]
use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(feature = "desktop")]
use tauri::{AppHandle, Manager, State};
#[cfg(all(feature = "desktop", target_os = "windows"))]
use window_vibrancy::apply_acrylic;

#[cfg(feature = "desktop")]
#[derive(Default)]
struct ManagedState {
    sync_handle: Mutex<Option<SyncHandle>>,
    last_sync_ms: Mutex<Option<u64>>,
    log_history: LogHistory,
}

#[cfg(feature = "desktop")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncStatus {
    running: bool,
    next_sync_secs: Option<u64>,
    last_sync_ms: Option<u64>,
    last_error: Option<String>,
}

#[cfg(feature = "desktop")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionResult {
    ok: bool,
    message: String,
}

#[cfg(feature = "desktop")]
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(feature = "desktop")]
fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|p| p.join("settings.json"))
        .map_err(|e| format!("Cannot locate config dir: {e}"))
}

#[cfg(feature = "desktop")]
fn lock<T>(m: &Mutex<T>) -> Result<MutexGuard<'_, T>, String> {
    m.lock().map_err(|_| "State lock poisoned.".to_string())
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn load_settings(app: AppHandle) -> Result<SyncConfig, String> {
    let path = settings_path(&app)?;
    if !path.exists() {
        return Ok(SyncConfig::default());
    }
    let contents = fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read settings: {e}"))?;
    serde_json::from_str(&contents).map_err(|e| format!("Cannot parse settings: {e}"))
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn save_settings(app: AppHandle, config: SyncConfig) -> Result<(), String> {
    let path = settings_path(&app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create config dir: {e}"))?;
    }
    let contents =
        serde_json::to_string_pretty(&config).map_err(|e| format!("Cannot serialize: {e}"))?;
    fs::write(&path, contents).map_err(|e| format!("Cannot write settings: {e}"))
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn start_sync(
    app: AppHandle,
    state: State<'_, ManagedState>,
    config: SyncConfig,
) -> Result<SyncStatus, String> {
    save_settings(app, config.clone())?;

    {
        let mut handle = lock(&state.sync_handle)?;
        *handle = None;
    }

    let interval_minutes = config.interval_minutes;
    let handle = start_sync_worker(config, state.log_history.clone()).await?;

    let next_sync_secs = if interval_minutes > 0 {
        Some(interval_minutes * 60)
    } else {
        None
    };

    *lock(&state.sync_handle)? = Some(handle);

    Ok(SyncStatus {
        running: true,
        next_sync_secs,
        last_sync_ms: *lock(&state.last_sync_ms)?,
        last_error: None,
    })
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn stop_sync(state: State<'_, ManagedState>) -> Result<SyncStatus, String> {
    *lock(&state.sync_handle)? = None;
    Ok(SyncStatus {
        running: false,
        next_sync_secs: None,
        last_sync_ms: *lock(&state.last_sync_ms)?,
        last_error: None,
    })
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn sync_now(state: State<'_, ManagedState>, config: SyncConfig) -> Result<u64, String> {
    let result = execute_sync(&config, &state.log_history).await?;
    *lock(&state.last_sync_ms)? = Some(now_ms());
    Ok(result)
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn sync_status(state: State<'_, ManagedState>) -> Result<SyncStatus, String> {
    let handle = lock(&state.sync_handle)?;
    let running = handle.is_some();
    let next_sync_secs = handle.as_ref().and_then(|h| {
        if h.interval_minutes == 0 {
            return None;
        }
        let elapsed_ms = now_ms().saturating_sub(h.started_at_ms);
        let interval_ms = h.interval_minutes * 60 * 1000;
        let next_ms = interval_ms.saturating_sub(elapsed_ms % interval_ms);
        Some(next_ms / 1000)
    });

    Ok(SyncStatus {
        running,
        next_sync_secs,
        last_sync_ms: *lock(&state.last_sync_ms)?,
        last_error: None,
    })
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn activity_logs(state: State<'_, ManagedState>) -> Result<Vec<LogEntry>, String> {
    Ok(lock(&state.log_history)?
        .iter()
        .rev()
        .take(300)
        .cloned()
        .collect())
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn clear_logs(state: State<'_, ManagedState>) -> Result<(), String> {
    lock(&state.log_history)?.clear();
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn test_connection(db_config: DbConfig) -> Result<ConnectionResult, String> {
    match open_pool(&db_config).await {
        Ok(pool) => {
            pool.close().await;
            Ok(ConnectionResult {
                ok: true,
                message: format!(
                    "Connected to {}:{}/{}",
                    db_config.host, db_config.port, db_config.database
                ),
            })
        }
        Err(e) => Ok(ConnectionResult {
            ok: false,
            message: e,
        }),
    }
}

#[cfg(feature = "desktop")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(ManagedState {
            sync_handle: Mutex::new(None),
            last_sync_ms: Mutex::new(None),
            log_history: Arc::new(Mutex::new(VecDeque::new())),
        })
        .invoke_handler(tauri::generate_handler![
            load_settings,
            save_settings,
            start_sync,
            stop_sync,
            sync_now,
            sync_status,
            activity_logs,
            clear_logs,
            test_connection,
        ])
        .setup(|app| {
            #[cfg(target_os = "windows")]
            {
                let window = app.get_webview_window("main").unwrap();
                if let Err(e) = apply_acrylic(&window, Some((18, 18, 18, 125))) {
                    eprintln!("Acrylic effect failed: {e}");
                }
            }

            #[cfg(feature = "desktop")]
            {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    if let Ok(config) = load_settings(app_handle.clone()) {
                        let ready = !config.local_db.host.trim().is_empty()
                            && !config.local_db.database.trim().is_empty()
                            && !config.remote_db.host.trim().is_empty()
                            && !config.remote_db.database.trim().is_empty()
                            && !config.table_mappings.is_empty();
                        if ready {
                            let state = app_handle.state::<ManagedState>();
                            let handle =
                                start_sync_worker(config, state.log_history.clone()).await;
                            if let Ok(h) = handle {
                                if let Ok(mut guard) = state.sync_handle.lock() {
                                    *guard = Some(h);
                                }
                            }
                        }
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(not(feature = "desktop"))]
pub fn run() {
    panic!("Requires the `desktop` feature.");
}
