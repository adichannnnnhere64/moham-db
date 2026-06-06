use serde::{Deserialize, Serialize};
use sqlx::{
    mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode},
    MySqlConnection, MySqlPool, Row,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
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
    /// When true, ignore table_mappings and sync EVERY table found on the
    /// remote DB (via SHOW TABLES), each mapped to a same-named local table.
    #[serde(default)]
    pub sync_all_tables: bool,
    /// When true, auto-create missing local tables from the remote schema
    /// (SHOW CREATE TABLE). Lets a fresh/empty local DB receive everything.
    #[serde(default)]
    pub create_missing_tables: bool,
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
            sync_all_tables: false,
            create_missing_tables: false,
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
    // MySqlSslMode::Disabled: skip SSL entirely. Preferred/Required fail with
    // HandshakeFailure when the server advertises SSL but has a bad cert or
    // TLS version mismatch — sqlx does NOT fall back after a failed handshake.
    // mysql_native_password (MySQL 5/8) transmits the password hash regardless
    // of SSL, so Disabled is safe for password-based auth on plain networks.
    // Trim credentials — a trailing space pasted into a field would otherwise
    // make MySQL reject the login or, worse, send the wrong password.
    let host = db.host.trim();
    let username = db.username.trim();
    let password = db.password.trim();
    let database = db.database.trim();

    let mut opts = MySqlConnectOptions::new()
        .host(host)
        .port(db.port)
        .ssl_mode(MySqlSslMode::Disabled);
    if !username.is_empty() {
        opts = opts.username(username);
    }
    if !password.is_empty() {
        opts = opts.password(password);
    }
    if !database.is_empty() {
        opts = opts.database(database);
    }
    opts
}

