//! Persistencia en SQLite local con búsqueda full-text FTS5.
//!
//! Define los puertos (traits) `ArticleRepository` y `SourceRepository` y sus
//! adaptadores concretos sobre `rusqlite`. Los traits permiten al crate
//! `pipeline` testearse con implementaciones mock.

mod article_repo;
mod embedding_repo;
mod settings;
mod smart_feed_repo;
mod source_repo;

pub use article_repo::{ArticleRepo, ArticleRepository};
pub use embedding_repo::{EmbeddingRepo, EmbeddingRepository};
pub use settings::{SettingsRepo, SettingsRepository};
pub use smart_feed_repo::{SmartFeedRepo, SmartFeedRepository};
pub use source_repo::{SourceRepo, SourceRepository};

use rusqlite::Connection;
use std::path::Path;
use std::sync::Once;

/// Registra la extensión `sqlite-vec` en el proceso (una sola vez).
///
/// Usa `sqlite3_auto_extension`, de modo que toda conexión SQLite abierta
/// después de este punto conoce el módulo virtual `vec0`. Debe llamarse ANTES
/// de abrir cualquier conexión.
fn register_sqlite_vec() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        use rusqlite::ffi::sqlite3_auto_extension;
        unsafe {
            sqlite3_auto_extension(Some(
                std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ()),
            ));
        }
    });
}

/// Convierte un vector `f32` en su representación binaria compacta que espera
/// `sqlite-vec` (floats little-endian concatenados).
pub fn embedding_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// Convierte un BLOB de `sqlite-vec` de vuelta a un vector `f32`.
pub fn blob_to_embedding(blob: &[u8]) -> Result<Vec<f32>, StorageError> {
    if blob.len() % 4 != 0 {
        return Err(StorageError::Invalid(format!(
            "BLOB de embedding con tamaño inválido: {} bytes",
            blob.len()
        )));
    }
    let mut out = Vec::with_capacity(blob.len() / 4);
    for chunk in blob.chunks_exact(4) {
        let bytes: [u8; 4] = chunk.try_into().unwrap();
        out.push(f32::from_le_bytes(bytes));
    }
    Ok(out)
}

/// Error tipado de la capa de storage.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("error de base de datos: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("registro no encontrado: {0}")]
    NotFound(String),
    #[error("operación no válida: {0}")]
    Invalid(String),
}

