use crate::db;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct Portfolio {
    pub portfolio_id: i64,
    pub name: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreatePortfolioInput {
    pub name: String,
}

#[tauri::command]
pub fn get_portfolios() -> Result<Vec<Portfolio>, String> {
    let conn = db::acquire()?;
    let mut stmt = conn
        .prepare("SELECT portfolio_id, name, created_at FROM portfolios ORDER BY portfolio_id")
        .map_err(|e| e.to_string())?;

    let portfolios = stmt
        .query_map([], |row| {
            Ok(Portfolio {
                portfolio_id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(portfolios)
}

#[tauri::command]
pub fn create_portfolio(input: CreatePortfolioInput) -> Result<Portfolio, String> {
    let conn = db::acquire()?;
    conn.execute("INSERT INTO portfolios (name) VALUES (?1)", [&input.name])
        .map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    let portfolio = conn
        .query_row(
            "SELECT portfolio_id, name, created_at FROM portfolios WHERE portfolio_id = ?1",
            [id],
            |row| {
                Ok(Portfolio {
                    portfolio_id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                })
            },
        )
        .map_err(|e| e.to_string())?;

    Ok(portfolio)
}

#[tauri::command]
pub fn rename_portfolio(portfolio_id: i64, name: String) -> Result<(), String> {
    let conn = db::acquire()?;
    conn.execute(
        "UPDATE portfolios SET name = ?1 WHERE portfolio_id = ?2",
        rusqlite::params![name, portfolio_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_portfolio(portfolio_id: i64) -> Result<(), String> {
    let conn = db::acquire()?;
    // Check no accounts exist under this portfolio
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE portfolio_id = ?1",
            [portfolio_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    if count > 0 {
        return Err("Cannot delete portfolio with existing accounts. Delete accounts first.".into());
    }

    conn.execute(
        "DELETE FROM portfolios WHERE portfolio_id = ?1",
        [portfolio_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
