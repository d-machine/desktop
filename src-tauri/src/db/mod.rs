pub mod migrations;

use anyhow::Result;
use once_cell::sync::OnceCell;
use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

// Inner type allows us to take() and replace the connection (needed for import).
static DB: OnceCell<Mutex<Option<Connection>>> = OnceCell::new();

/// A guard that derefs to &Connection.
/// Holds the DB mutex for its lifetime — drop as soon as you're done.
pub struct ConnGuard(MutexGuard<'static, Option<Connection>>);

impl std::ops::Deref for ConnGuard {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.0.as_ref().expect("Database is not open")
    }
}

/// Acquire the DB connection. Use this in all commands.
/// Returns an error if the mutex is poisoned or DB is not initialized.
pub fn acquire() -> Result<ConnGuard, String> {
    let guard = DB
        .get()
        .expect("Database not initialized — unlock the app first")
        .lock()
        .map_err(|e| e.to_string())?;
    Ok(ConnGuard(guard))
}

/// Acquire the raw mutex guard (needed for import — allows take() + replace).
pub fn acquire_mut() -> Result<MutexGuard<'static, Option<Connection>>, String> {
    DB.get()
        .expect("Database not initialized")
        .lock()
        .map_err(|e| e.to_string())
}

/// Initialize the database. Called once on app startup.
pub fn init(db_path: PathBuf) -> Result<()> {
    let conn = open_conn(&db_path)?;
    run_migrations(&conn)?;
    DB.set(Mutex::new(Some(conn)))
        .map_err(|_| anyhow::anyhow!("Database already initialized"))?;
    Ok(())
}

/// Close the current connection, replace the db file, reopen.
/// Used exclusively by the import flow.
pub fn replace(db_path: &PathBuf) -> Result<(), String> {
    let mut guard = acquire_mut()?;
    // Close existing connection by dropping it
    let old = guard.take();
    drop(old);
    // Open fresh connection to the new file (already in place at db_path)
    let conn = open_conn(db_path).map_err(|e| e.to_string())?;
    run_migrations(&conn).map_err(|e| e.to_string())?;
    *guard = Some(conn);
    Ok(())
}

fn open_conn(db_path: &PathBuf) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    Ok(conn)
}

/// Run all pending migrations in order.
fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            version    INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;

    let applied: i64 = conn.query_row(
        "SELECT COUNT(*) FROM _migrations",
        [],
        |row| row.get(0),
    )?;

    let migrations = migrations::MIGRATIONS;
    let pending = &migrations[applied as usize..];

    if pending.is_empty() {
        return Ok(());
    }

    for (i, sql) in pending.iter().enumerate() {
        let version = applied as usize + i + 1;
        conn.execute_batch(sql)?;
        conn.execute(
            "INSERT INTO _migrations (version) VALUES (?1)",
            [version],
        )?;
    }

    Ok(())
}