/// Abre (creando si no existe) la base de datos y aplica las migraciones.
pub fn open_db(path: &Path) -> Result<Connection, StorageError> {
    register_sqlite_vec();
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

/// Abre una base en memoria (para tests) y aplica las migraciones.
pub fn open_db_in_memory() -> Result<Connection, StorageError> {
    register_sqlite_vec();
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

/// Aplica las migraciones de forma idempotente, usando `PRAGMA user_version`.
pub fn migrate(conn: &Connection) -> Result<(), StorageError> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sources (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                url TEXT NOT NULL UNIQUE,
                home_url TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT,
                feed_url TEXT,
                last_fetched_at TEXT
            );

            CREATE TABLE IF NOT EXISTS articles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_id INTEGER REFERENCES sources(id) ON DELETE SET NULL,
                url TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                html TEXT NOT NULL,
                text TEXT NOT NULL,
                raw_html TEXT NOT NULL,
                byline TEXT,
                site_name TEXT,
                published_at TEXT,
                fetched_at TEXT NOT NULL,
                read INTEGER NOT NULL DEFAULT 0,
                starred INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_articles_source ON articles(source_id);
            CREATE INDEX IF NOT EXISTS idx_articles_published ON articles(published_at);

            CREATE VIRTUAL TABLE IF NOT EXISTS articles_fts USING fts5(
                title,
                text,
                content='articles',
                content_rowid='id'
            );

            CREATE TRIGGER IF NOT EXISTS articles_ai AFTER INSERT ON articles BEGIN
                INSERT INTO articles_fts(rowid, title, text) VALUES (new.id, new.title, new.text);
            END;

            CREATE TRIGGER IF NOT EXISTS articles_ad AFTER DELETE ON articles BEGIN
                INSERT INTO articles_fts(articles_fts, rowid, title, text)
                VALUES('delete', old.id, old.title, old.text);
            END;

            CREATE TRIGGER IF NOT EXISTS articles_au AFTER UPDATE ON articles BEGIN
                INSERT INTO articles_fts(articles_fts, rowid, title, text)
                VALUES('delete', old.id, old.title, old.text);
                INSERT INTO articles_fts(rowid, title, text) VALUES (new.id, new.title, new.text);
            END;
            "#,
        )?;
        conn.pragma_update(None, "user_version", 1)?;
    }
    if version < 2 {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            INSERT OR IGNORE INTO settings (key, value)
                VALUES ('refresh_interval_minutes', '30');
            "#,
        )?;
        conn.pragma_update(None, "user_version", 2)?;
    }
    if version < 3 {
        conn.execute_batch(
            r#"
            ALTER TABLE sources ADD COLUMN last_error TEXT;
            ALTER TABLE sources ADD COLUMN last_status INTEGER;
            ALTER TABLE sources ADD COLUMN error_count INTEGER NOT NULL DEFAULT 0;
            "#,
        )?;
        conn.pragma_update(None, "user_version", 3)?;
    }
    if version < 4 {
        conn.execute_batch(
            r#"
            ALTER TABLE sources ADD COLUMN category TEXT;
            "#,
        )?;
        conn.pragma_update(None, "user_version", 4)?;
    }
    if version < 5 {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS smart_feeds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                query TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            "#,
        )?;
        conn.pragma_update(None, "user_version", 5)?;
    }
    if version < 6 {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS article_embeddings (
                article_id INTEGER PRIMARY KEY REFERENCES articles(id) ON DELETE CASCADE,
                embedding BLOB NOT NULL,
                model TEXT NOT NULL,
                tokens_used INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS vec_articles USING vec0(
                article_id INTEGER PRIMARY KEY,
                embedding float[384]
            );

            ALTER TABLE smart_feeds ADD COLUMN search_mode TEXT NOT NULL DEFAULT 'bm25';

            CREATE TRIGGER IF NOT EXISTS article_embeddings_ad AFTER DELETE ON articles BEGIN
                DELETE FROM article_embeddings WHERE article_id = old.id;
                DELETE FROM vec_articles WHERE article_id = old.id;
            END;
            "#,
        )?;
        conn.pragma_update(None, "user_version", 6)?;
    }
    if version < 7 {
        // La búsqueda vectorial filtra por similitud coseno, así que la tabla
        // vec0 debe usar la métrica cosine (`distance = 1 - cos_sim`, en [0,2]).
        // Se recrea y se repuebla desde `article_embeddings` (para bases ya
        // existentes creadas con la métrica L2 por defecto).
        conn.execute_batch(
            r#"
            DROP TABLE IF EXISTS vec_articles;
            CREATE VIRTUAL TABLE vec_articles USING vec0(
                article_id INTEGER PRIMARY KEY,
                embedding float[384] distance_metric=cosine
            );
            INSERT OR IGNORE INTO vec_articles (article_id, embedding)
                SELECT article_id, embedding FROM article_embeddings;

            INSERT OR IGNORE INTO settings (key, value)
                VALUES ('vector_similarity_threshold', '0.7');
            "#,
        )?;
        conn.pragma_update(None, "user_version", 7)?;
    }
    if version < 8 {
        // Ajustes de apariencia y lectura del panel de configuración (vistas en
        // el modal de "Configuración"). El tema ("system"/"light"/"dark"/"sepia")
        // pasa aquí del localStorage del frontend para quedar persistido en la DB.
        conn.execute_batch(
            r#"
            INSERT OR IGNORE INTO settings (key, value) VALUES
                ('theme', 'system'),
                ('reader_font_size', '19'),
                ('reader_font_family', 'serif'),
                ('reader_line_height', 'normal'),
                ('reader_width', 'medium'),
                ('show_snippets', 'true');
            "#,
        )?;
        conn.pragma_update(None, "user_version", 8)?;
    }
    Ok(())
}

/// Convierte una consulta libre del usuario en una consulta FTS5 segura:
/// cada término se cita y se le aplica prefijo, unidos por AND.
pub fn to_fts_query(raw: &str) -> String {
    let terms: Vec<String> = raw
        .split_whitespace()
        .map(|t| format!("\"{}\"*", t.trim_matches('"')))
        .collect();
    if terms.is_empty() {
        raw.to_string()
    } else {
        terms.join(" AND ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_is_idempotent() {
        let conn = open_db_in_memory().unwrap();
        migrate(&conn).unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(tables.iter().any(|t| t == "articles"));
        assert!(tables.iter().any(|t| t == "sources"));
        assert!(tables.iter().any(|t| t == "articles_fts"));
        assert!(tables.iter().any(|t| t == "settings"));
        assert!(tables.iter().any(|t| t == "article_embeddings"));
        assert!(tables.iter().any(|t| t == "vec_articles"));
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 8);
        // La tabla vec0 usa la métrica cosine (necesaria para el umbral de similitud).
        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='vec_articles'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            sql.to_lowercase().contains("distance_metric=cosine"),
            "vec_articles debe usar distance_metric=cosine: {sql}"
        );
    }

    #[test]
    fn fts_query_quotes_terms() {
        assert_eq!(to_fts_query("rust async"), "\"rust\"* AND \"async\"*");
        assert_eq!(to_fts_query(""), "");
        assert_eq!(to_fts_query("  spaced  out  "), "\"spaced\"* AND \"out\"*");
    }

    #[test]
    fn open_db_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let conn = open_db(&path).unwrap();
        let value: i64 = conn.query_row("SELECT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(value, 1);
        assert!(path.exists());
    }
}