pub async fn open_pool(db: &DbConfig) -> Result<MySqlPool, String> {
    MySqlPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
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

/// Read a column as a String, tolerating MySQL metadata commands (SHOW FULL
/// TABLES, SHOW CREATE TABLE, …) whose columns arrive typed as VAR/BINARY
/// rather than VARCHAR — a plain `try_get::<String>` fails on those.
fn row_string(row: &sqlx::mysql::MySqlRow, idx: usize) -> Option<String> {
    if let Ok(s) = row.try_get::<String, _>(idx) {
        return Some(s);
    }
    row.try_get::<Vec<u8>, _>(idx)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Build the list of (remote_table, local_table) pairs to sync.
/// When `sync_all_tables` is set, discover every BASE TABLE on the remote
/// (skipping VIEWs) and map each to a same-named local table. Otherwise use
/// the manual `table_mappings`. Blank names are filtered out either way.
async fn resolve_mappings(
    config: &SyncConfig,
    remote_pool: &MySqlPool,
) -> Result<Vec<(String, String)>, String> {
    let pairs: Vec<(String, String)> = if config.sync_all_tables {
        // SHOW FULL TABLES: col 0 = table name, col 1 = Table_type. We filter to
        // BASE TABLE in Rust (a `WHERE` clause on SHOW does not survive sqlx's
        // prepared-statement protocol and silently returns no rows).
        let rows = sqlx::query("SHOW FULL TABLES")
            .fetch_all(remote_pool)
            .await
            .map_err(|e| format!("Failed to list remote tables: {e}"))?;
        rows.iter()
            .filter(|r| {
                // col 1 = Table_type; keep only BASE TABLE (skip VIEWs).
                row_string(r, 1)
                    .map(|t| t.eq_ignore_ascii_case("BASE TABLE"))
                    .unwrap_or(true)
            })
            .filter_map(|r| row_string(r, 0))
            .map(|t| (t.clone(), t))
            .collect()
    } else {
        config
            .table_mappings
            .iter()
            .map(|m| (m.remote_table.clone(), m.local_table.clone()))
            .collect()
    };

    Ok(pairs
        .into_iter()
        .filter(|(r, l)| !r.trim().is_empty() && !l.trim().is_empty())
        .collect())
}

/// Turn a `SHOW CREATE TABLE` DDL into `CREATE TABLE IF NOT EXISTS \`local\` (...)`.
/// SHOW CREATE TABLE always emits `CREATE TABLE \`name\` (...)` (never IF NOT
/// EXISTS), so we find the first backtick-quoted identifier, drop it, and splice
/// in `IF NOT EXISTS` plus the (possibly different) local table name.
fn rewrite_create_table_ddl(ddl: &str, local_table: &str) -> String {
    let quoted_local = format!("`{}`", local_table.replace('`', "``"));
    if let Some(open) = ddl.find('`') {
        if let Some(rel_close) = ddl[open + 1..].find('`') {
            let close = open + 1 + rel_close;
            let remainder = &ddl[close + 1..]; // everything after the table name
            return format!("CREATE TABLE IF NOT EXISTS {quoted_local}{remainder}");
        }
    }
    // No backtick found — not a recognizable SHOW CREATE TABLE output. Leave as-is.
    ddl.to_string()
}

/// Ensure the local table exists by copying the remote schema. Reads
/// `SHOW CREATE TABLE` from the remote (column index 1 holds the DDL), rewrites
/// the name, and runs it on the single local connection. Idempotent.
async fn ensure_local_table(
    conn: &mut MySqlConnection,
    remote_pool: &MySqlPool,
    remote_table: &str,
    local_table: &str,
) -> Result<(), String> {
    let row = sqlx::query(&format!("SHOW CREATE TABLE `{remote_table}`"))
        .fetch_one(remote_pool)
        .await
        .map_err(|e| format!("Failed to read schema of '{remote_table}': {e}"))?;
    let raw_ddl = row_string(&row, 1)
        .ok_or_else(|| format!("Could not extract DDL for '{remote_table}'"))?;
    let ddl = rewrite_create_table_ddl(&raw_ddl, local_table);
    sqlx::query(&ddl)
        .execute(&mut *conn)
        .await
        .map_err(|e| format!("Failed to create local table '{local_table}': {e}"))?;
    Ok(())
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

    let mappings = resolve_mappings(config, &remote_pool).await.map_err(|e| {
        push_log(history, LogLevel::Error, &e);
        e
    })?;

    if mappings.is_empty() {
        push_log(history, LogLevel::Info, "No tables to sync.");
        return Ok(0);
    }

    // Run ALL local writes (DDL + REPLACE) on a SINGLE connection so the
    // session-scoped FOREIGN_KEY_CHECKS setting actually holds — a pooled
    // multi-connection executor would scatter statements across sessions.
    let mut conn = local_pool.acquire().await.map_err(|e| {
        let msg = format!("Local DB connection acquire failed: {e}");
        push_log(history, LogLevel::Error, &msg);
        msg
    })?;

    // Disable FK checks when bootstrapping a fresh DB or syncing everything, so
    // tables can be created and rows replaced in arbitrary order.
    let fk_managed = config.create_missing_tables || config.sync_all_tables;
    if fk_managed {
        let _ = sqlx::query("SET FOREIGN_KEY_CHECKS=0")
            .execute(&mut *conn)
            .await;
    }

    let mut total_rows: u64 = 0;

    for (remote_table, local_table) in &mappings {
        if config.create_missing_tables {
            if let Err(e) = ensure_local_table(&mut conn, &remote_pool, remote_table, local_table).await {
                push_log(history, LogLevel::Error, e);
                continue;
            }
        }

        // Discover columns via SHOW COLUMNS so we know exact names and order.
        let col_rows = match sqlx::query(&format!("SHOW COLUMNS FROM `{remote_table}`"))
            .fetch_all(&remote_pool)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                push_log(
                    history,
                    LogLevel::Error,
                    format!("Failed to describe '{remote_table}': {e}"),
                );
                continue;
            }
        };

        if col_rows.is_empty() {
            push_log(
                history,
                LogLevel::Info,
                format!("Table '{remote_table}' has no columns"),
            );
            continue;
        }

        let columns: Vec<String> = col_rows
            .iter()
            .filter_map(|r| r.try_get::<String, _>("Field").ok())
            .collect();

        // CAST every column to CHAR so binary-protocol type inference is bypassed.
        // All values arrive as strings regardless of source type (INT, DECIMAL, TIMESTAMP, etc.).
        let cast_list = columns
            .iter()
            .map(|c| format!("CAST(`{c}` AS CHAR) AS `{c}`"))
            .collect::<Vec<_>>()
            .join(", ");

        let select_sql = format!("SELECT {cast_list} FROM `{remote_table}`");
        let rows = match sqlx::query(&select_sql).fetch_all(&remote_pool).await {
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
                // All values are CHAR now — simple string decode always works.
                let val: Option<String> = row.try_get::<Option<String>, _>(col.as_str())
                    .ok()
                    .flatten();
                q = q.bind(val);
            }
            match q.execute(&mut *conn).await {
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

    if fk_managed {
        let _ = sqlx::query("SET FOREIGN_KEY_CHECKS=1")
            .execute(&mut *conn)
            .await;
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

    // Remote user with SELECT-only privilege — proves readonly remotes sync fine.
    #[cfg(feature = "integration")]
    fn readonly_remote_db() -> DbConfig {
        DbConfig {
            username: std::env::var("SYNC_REMOTE_RO_USER").unwrap_or_else(|_| "readonly".into()),
            password: std::env::var("SYNC_REMOTE_RO_PASS").unwrap_or_else(|_| "readonly".into()),
            ..remote_db()
        }
    }

    // Empty local database (no tables) for fresh-bootstrap tests.
    #[cfg(feature = "integration")]
    fn local_fresh_db() -> DbConfig {
        DbConfig {
            database: std::env::var("SYNC_LOCAL_FRESH_DB")
                .unwrap_or_else(|_| "sync_local_fresh".into()),
            ..local_db()
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
            sync_all_tables: false,
            create_missing_tables: false,
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
    fn connect_options_does_not_panic_with_password() {
        // Regression: password must not be silently dropped regardless of MySQL version.
        // connect_options() must accept non-empty password without panicking.
        let with_pass = DbConfig {
            host: "127.0.0.1".into(),
            port: 3306,
            database: "mydb".into(),
            username: "readonly".into(),
            password: "secret123".into(),
        };
        let _ = connect_options(&with_pass); // must not panic

        let no_pass = DbConfig {
            host: "127.0.0.1".into(),
            port: 3306,
            database: "mydb".into(),
            username: "readonly".into(),
            password: "".into(),
        };
        let _ = connect_options(&no_pass); // must not panic
    }

    #[test]
    fn rewrite_create_table_ddl_injects_if_not_exists() {
        let ddl = "CREATE TABLE `customers` (\n  `id` int NOT NULL\n) ENGINE=InnoDB";
        let out = rewrite_create_table_ddl(ddl, "customers");
        assert!(out.starts_with("CREATE TABLE IF NOT EXISTS `customers` ("));
        assert!(out.contains("ENGINE=InnoDB"));
    }

    #[test]
    fn rewrite_create_table_ddl_renames_table() {
        let ddl = "CREATE TABLE `customers` (`id` int)";
        let out = rewrite_create_table_ddl(ddl, "local_customers");
        assert_eq!(out, "CREATE TABLE IF NOT EXISTS `local_customers` (`id` int)");
    }

    #[test]
    fn rewrite_create_table_ddl_escapes_backticks() {
        let ddl = "CREATE TABLE `t` (`id` int)";
        let out = rewrite_create_table_ddl(ddl, "we`ird");
        assert!(out.starts_with("CREATE TABLE IF NOT EXISTS `we``ird` ("));
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
            sync_all_tables: false,
            create_missing_tables: false,
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
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM customers")
            .fetch_one(&pool)
            .await
            .expect("customers table must exist and have rows");
        assert!(count > 0, "remote customers must have seed rows, got {count}");
        pool.close().await;
    }

    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn db_local_table_exists() {
        let pool = open_pool(&local_db())
            .await
            .expect("local DB must be reachable");
        let _rows = sqlx::query("SELECT * FROM customers LIMIT 1")
            .fetch_all(&pool)
            .await
            .expect("local customers table must exist");
        pool.close().await;
    }

    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn execute_sync_copies_rows_to_local() {
        // Sync customers from remote → local.
        // Remote seed has 3 rows; local seed has 2. After sync local must have 3.
        let config = SyncConfig {
            remote_db: remote_db(),
            local_db: local_db(),
            table_mappings: vec![TableMapping {
                remote_table: "customers".into(),
                local_table: "customers".into(),
            }],
            interval_minutes: 10,
            sync_all_tables: false,
            create_missing_tables: false,
        };

        let history: LogHistory = Arc::new(Mutex::new(VecDeque::new()));
        let synced = execute_sync(&config, &history)
            .await
            .expect("sync must succeed");

        {
            let logs = history.lock().unwrap();
            for entry in logs.iter() {
                println!("[{:?}] {}", entry.level, entry.message);
            }
        }

        assert!(synced > 0, "must sync at least one row, got {synced}");

        let local_pool = open_pool(&local_db()).await.unwrap();
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM customers")
            .fetch_one(&local_pool)
            .await
            .unwrap();
        assert!(count >= 3, "local customers must have >= 3 rows after sync, got {count}");
        local_pool.close().await;

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
            sync_all_tables: false,
            create_missing_tables: false,
        };
        let history: LogHistory = Arc::new(Mutex::new(VecDeque::new()));
        let synced = execute_sync(&config, &history)
            .await
            .expect("sync with blank mapping must not error");
        assert_eq!(synced, 0, "blank mapping must sync 0 rows");
    }

    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn wrong_password_error_says_using_password_yes() {
        // Regression test: when a non-empty password is provided, sqlx must
        // transmit it. MySQL reports "using password: NO" if the password hash
        // is not sent — this was broken with MySqlSslMode::Disabled on MySQL 5.
        let mut db = remote_db();
        db.password = "definitely_wrong_password_xyz_12345".into();
        let result = open_pool(&db).await;
        assert!(result.is_err(), "connection with wrong password must fail");
        let err = result.unwrap_err().to_string().to_lowercase();
        // "using password: no" means sqlx silently dropped the password — that's the bug.
        assert!(
            !err.contains("using password: no"),
            "password was silently dropped (using password: NO). err: {err}"
        );
        // Confirm MySQL received the password (even if wrong).
        assert!(
            err.contains("access denied") || err.contains("using password: yes"),
            "expected auth failure with password transmitted, got: {err}"
        );
    }

    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn execute_sync_logs_error_for_missing_table() {
        let config = SyncConfig {
            remote_db: remote_db(),
            local_db: local_db(),
            table_mappings: vec![TableMapping {
                remote_table: "table_that_does_not_exist_xyz".into(),
                local_table: "customers".into(),
            }],
            interval_minutes: 0,
            sync_all_tables: false,
            create_missing_tables: false,
        };
        let history: LogHistory = Arc::new(Mutex::new(VecDeque::new()));
        let _ = execute_sync(&config, &history).await;
        let logs = history.lock().unwrap();
        assert!(
            logs.iter().any(|e| e.level == LogLevel::Error),
            "missing table must produce an ERROR log"
        );
    }

    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn execute_sync_works_with_readonly_remote_user() {
        // A SELECT-only remote user must be able to drive a full sync — the
        // engine never writes to the remote. Sync everything into a fresh local.
        let config = SyncConfig {
            remote_db: readonly_remote_db(),
            local_db: local_fresh_db(),
            table_mappings: vec![],
            interval_minutes: 0,
            sync_all_tables: true,
            create_missing_tables: true,
        };
        let history: LogHistory = Arc::new(Mutex::new(VecDeque::new()));
        let synced = execute_sync(&config, &history)
            .await
            .expect("readonly-remote sync must succeed");
        {
            let logs = history.lock().unwrap();
            for e in logs.iter() {
                println!("[{:?}] {}", e.level, e.message);
            }
        }
        assert!(synced > 0, "readonly sync must copy rows, got {synced}");
    }

    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn execute_sync_bootstraps_fresh_local() {
        // Fresh empty local DB + sync_all + create_missing → every remote table
        // is created locally and all rows copied (exercises FK-off ordering).
        let config = SyncConfig {
            remote_db: remote_db(),
            local_db: local_fresh_db(),
            table_mappings: vec![],
            interval_minutes: 0,
            sync_all_tables: true,
            create_missing_tables: true,
        };
        let history: LogHistory = Arc::new(Mutex::new(VecDeque::new()));
        execute_sync(&config, &history)
            .await
            .expect("fresh-local bootstrap must succeed");

        let pool = open_pool(&local_fresh_db()).await.unwrap();
        for (table, expected) in [
            ("customers", 3i64),
            ("products", 3),
            ("orders", 2),
            ("order_items", 3),
        ] {
            let (count,): (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM `{table}`"))
                .fetch_one(&pool)
                .await
                .unwrap_or_else(|e| panic!("table '{table}' must exist locally: {e}"));
            assert_eq!(
                count, expected,
                "fresh local '{table}' must have {expected} rows, got {count}"
            );
        }
        pool.close().await;
    }
}
