use crate::db;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct Account {
    pub account_id: i64,
    pub portfolio_id: i64,
    pub name: String,
    pub account_type: String,
    pub broker: Option<String>,
    pub account_no: Option<String>,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreateAccountInput {
    pub portfolio_id: i64,
    pub name: String,
    pub account_type: String,
    pub broker: Option<String>,
    pub account_no: Option<String>,
}

#[tauri::command]
pub fn get_accounts(portfolio_id: Option<i64>) -> Result<Vec<Account>, String> {
    let conn = db::acquire()?;

    let sql = match portfolio_id {
        Some(_) =>
            "SELECT account_id, portfolio_id, name, account_type, broker, account_no, created_at
             FROM accounts WHERE portfolio_id = ?1 ORDER BY account_id",
        None =>
            "SELECT account_id, portfolio_id, name, account_type, broker, account_no, created_at
             FROM accounts ORDER BY account_id",
    };

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;

    let map_row = |row: &rusqlite::Row| {
        Ok(Account {
            account_id: row.get(0)?,
            portfolio_id: row.get(1)?,
            name: row.get(2)?,
            account_type: row.get(3)?,
            broker: row.get(4)?,
            account_no: row.get(5)?,
            created_at: row.get(6)?,
        })
    };

    let accounts = match portfolio_id {
        Some(pid) => stmt.query_map([pid], map_row),
        None => stmt.query_map([], map_row),
    }
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;

    Ok(accounts)
}

#[tauri::command]
pub fn create_account(input: CreateAccountInput) -> Result<Account, String> {
    let conn = db::acquire()?;
    conn.execute(
        "INSERT INTO accounts (portfolio_id, name, account_type, broker, account_no)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            input.portfolio_id,
            input.name,
            input.account_type,
            input.broker,
            input.account_no,
        ],
    )
    .map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    conn.query_row(
        "SELECT account_id, portfolio_id, name, account_type, broker, account_no, created_at
         FROM accounts WHERE account_id = ?1",
        [id],
        |row| {
            Ok(Account {
                account_id: row.get(0)?,
                portfolio_id: row.get(1)?,
                name: row.get(2)?,
                account_type: row.get(3)?,
                broker: row.get(4)?,
                account_no: row.get(5)?,
                created_at: row.get(6)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_account(account_id: i64, name: String) -> Result<(), String> {
    let conn = db::acquire()?;
    conn.execute(
        "UPDATE accounts SET name = ?1 WHERE account_id = ?2",
        rusqlite::params![name, account_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_account(account_id: i64) -> Result<(), String> {
    let conn = db::acquire()?;
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE account_id = ?1",
            [account_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    if count > 0 {
        return Err(format!(
            "Cannot delete account with {count} existing transactions. Remove transactions first."
        ));
    }

    conn.execute("DELETE FROM accounts WHERE account_id = ?1", [account_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}
