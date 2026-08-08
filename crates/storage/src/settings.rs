use crate::StorageError;
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Arc, Mutex};

/// Puente de acceso a la tabla de configuración clave-valor (puerto hexagonal).
pub trait SettingsRepository: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError>;
    fn set(&self, key: &str, value: &str) -> Result<(), StorageError>;
}

/// Adaptador concreto sobre SQLite (comparte la conexión con los otros repos).
#[derive(Clone)]
pub struct SettingsRepo {
    conn: Arc<Mutex<Connection>>,
}

impl SettingsRepo {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

impl SettingsRepository for SettingsRepo {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(StorageError::Sqlite)
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> SettingsRepo {
        let conn = Arc::new(std::sync::Mutex::new(crate::open_db_in_memory().unwrap()));
        SettingsRepo::new(conn)
    }

    #[test]
    fn default_interval_is_present() {
        let repo = setup();
        assert_eq!(
            repo.get("refresh_interval_minutes").unwrap().as_deref(),
            Some("30")
        );
    }

    #[test]
    fn default_vector_threshold_is_present() {
        let repo = setup();
        assert_eq!(
            repo.get("vector_similarity_threshold").unwrap().as_deref(),
            Some("0.7")
        );
    }

    #[test]
    fn appearance_defaults_are_present() {
        let repo = setup();
        assert_eq!(repo.get("theme").unwrap().as_deref(), Some("system"));
        assert_eq!(repo.get("reader_font_size").unwrap().as_deref(), Some("19"));
        assert_eq!(
            repo.get("reader_font_family").unwrap().as_deref(),
            Some("serif")
        );
        assert_eq!(
            repo.get("reader_line_height").unwrap().as_deref(),
            Some("normal")
        );
        assert_eq!(repo.get("reader_width").unwrap().as_deref(), Some("medium"));
        assert_eq!(
            repo.get("show_snippets").unwrap().as_deref(),
            Some("true")
        );
    }

    #[test]
    fn set_and_get_roundtrip() {
        let repo = setup();
        repo.set("refresh_interval_minutes", "60").unwrap();
        assert_eq!(
            repo.get("refresh_interval_minutes").unwrap().as_deref(),
            Some("60")
        );
    }

    #[test]
    fn missing_key_returns_none() {
        let repo = setup();
        assert_eq!(repo.get("no_existe").unwrap(), None);
    }
}
