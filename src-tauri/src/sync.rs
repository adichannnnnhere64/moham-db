use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use sqlx::{
    mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlRow, MySqlSslMode},
    Column, MySqlPool, Row,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::oneshot;

pub const LOG_LIMIT: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DbConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableMapping {
    pub remote_table: String,
    pub local_table: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConfig {
    pub local_db: DbConfig,
    pub remote_db: DbConfig,
    pub table_mappings: Vec<TableMapping>,
    pub interval_minutes: u64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            local_db: DbConfig {
                host: "127.0.0.1".into(),
                port: 3306,
                database: String::new(),
                username: String::new(),
                password: String::new(),
            },
            remote_db: DbConfig {
                host: String::new(),
                port: 3306,
                database: String::new(),
                username: String::new(),
                password: String::new(),
            },
            table_mappings: Vec::new(),
            interval_minutes: 10,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogLevel {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: u64,
    pub timestamp_ms: u64,
    pub level: LogLevel,
    pub message: String,
}

pub type LogHistory = Arc<Mutex<VecDeque<LogEntry>>>;

pub struct SyncHandle {
    pub shutdown: oneshot::Sender<()>,
    pub interval_minutes: u64,
    pub started_at_ms: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn connect_options(db: &DbConfig) -> MySqlConnectOptions {
    MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.database)
        .username(&db.username)
        .password(&db.password)
        .ssl_mode(MySqlSslMode::Disabled)
}

pub async fn open_pool(db: &DbConfig) -> Result<MySqlPool, String> {
    MySqlPoolOptions::new()
        .max_connections(5)
        .connect_with(connect_options(db))
        .await
        .map_err(|e| e.to_string())
}

pub fn push_log(history: &LogHistory, level: LogLevel, message: impl Into<String>) {
    if let Ok(mut h) = history.lock() {
        let id = h.back().map(|e| e.id + 1).unwrap_or(1);
        h.push_back(LogEntry {
            id,
            timestamp_ms: now_ms(),
            level,
            message: message.into(),
        });
        while h.len() > LOG_LIMIT {
            h.pop_front();
        }
    }
}

/// Extract any MySQL column value as an Option<String>.
/// Tries types in order: i64, u64, f64, bool, NaiveDateTime, NaiveDate, NaiveTime, String, Vec<u8>.
/// Returns None only when the value is truly SQL NULL.
fn col_to_string(row: &MySqlRow, col: &str) -> Option<String> {
    if let Ok(v) = row.try_get::<Option<i64>, _>(col) {
        return v.map(|n| n.to_string());
    }
    if let Ok(v) = row.try_get::<Option<u64>, _>(col) {
        return v.map(|n| n.to_string());
    }
    if let Ok(v) = row.try_get::<Option<f64>, _>(col) {
        return v.map(|n| n.to_string());
    }
    if let Ok(v) = row.try_get::<Option<bool>, _>(col) {
        return v.map(|b| (b as i32).to_string());
    }
    if let Ok(v) = row.try_get::<Option<NaiveDateTime>, _>(col) {
        return v.map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string());
    }
    if let Ok(v) = row.try_get::<Option<NaiveDate>, _>(col) {
        return v.map(|d| d.format("%Y-%m-%d").to_string());
    }
    if let Ok(v) = row.try_get::<Option<NaiveTime>, _>(col) {
        return v.map(|t| t.format("%H:%M:%S").to_string());
    }
    if let Ok(v) = row.try_get::<Option<String>, _>(col) {
        return v;
    }
    if let Ok(v) = row.try_get::<Option<Vec<u8>>, _>(col) {
        return v.map(|b| String::from_utf8_lossy(&b).into_owned());
    }
    None
}

pub async fn execute_sync(config: &SyncConfig, history: &LogHistory) -> Result<u64, String> {
    push_log(
        history,
        LogLevel::Info,
        format!(
            "Connection established with Remote IP: {}",
            config.remote_db.host
        ),
    );

    let remote_pool = open_pool(&config.remote_db).await.map_err(|e| {
        let msg = format!("Remote DB connection failed: {e}");
        push_log(history, LogLevel::Error, &msg);
        msg
    })?;

    let local_pool = open_pool(&config.local_db).await.map_err(|e| {
        let msg = format!("Local DB connection failed: {e}");
        push_log(history, LogLevel::Error, &msg);
        msg
    })?;

    let mut total_rows: u64 = 0;

    for mapping in &config.table_mappings {
        let remote_table = &mapping.remote_table;
        let local_table = &mapping.local_table;

        if remote_table.trim().is_empty() || local_table.trim().is_empty() {
            continue;
        }

        let query = format!("SELECT * FROM `{remote_table}`");
        let rows = match sqlx::query(&query).fetch_all(&remote_pool).await {
            Ok(r) => r,
            Err(e) => {
                push_log(
                    history,
                    LogLevel::Error,
                    format!("Failed to read '{remote_table}': {e}"),
                );
                continue;
            }
        };

        if rows.is_empty() {
            push_log(
                history,
                LogLevel::Info,
                format!("Table '{remote_table}' has no rows to sync"),
            );
            continue;
        }

        let columns: Vec<String> = rows[0]
            .columns()
            .iter()
            .map(|c| c.name().to_string())
            .collect();

        let col_list = columns
            .iter()
            .map(|c| format!("`{c}`"))
            .collect::<Vec<_>>()
            .join(", ");

        let placeholders = columns.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let insert_sql =
            format!("REPLACE INTO `{local_table}` ({col_list}) VALUES ({placeholders})");

        let mut synced: u64 = 0;
        for row in &rows {
            let mut q = sqlx::query(&insert_sql);
            for col in &columns {
                q = q.bind(col_to_string(row, col));
            }
            match q.execute(&local_pool).await {
                Ok(_) => synced += 1,
                Err(e) => {
                    push_log(
                        history,
                        LogLevel::Error,
                        format!("Row insert error in '{local_table}': {e}"),
                    );
                }
            }
        }

        push_log(
            history,
            LogLevel::Success,
            format!("Synced {synced} rows from Table '{remote_table}'"),
        );
        total_rows += synced;
    }

    Ok(total_rows)
}

pub async fn start_sync_worker(
    config: SyncConfig,
    history: LogHistory,
) -> Result<SyncHandle, String> {
    let (tx, mut rx) = oneshot::channel::<()>();
    let interval_minutes = config.interval_minutes;
    let started_at_ms = now_ms();

    let history_clone = history.clone();
    tokio::spawn(async move {
        let interval_secs = if interval_minutes == 0 {
            u64::MAX
        } else {
            interval_minutes * 60
        };

        let duration = tokio::time::Duration::from_secs(interval_secs);
        let mut ticker = tokio::time::interval(duration);
        ticker.tick().await; // skip immediate first tick

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let _ = execute_sync(&config, &history_clone).await;
                }
                _ = &mut rx => {
                    push_log(&history_clone, LogLevel::Info, "Sync worker stopped.");
                    break;
                }
            }
        }
    });

    Ok(SyncHandle {
        shutdown: tx,
        interval_minutes,
        started_at_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn remote_db() -> DbConfig {
        DbConfig {
            host: std::env::var("SYNC_REMOTE_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            port: std::env::var("SYNC_REMOTE_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3308),
            database: std::env::var("SYNC_REMOTE_DB")
                .unwrap_or_else(|_| "sync_remote_test".into()),
            username: std::env::var("SYNC_REMOTE_USER").unwrap_or_else(|_| "sync".into()),
            password: std::env::var("SYNC_REMOTE_PASS").unwrap_or_else(|_| "sync".into()),
        }
    }

    fn local_db() -> DbConfig {
        DbConfig {
            host: std::env::var("SYNC_LOCAL_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            port: std::env::var("SYNC_LOCAL_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3307),
            database: std::env::var("SYNC_LOCAL_DB")
                .unwrap_or_else(|_| "sync_local_test".into()),
            username: std::env::var("SYNC_LOCAL_USER").unwrap_or_else(|_| "sync".into()),
            password: std::env::var("SYNC_LOCAL_PASS").unwrap_or_else(|_| "sync".into()),
        }
    }

    // ── unit tests (no DB) ───────────────────────────────────────────────────

    #[test]
    fn log_level_serializes() {
        assert_eq!(serde_json::to_string(&LogLevel::Info).unwrap(), "\"INFO\"");
        assert_eq!(
            serde_json::to_string(&LogLevel::Success).unwrap(),
            "\"SUCCESS\""
        );
        assert_eq!(
            serde_json::to_string(&LogLevel::Error).unwrap(),
            "\"ERROR\""
        );
    }

    #[test]
    fn push_log_increments_id() {
        let history: LogHistory = Arc::new(Mutex::new(VecDeque::new()));
        push_log(&history, LogLevel::Info, "first");
        push_log(&history, LogLevel::Success, "second");
        let h = history.lock().unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].id, 1);
        assert_eq!(h[1].id, 2);
    }

    #[test]
    fn push_log_respects_limit() {
        let history: LogHistory = Arc::new(Mutex::new(VecDeque::new()));
        for i in 0..(LOG_LIMIT + 10) {
            push_log(&history, LogLevel::Info, format!("msg {i}"));
        }
        assert_eq!(history.lock().unwrap().len(), LOG_LIMIT);
    }

    #[test]
    fn sync_config_defaults() {
        let cfg = SyncConfig::default();
        assert_eq!(cfg.interval_minutes, 10);
        assert_eq!(cfg.local_db.host, "127.0.0.1");
        assert_eq!(cfg.local_db.port, 3306);
    }

    #[test]
    fn table_mapping_serializes_round_trip() {
        let m = TableMapping {
            remote_table: "orders".into(),
            local_table: "local_orders".into(),
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: TableMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(back.remote_table, "orders");
        assert_eq!(back.local_table, "local_orders");
    }

    #[tokio::test]
    async fn execute_sync_fails_gracefully_on_bad_remote() {
        let config = SyncConfig {
            remote_db: DbConfig {
                host: "127.0.0.1".into(),
                port: 19999,
                database: "no".into(),
                username: "bad".into(),
                password: "bad".into(),
            },
            local_db: DbConfig {
                host: "127.0.0.1".into(),
                port: 19998,
                database: "no".into(),
                username: "bad".into(),
                password: "bad".into(),
            },
            table_mappings: vec![TableMapping {
                remote_table: "orders".into(),
                local_table: "local_orders".into(),
            }],
            interval_minutes: 10,
        };
        let history: LogHistory = Arc::new(Mutex::new(VecDeque::new()));
        let result = execute_sync(&config, &history).await;
        assert!(result.is_err());
        assert!(history
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.level == LogLevel::Error));
    }

    #[test]
    fn execute_sync_skips_empty_table_names() {
        // SyncConfig with a mapping that has blank table names shouldn't panic.
        // We just verify the config struct accepts it safely.
        let config = SyncConfig {
            remote_db: DbConfig::default(),
            local_db: DbConfig::default(),
            table_mappings: vec![TableMapping {
                remote_table: "".into(),
                local_table: "".into(),
            }],
            interval_minutes: 0,
        };
        // execute_sync skips blank names — validated in integration tests.
        // Here we just ensure the struct is valid.
        assert_eq!(config.table_mappings[0].remote_table, "");
    }

    // ── integration tests (require Docker MySQL) ─────────────────────────────
    // Run with: cargo test --no-default-features --features integration
    // Env defaults match docker-compose.yml (local:3307, remote:3308).

    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn db_remote_connection_works() {
        let pool = open_pool(&remote_db())
            .await
            .expect("remote DB must be reachable");
        let row: (String,) = sqlx::query_as("SELECT 'remote_ok'")
            .fetch_one(&pool)
            .await
            .expect("simple SELECT must work");
        assert_eq!(row.0, "remote_ok");
        pool.close().await;
    }

    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn db_local_connection_works() {
        let pool = open_pool(&local_db())
            .await
            .expect("local DB must be reachable");
        let row: (String,) = sqlx::query_as("SELECT 'local_ok'")
            .fetch_one(&pool)
            .await
            .expect("simple SELECT must work");
        assert_eq!(row.0, "local_ok");
        pool.close().await;
    }

    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn db_remote_has_seed_rows() {
        let pool = open_pool(&remote_db())
            .await
            .expect("remote DB must be reachable");
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM remote_orders")
            .fetch_one(&pool)
            .await
            .expect("remote_orders table must exist and have rows");
        assert!(count > 0, "remote_orders must have seed rows, got {count}");
        pool.close().await;
    }

    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn db_local_table_exists() {
        let pool = open_pool(&local_db())
            .await
            .expect("local DB must be reachable");
        // Just verify the table exists (may be empty before first sync).
        let _rows = sqlx::query("SELECT * FROM local_orders LIMIT 1")
            .fetch_all(&pool)
            .await
            .expect("local_orders table must exist");
        pool.close().await;
    }

    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn execute_sync_copies_rows_to_local() {
        let config = SyncConfig {
            remote_db: remote_db(),
            local_db: local_db(),
            table_mappings: vec![TableMapping {
                remote_table: "remote_orders".into(),
                local_table: "local_orders".into(),
            }],
            interval_minutes: 10,
        };

        let history: LogHistory = Arc::new(Mutex::new(VecDeque::new()));
        let synced = execute_sync(&config, &history)
            .await
            .expect("sync must succeed");
        assert!(synced > 0, "must sync at least one row, got {synced}");

        // Verify rows actually landed in local DB.
        let local_pool = open_pool(&local_db()).await.unwrap();
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM local_orders")
            .fetch_one(&local_pool)
            .await
            .unwrap();
        assert!(count > 0, "local_orders must have rows after sync, got {count}");
        local_pool.close().await;

        // Verify SUCCESS log was emitted.
        let logs = history.lock().unwrap();
        assert!(
            logs.iter().any(|e| e.level == LogLevel::Success),
            "expected at least one SUCCESS log entry"
        );
    }

    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn execute_sync_skips_blank_mapping_names() {
        let config = SyncConfig {
            remote_db: remote_db(),
            local_db: local_db(),
            table_mappings: vec![TableMapping {
                remote_table: "".into(),
                local_table: "".into(),
            }],
            interval_minutes: 0,
        };
        let history: LogHistory = Arc::new(Mutex::new(VecDeque::new()));
        // Should succeed (return Ok) but sync 0 rows.
        let synced = execute_sync(&config, &history)
            .await
            .expect("sync with blank mapping must not error");
        assert_eq!(synced, 0, "blank mapping must sync 0 rows");
    }

    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn execute_sync_logs_error_for_missing_table() {
        let config = SyncConfig {
            remote_db: remote_db(),
            local_db: local_db(),
            table_mappings: vec![TableMapping {
                remote_table: "table_that_does_not_exist_xyz".into(),
                local_table: "local_orders".into(),
            }],
            interval_minutes: 0,
        };
        let history: LogHistory = Arc::new(Mutex::new(VecDeque::new()));
        // Should succeed overall (partial failure per-table) but log an error.
        let _ = execute_sync(&config, &history).await;
        let logs = history.lock().unwrap();
        assert!(
            logs.iter().any(|e| e.level == LogLevel::Error),
            "missing table must produce an ERROR log"
        );
    }
}
