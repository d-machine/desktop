use crate::db;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone)]
pub struct Person {
    pub person_id: i64,
    pub name: String,
    pub pan: Option<String>,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreatePersonInput {
    pub name: String,
    pub pan: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdatePersonInput {
    pub name: Option<String>,
    pub pan: Option<String>,
}

#[tauri::command]
pub fn get_persons() -> Result<Vec<Person>, String> {
    let conn = db::acquire()?;
    let mut stmt = conn
        .prepare("SELECT person_id, name, pan, created_at FROM persons ORDER BY person_id")
        .map_err(|e| e.to_string())?;

    let persons = stmt
        .query_map([], |row| {
            Ok(Person {
                person_id: row.get(0)?,
                name:      row.get(1)?,
                pan:       row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(persons)
}

#[tauri::command]
pub fn create_person(input: CreatePersonInput) -> Result<Person, String> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err("Name is required".into());
    }
    let conn = db::acquire()?;
    conn.execute(
        "INSERT INTO persons (name, pan) VALUES (?1, ?2)",
        rusqlite::params![name, input.pan],
    )
    .map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    let person = conn
        .query_row(
            "SELECT person_id, name, pan, created_at FROM persons WHERE person_id = ?1",
            [id],
            |row| {
                Ok(Person {
                    person_id:  row.get(0)?,
                    name:       row.get(1)?,
                    pan:        row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        )
        .map_err(|e| e.to_string())?;

    Ok(person)
}

#[tauri::command]
pub fn update_person(person_id: i64, input: UpdatePersonInput) -> Result<Person, String> {
    let conn = db::acquire()?;

    if let Some(name) = &input.name {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("Name cannot be empty".into());
        }
        conn.execute(
            "UPDATE persons SET name = ?1 WHERE person_id = ?2",
            rusqlite::params![name, person_id],
        )
        .map_err(|e| e.to_string())?;
    }

    // Allow clearing PAN by passing Some("") — stored as NULL
    if let Some(pan) = &input.pan {
        let pan_val: Option<&str> = if pan.trim().is_empty() { None } else { Some(pan.trim()) };
        conn.execute(
            "UPDATE persons SET pan = ?1 WHERE person_id = ?2",
            rusqlite::params![pan_val, person_id],
        )
        .map_err(|e| e.to_string())?;
    }

    let person = conn
        .query_row(
            "SELECT person_id, name, pan, created_at FROM persons WHERE person_id = ?1",
            [person_id],
            |row| {
                Ok(Person {
                    person_id:  row.get(0)?,
                    name:       row.get(1)?,
                    pan:        row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        )
        .map_err(|e| e.to_string())?;

    Ok(person)
}
