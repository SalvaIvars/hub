use crate::StorageError;
use reader_domain::{Source, SourceSummary};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Arc, Mutex};

/// Puente de acceso a sources (puerto hexagonal).
pub trait SourceRepository: Send + Sync {
    fn upsert(&self, source: &Source) -> Result<i64, StorageError>;
    fn get(&self, id: i64) -> Result<Option<Source>, StorageError>;
    fn find_by_home_url(&self, home_url: &str) -> Result<Option<Source>, StorageError>;
    fn update(&self, source: &Source) -> Result<(), StorageError>;
    fn rename(&self, id: i64, title: &str) -> Result<(), StorageError>;
    fn delete(&self, id: i64) -> Result<(), StorageError>;
    fn list(&self) -> Result<Vec<SourceSummary>, StorageError>;
    fn update_last_fetched(&self, id: i64, at: &str) -> Result<(), StorageError>;
    fn update_health(&self, id: i64, status: Option<i64>, error: Option<&str>) -> Result<(), StorageError>;
    fn increment_error_count(&self, id: i64) -> Result<(), StorageError>;
    fn reset_error_count(&self, id: i64) -> Result<(), StorageError>;
    fn set_category(&self, id: i64, category: Option<&str>) -> Result<(), StorageError>;
    fn clear_category(&self, category: &str) -> Result<usize, StorageError>;
    fn list_categories(&self) -> Result<Vec<String>, StorageError>;
}

/// Adaptador concreto sobre SQLite (comparte la conexión con `ArticleRepo`).
#[derive(Clone)]
pub struct SourceRepo {
    conn: Arc<Mutex<Connection>>,
}

impl SourceRepo {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

impl SourceRepository for SourceRepo {
    fn upsert(&self, source: &Source) -> Result<i64, StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            r#"
            INSERT INTO sources
                (url, home_url, title, description, feed_url, last_fetched_at, last_error, last_status, error_count, category)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(url) DO UPDATE SET
                home_url = excluded.home_url,
                title = excluded.title,
                description = COALESCE(excluded.description, sources.description),
                feed_url = COALESCE(excluded.feed_url, sources.feed_url),
                last_fetched_at = excluded.last_fetched_at,
                last_error = excluded.last_error,
                last_status = excluded.last_status,
                error_count = excluded.error_count,
                category = excluded.category
            RETURNING id
            "#,
            params![
                source.url,
                source.home_url,
                source.title,
                source.description,
                source.feed_url,
                source.last_fetched_at,
                source.last_error,
                source.last_status,
                source.error_count,
                source.category
            ],
            |row| row.get(0),
        )
        .map_err(StorageError::Sqlite)
    }

    fn get(&self, id: i64) -> Result<Option<Source>, StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            r#"
            SELECT id, url, home_url, title, description, feed_url, last_fetched_at, last_error, last_status, error_count, category
            FROM sources WHERE id = ?1
            "#,
            params![id],
            |row| {
                Ok(Source {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    home_url: row.get(2)?,
                    title: row.get(3)?,
                    description: row.get(4)?,
                    feed_url: row.get(5)?,
                    last_fetched_at: row.get(6)?,
                    last_error: row.get(7)?,
                    last_status: row.get(8)?,
                    error_count: row.get(9)?,
                    category: row.get(10)?,
                })
            },
        )
        .optional()
        .map_err(StorageError::Sqlite)
    }

    fn find_by_home_url(&self, home_url: &str) -> Result<Option<Source>, StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            r#"
            SELECT id, url, home_url, title, description, feed_url, last_fetched_at, last_error, last_status, error_count, category
            FROM sources WHERE home_url = ?1
            "#,
            params![home_url],
            |row| {
                Ok(Source {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    home_url: row.get(2)?,
                    title: row.get(3)?,
                    description: row.get(4)?,
                    feed_url: row.get(5)?,
                    last_fetched_at: row.get(6)?,
                    last_error: row.get(7)?,
                    last_status: row.get(8)?,
                    error_count: row.get(9)?,
                    category: row.get(10)?,
                })
            },
        )
        .optional()
        .map_err(StorageError::Sqlite)
    }

    fn update(&self, source: &Source) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            r#"
            UPDATE sources SET
                url = ?1,
                home_url = ?2,
                title = ?3,
                description = ?4,
                feed_url = ?5,
                last_fetched_at = ?6,
                last_error = ?7,
                last_status = ?8,
                error_count = ?9,
                category = ?10
            WHERE id = ?11
            "#,
            params![
                source.url,
                source.home_url,
                source.title,
                source.description,
                source.feed_url,
                source.last_fetched_at,
                source.last_error,
                source.last_status,
                source.error_count,
                source.category,
                source.id
            ],
        )?;
        if affected == 0 {
            return Err(StorageError::NotFound(format!("source {}", source.id)));
        }
        Ok(())
    }

    fn rename(&self, id: i64, title: &str) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE sources SET title = ?1 WHERE id = ?2",
            params![title, id],
        )?;
        if affected == 0 {
            return Err(StorageError::NotFound(format!("source {id}")));
        }
        Ok(())
    }

    fn delete(&self, id: i64) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        // Se borran primero los artículos del source: al borrar el source ya no
        // hay artículos a los que aplicar el `ON DELETE SET NULL` (no quedan
        // "sueltos"). El trigger `articles_ad` limpia el índice FTS5 solo.
        conn.execute("DELETE FROM articles WHERE source_id = ?1", params![id])?;
        let affected = conn.execute("DELETE FROM sources WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(StorageError::NotFound(format!("source {id}")));
        }
        Ok(())
    }

    fn list(&self) -> Result<Vec<SourceSummary>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT s.id, s.url, s.home_url, s.title, s.description, s.feed_url,
                   s.last_fetched_at,
                   (SELECT COUNT(*) FROM articles a WHERE a.source_id = s.id) AS total,
                   (SELECT COUNT(*) FROM articles a
                    WHERE a.source_id = s.id AND a.read = 0) AS unread,
                   s.last_error,
                   s.error_count,
                   s.category
            FROM sources s
            ORDER BY s.title COLLATE NOCASE ASC
            "#,
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SourceSummary {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    home_url: row.get(2)?,
                    title: row.get(3)?,
                    description: row.get(4)?,
                    feed_url: row.get(5)?,
                    last_fetched_at: row.get(6)?,
                    article_count: row.get(7)?,
                    unread_count: row.get(8)?,
                    last_error: row.get(9)?,
                    error_count: row.get(10)?,
                    category: row.get(11)?,
                })
            })
            .map_err(StorageError::Sqlite)?;
        rows.collect::<Result<_, _>>().map_err(StorageError::Sqlite)
    }

    fn update_last_fetched(&self, id: i64, at: &str) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE sources SET last_fetched_at = ?1 WHERE id = ?2",
            params![at, id],
        )?;
        if affected == 0 {
            return Err(StorageError::NotFound(format!("source {id}")));
        }
        Ok(())
    }

    fn update_health(&self, id: i64, status: Option<i64>, error: Option<&str>) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE sources SET last_status = ?1, last_error = ?2 WHERE id = ?3",
            params![status, error, id],
        )?;
        if affected == 0 {
            return Err(StorageError::NotFound(format!("source {id}")));
        }
        Ok(())
    }

    fn increment_error_count(&self, id: i64) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE sources SET error_count = error_count + 1 WHERE id = ?1",
            params![id],
        )?;
        if affected == 0 {
            return Err(StorageError::NotFound(format!("source {id}")));
        }
        Ok(())
    }

    fn reset_error_count(&self, id: i64) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE sources SET error_count = 0 WHERE id = ?1",
            params![id],
        )?;
        if affected == 0 {
            return Err(StorageError::NotFound(format!("source {id}")));
        }
        Ok(())
    }

    fn set_category(&self, id: i64, category: Option<&str>) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE sources SET category = ?1 WHERE id = ?2",
            params![category, id],
        )?;
        if affected == 0 {
            return Err(StorageError::NotFound(format!("source {id}")));
        }
        Ok(())
    }

    fn clear_category(&self, category: &str) -> Result<usize, StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE sources SET category = NULL WHERE category = ?1",
            params![category],
        )?;
        Ok(affected)
    }

    fn list_categories(&self) -> Result<Vec<String>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT category FROM sources WHERE category IS NOT NULL ORDER BY category",
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut categories = Vec::new();
        for row in rows {
            categories.push(row.map_err(StorageError::Sqlite)?);
        }
        Ok(categories)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> SourceRepo {
        let conn = Arc::new(std::sync::Mutex::new(crate::open_db_in_memory().unwrap()));
        SourceRepo::new(conn)
    }

    fn source(url: &str, title: &str) -> Source {
        Source {
            id: 0,
            url: url.to_string(),
            home_url: "https://example.com".to_string(),
            title: title.to_string(),
            description: None,
            feed_url: Some(url.to_string()),
            last_fetched_at: None,
            last_error: None,
            last_status: None,
            error_count: 0,
            category: None,
        }
    }

    #[test]
    fn insert_and_get() {
        let repo = setup();
        let id = repo.upsert(&source("https://example.com/feed.xml", "Example")).unwrap();
        let got = repo.get(id).unwrap().unwrap();
        assert_eq!(got.title, "Example");
        assert_eq!(got.feed_url.as_deref(), Some("https://example.com/feed.xml"));
    }

    #[test]
    fn dedupe_by_url() {
        let repo = setup();
        repo.upsert(&source("https://example.com/feed.xml", "Original")).unwrap();
        repo.upsert(&source("https://example.com/feed.xml", "Actualizado")).unwrap();
        assert_eq!(repo.list().unwrap().len(), 1);
        let id = repo.list().unwrap()[0].id;
        assert_eq!(repo.get(id).unwrap().unwrap().title, "Actualizado");
    }

    #[test]
    fn update_last_fetched() {
        let repo = setup();
        let id = repo.upsert(&source("https://example.com/feed.xml", "Example")).unwrap();
        repo.update_last_fetched(id, "2024-01-02T00:00:00Z").unwrap();
        assert_eq!(
            repo.get(id).unwrap().unwrap().last_fetched_at.as_deref(),
            Some("2024-01-02T00:00:00Z")
        );
    }

    #[test]
    fn find_by_home_url_and_update() {
        let repo = setup();
        let mut s = source("https://example.com/feed.xml", "Example");
        s.home_url = "https://example.com".to_string();
        let id = repo.upsert(&s).unwrap();

        let found = repo.find_by_home_url("https://example.com").unwrap().unwrap();
        assert_eq!(found.id, id);

        let mut updated = found;
        updated.title = "Nuevo título".to_string();
        repo.update(&updated).unwrap();
        assert_eq!(repo.get(id).unwrap().unwrap().title, "Nuevo título");
    }

    #[test]
    fn rename_changes_title() {
        let repo = setup();
        let id = repo.upsert(&source("https://example.com/feed.xml", "Example")).unwrap();
        repo.rename(id, "Renombrado").unwrap();
        assert_eq!(repo.get(id).unwrap().unwrap().title, "Renombrado");
    }

    #[test]
    fn rename_missing_errors() {
        let repo = setup();
        assert!(matches!(
            repo.rename(999, "X"),
            Err(StorageError::NotFound(_))
        ));
    }

    #[test]
    fn delete_removes_source() {
        let repo = setup();
        let id = repo.upsert(&source("https://example.com/feed.xml", "Example")).unwrap();
        repo.delete(id).unwrap();
        assert!(repo.get(id).unwrap().is_none());
        assert!(repo.list().unwrap().is_empty());
    }

    #[test]
    fn delete_missing_errors() {
        let repo = setup();
        assert!(matches!(
            repo.delete(999),
            Err(StorageError::NotFound(_))
        ));
    }

    #[test]
    fn clear_category_removes_category_but_keeps_sources() {
        let repo = setup();
        let a = repo.upsert(&source("https://a.com/feed.xml", "A")).unwrap();
        let b = repo.upsert(&source("https://b.com/feed.xml", "B")).unwrap();
        repo.set_category(a, Some("Tecnología")).unwrap();
        repo.set_category(b, Some("Tecnología")).unwrap();

        let changed = repo.clear_category("Tecnología").unwrap();
        assert_eq!(changed, 2);
        assert_eq!(repo.list_categories().unwrap(), Vec::<String>::new());
        assert!(repo.get(a).unwrap().unwrap().category.is_none());
        assert!(repo.get(b).unwrap().unwrap().category.is_none());

        // Borrar una categoría inexistente no toca nada.
        assert_eq!(repo.clear_category("Nada").unwrap(), 0);
    }

    #[test]
    fn delete_deletes_articles_of_source() {
        use crate::article_repo::ArticleRepository;
        let repo = setup();
        let id = repo.upsert(&source("https://example.com/feed.xml", "Example")).unwrap();
        let conn = repo.conn.clone();
        let articles = crate::article_repo::ArticleRepo::new(conn);

        let mut a = reader_domain::Article {
            id: 0,
            source_id: Some(id),
            url: "https://example.com/1".into(),
            title: "uno".into(),
            html: "<p>uno</p>".into(),
            text: "uno".into(),
            raw_html: String::new(),
            byline: None,
            site_name: None,
            published_at: None,
            fetched_at: "2024-01-02T00:00:00Z".into(),
            read: false,
            starred: false,
            has_embedding: false,
        };
        articles.upsert(&a).unwrap();
        a.url = "https://example.com/2".into();
        a.title = "dos".into();
        articles.upsert(&a).unwrap();

        assert_eq!(articles.list_all().unwrap().len(), 2);
        repo.delete(id).unwrap();
        assert!(articles.list_all().unwrap().is_empty());
        assert!(articles.list_unassigned().unwrap().is_empty());
    }

    #[test]
    fn update_health_sets_error_and_status() {
        let repo = setup();
        let id = repo.upsert(&source("https://example.com/feed.xml", "Example")).unwrap();
        repo.update_health(id, Some(500), Some("Internal Server Error")).unwrap();
        let got = repo.get(id).unwrap().unwrap();
        assert_eq!(got.last_status, Some(500));
        assert_eq!(got.last_error.as_deref(), Some("Internal Server Error"));
    }

    #[test]
    fn increment_and_reset_error_count() {
        let repo = setup();
        let id = repo.upsert(&source("https://example.com/feed.xml", "Example")).unwrap();
        assert_eq!(repo.get(id).unwrap().unwrap().error_count, 0);
        repo.increment_error_count(id).unwrap();
        assert_eq!(repo.get(id).unwrap().unwrap().error_count, 1);
        repo.increment_error_count(id).unwrap();
        assert_eq!(repo.get(id).unwrap().unwrap().error_count, 2);
        repo.reset_error_count(id).unwrap();
        assert_eq!(repo.get(id).unwrap().unwrap().error_count, 0);
    }
}
