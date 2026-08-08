use crate::StorageError;
use reader_domain::{Article, ArticleSummary, FeedEntry, ReadScope};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Arc, Mutex};

/// Puente de acceso a artículos (puerto hexagonal).
pub trait ArticleRepository: Send + Sync {
    fn upsert(&self, article: &Article) -> Result<i64, StorageError>;
    fn get(&self, id: i64) -> Result<Option<Article>, StorageError>;
    fn list_all(&self) -> Result<Vec<ArticleSummary>, StorageError>;
    fn list_by_source(&self, source_id: i64) -> Result<Vec<ArticleSummary>, StorageError>;
    fn list_by_category(&self, category: &str) -> Result<Vec<ArticleSummary>, StorageError>;
    fn list_unassigned(&self) -> Result<Vec<ArticleSummary>, StorageError>;
    fn list_unread(&self) -> Result<Vec<ArticleSummary>, StorageError>;
    fn list_starred(&self) -> Result<Vec<ArticleSummary>, StorageError>;
    fn list_recent(&self, days: i64) -> Result<Vec<ArticleSummary>, StorageError>;
    fn search(&self, query: &str) -> Result<Vec<ArticleSummary>, StorageError>;
    fn mark_read(&self, id: i64, read: bool) -> Result<(), StorageError>;
    fn mark_all_read(&self, scope: &ReadScope) -> Result<usize, StorageError>;
    /// Marca como leídos todos los ids dados (para alcances que requieren
    /// cálculo previo, p. ej. los resultados de una búsqueda vectorial).
    fn mark_read_ids(&self, ids: &[i64]) -> Result<usize, StorageError>;
    fn toggle_star(&self, id: i64) -> Result<(), StorageError>;
    fn delete(&self, id: i64) -> Result<(), StorageError>;
    fn insert_feed_entry(
        &self,
        source_id: i64,
        entry: &FeedEntry,
        fetched_at: &str,
    ) -> Result<Option<i64>, StorageError>;
    fn count_by_source(&self, source_id: i64) -> Result<(i64, i64), StorageError>;
    /// Vacía el contenido extraído (`html`, `text`, `raw_html`) de los
    /// artículos de feed ya leídos, volviéndolos a su resumen original
    /// (columna `summary`). También borra sus embeddings. Con `days > 0` solo
    /// se purgan los artículos cuyo `fetched_at` sea anterior a `days`.
    /// Devuelve el nº de artículos purgados.
    fn purge_extracted_content(&self, days: i64) -> Result<usize, StorageError>;
}

/// Adaptador concreto sobre SQLite.
///
/// Comparte un `Arc<Mutex<Connection>>` con `SourceRepo`. El mutex se
/// adquiere y libera dentro de cada método, de modo que el pipeline nunca lo
/// mantiene cruzando un `.await`.
#[derive(Clone)]
pub struct ArticleRepo {
    conn: Arc<Mutex<Connection>>,
}

impl ArticleRepo {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

impl ArticleRepository for ArticleRepo {
    fn upsert(&self, article: &Article) -> Result<i64, StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            r#"
            INSERT INTO articles
                (source_id, url, title, html, text, raw_html, byline, site_name,
                 published_at, fetched_at, read, starred)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(url) DO UPDATE SET
                source_id = COALESCE(excluded.source_id, articles.source_id),
                title = excluded.title,
                html = excluded.html,
                text = excluded.text,
                raw_html = excluded.raw_html,
                byline = excluded.byline,
                site_name = excluded.site_name,
                published_at = COALESCE(excluded.published_at, articles.published_at),
                fetched_at = excluded.fetched_at
            RETURNING id
            "#,
            params![
                article.source_id,
                article.url,
                article.title,
                article.html,
                article.text,
                article.raw_html,
                article.byline,
                article.site_name,
                article.published_at,
                article.fetched_at,
                article.read,
                article.starred
            ],
            |row| row.get(0),
        )
        .map_err(StorageError::Sqlite)
    }

    fn get(&self, id: i64) -> Result<Option<Article>, StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            r#"
            SELECT a.id, a.source_id, a.url, a.title, a.html, a.text, a.raw_html,
                   a.byline, a.site_name, a.published_at, a.fetched_at, a.read,
                   a.starred,
                   CASE WHEN e.article_id IS NOT NULL THEN 1 ELSE 0 END
            FROM articles a
            LEFT JOIN article_embeddings e ON e.article_id = a.id
            WHERE a.id = ?1
            "#,
            params![id],
            row_to_article,
        )
        .optional()
        .map_err(StorageError::Sqlite)
    }

    fn list_all(&self) -> Result<Vec<ArticleSummary>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("{}{}", summary_query(), summary_order());
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], row_to_summary)
            .map_err(StorageError::Sqlite)?;
        rows.collect::<Result<_, _>>().map_err(StorageError::Sqlite)
    }

    fn list_by_source(&self, source_id: i64) -> Result<Vec<ArticleSummary>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "{} WHERE a.source_id = ?1{}",
            summary_query(),
            summary_order()
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![source_id], row_to_summary)
            .map_err(StorageError::Sqlite)?;
        rows.collect::<Result<_, _>>().map_err(StorageError::Sqlite)
    }

    fn list_by_category(&self, category: &str) -> Result<Vec<ArticleSummary>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "{} WHERE s.category = ?1{}",
            summary_query(),
            summary_order()
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![category], row_to_summary)
            .map_err(StorageError::Sqlite)?;
        rows.collect::<Result<_, _>>().map_err(StorageError::Sqlite)
    }

    fn list_unassigned(&self) -> Result<Vec<ArticleSummary>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "{} WHERE a.source_id IS NULL{}",
            summary_query(),
            summary_order()
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], row_to_summary)
            .map_err(StorageError::Sqlite)?;
        rows.collect::<Result<_, _>>().map_err(StorageError::Sqlite)
    }

    fn list_unread(&self) -> Result<Vec<ArticleSummary>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("{} WHERE a.read = 0{}", summary_query(), summary_order());
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], row_to_summary)
            .map_err(StorageError::Sqlite)?;
        rows.collect::<Result<_, _>>().map_err(StorageError::Sqlite)
    }

    fn list_starred(&self) -> Result<Vec<ArticleSummary>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("{} WHERE a.starred = 1{}", summary_query(), summary_order());
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], row_to_summary)
            .map_err(StorageError::Sqlite)?;
        rows.collect::<Result<_, _>>().map_err(StorageError::Sqlite)
    }

    fn list_recent(&self, days: i64) -> Result<Vec<ArticleSummary>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "{} WHERE COALESCE(a.published_at, a.fetched_at) >= datetime('now', '-' || ?1 || ' days'){}",
            summary_query(),
            summary_order()
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![days], row_to_summary)
            .map_err(StorageError::Sqlite)?;
        rows.collect::<Result<_, _>>().map_err(StorageError::Sqlite)
    }

    fn search(&self, query: &str) -> Result<Vec<ArticleSummary>, StorageError> {
        let fts = crate::to_fts_query(query);
        if fts.trim().is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        let sql = r#"
            SELECT a.id, a.source_id, s.title, a.url, a.title, a.site_name,
                   a.published_at, a.fetched_at, a.read, a.starred,
                   CASE WHEN e.article_id IS NOT NULL THEN 1 ELSE 0 END,
                   snippet(articles_fts, 1, '<mark>', '</mark>', '…', 12) AS snip
            FROM articles_fts fts
            JOIN articles a ON a.id = fts.rowid
            LEFT JOIN sources s ON s.id = a.source_id
            LEFT JOIN article_embeddings e ON e.article_id = a.id
            WHERE articles_fts MATCH ?1
            ORDER BY bm25(articles_fts)
        "#;
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map(params![fts], |row| {
                let mut summary = row_to_summary(row)?;
                summary.snippet = row.get(11)?;
                Ok(summary)
            })
            .map_err(StorageError::Sqlite)?;
        rows.collect::<Result<_, _>>().map_err(StorageError::Sqlite)
    }

    fn mark_read(&self, id: i64, read: bool) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE articles SET read = ?1 WHERE id = ?2",
            params![read, id],
        )?;
        if affected == 0 {
            return Err(StorageError::NotFound(format!("artículo {id}")));
        }
        Ok(())
    }

    fn mark_all_read(&self, scope: &ReadScope) -> Result<usize, StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = match scope {
            ReadScope::All => conn.execute("UPDATE articles SET read = 1 WHERE read = 0", [])?,
            ReadScope::Source { id } => conn.execute(
                "UPDATE articles SET read = 1 WHERE source_id = ?1 AND read = 0",
                params![id],
            )?,
            ReadScope::Category { name } => conn.execute(
                "UPDATE articles SET read = 1 WHERE source_id IN (SELECT id FROM sources WHERE category = ?1) AND read = 0",
                params![name],
            )?,
            ReadScope::SmartFeed { id } => {
                let query: String = conn.query_row(
                    "SELECT query FROM smart_feeds WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )?;
                let fts = crate::to_fts_query(&query);
                if fts.trim().is_empty() {
                    return Ok(0);
                }
                conn.execute(
                    "UPDATE articles SET read = 1 WHERE id IN (SELECT rowid FROM articles_fts WHERE articles_fts MATCH ?1) AND read = 0",
                    params![fts],
                )?
            }
        };
        Ok(affected)
    }

    fn mark_read_ids(&self, ids: &[i64]) -> Result<usize, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut affected = 0usize;
        for chunk in ids.chunks(500) {
            let placeholders = vec!["?1"; chunk.len()].join(", ");
            let sql = format!(
                "UPDATE articles SET read = 1 WHERE id IN ({placeholders}) AND read = 0"
            );
            let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
            for id in chunk {
                params.push(id);
            }
            affected += conn.execute(&sql, rusqlite::params_from_iter(params.iter()))?;
        }
        Ok(affected)
    }

    fn toggle_star(&self, id: i64) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE articles SET starred = NOT starred WHERE id = ?1",
            params![id],
        )?;
        if affected == 0 {
            return Err(StorageError::NotFound(format!("artículo {id}")));
        }
        Ok(())
    }

    fn delete(&self, id: i64) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute("DELETE FROM articles WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(StorageError::NotFound(format!("artículo {id}")));
        }
        Ok(())
    }

    fn insert_feed_entry(
        &self,
        source_id: i64,
        entry: &FeedEntry,
        fetched_at: &str,
    ) -> Result<Option<i64>, StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT OR IGNORE INTO articles
                (source_id, url, title, html, text, raw_html, summary, byline, site_name,
                 published_at, fetched_at, read, starred)
            VALUES (?1, ?2, ?3, '', ?4, '', ?4, NULL, NULL, ?5, ?6, 0, 0)
            "#,
            params![
                source_id,
                entry.link,
                entry.title,
                entry.summary.clone().unwrap_or_default(),
                entry.published,
                fetched_at
            ],
        )?;
        let inserted = conn.changes();
        let id: i64 = conn.query_row(
            "SELECT id FROM articles WHERE url = ?1",
            params![entry.link],
            |row| row.get(0),
        )?;
        Ok(if inserted > 0 { Some(id) } else { None })
    }

    fn count_by_source(&self, source_id: i64) -> Result<(i64, i64), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            r#"
            SELECT COUNT(*), COALESCE(SUM(CASE WHEN read = 0 THEN 1 ELSE 0 END), 0)
            FROM articles WHERE source_id = ?1
            "#,
            params![source_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(StorageError::Sqlite)
    }

    fn purge_extracted_content(&self, days: i64) -> Result<usize, StorageError> {
        let conn = self.conn.lock().unwrap();
        // Candidatos: artículos de feed (no sueltos), leídos, con contenido
        // extraído, y (si `days > 0`) con `fetched_at` anterior a `days`.
        let age = if days > 0 {
            " AND fetched_at < datetime('now', '-' || ?1 || ' days')"
        } else {
            ""
        };
        let where_clause = format!("source_id IS NOT NULL AND read = 1 AND html != ''{age}");

        // Una sola transacción: embeddings, tabla vectorial y artículo.
        let tx = conn.unchecked_transaction().map_err(StorageError::Sqlite)?;
        if days > 0 {
            tx.execute(
                &format!(
                    "DELETE FROM article_embeddings WHERE article_id IN (SELECT id FROM articles WHERE {where_clause})"
                ),
                params![days],
            )?;
            tx.execute(
                &format!(
                    "DELETE FROM vec_articles WHERE article_id IN (SELECT id FROM articles WHERE {where_clause})"
                ),
                params![days],
            )?;
            tx.execute(
                &format!(
                    "UPDATE articles SET html = '', text = COALESCE(NULLIF(summary, ''), ''), raw_html = '' WHERE {where_clause}"
                ),
                params![days],
            )?;
        } else {
            tx.execute(
                &format!(
                    "DELETE FROM article_embeddings WHERE article_id IN (SELECT id FROM articles WHERE {where_clause})"
                ),
                [],
            )?;
            tx.execute(
                &format!(
                    "DELETE FROM vec_articles WHERE article_id IN (SELECT id FROM articles WHERE {where_clause})"
                ),
                [],
            )?;
            tx.execute(
                &format!(
                    "UPDATE articles SET html = '', text = COALESCE(NULLIF(summary, ''), ''), raw_html = '' WHERE {where_clause}"
                ),
                [],
            )?;
        }
        let changed = tx.changes() as usize;
        tx.commit().map_err(StorageError::Sqlite)?;
        Ok(changed)
    }
}

fn summary_query() -> &'static str {
    r#"
    SELECT a.id, a.source_id, s.title, a.url, a.title, a.site_name,
           a.published_at, a.fetched_at, a.read, a.starred,
           CASE WHEN e.article_id IS NOT NULL THEN 1 ELSE 0 END
    FROM articles a
    LEFT JOIN sources s ON s.id = a.source_id
    LEFT JOIN article_embeddings e ON e.article_id = a.id
    "#
}

fn summary_order() -> &'static str {
    " ORDER BY COALESCE(a.published_at, a.fetched_at) DESC"
}

fn row_to_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArticleSummary> {
    Ok(ArticleSummary {
        id: row.get(0)?,
        source_id: row.get(1)?,
        source_title: row.get(2)?,
        url: row.get(3)?,
        title: row.get(4)?,
        site_name: row.get(5)?,
        published_at: row.get(6)?,
        fetched_at: row.get(7)?,
        read: row.get(8)?,
        starred: row.get(9)?,
        snippet: None,
        has_embedding: row.get(10)?,
    })
}

fn row_to_article(row: &rusqlite::Row<'_>) -> rusqlite::Result<Article> {
    Ok(Article {
        id: row.get(0)?,
        source_id: row.get(1)?,
        url: row.get(2)?,
        title: row.get(3)?,
        html: row.get(4)?,
        text: row.get(5)?,
        raw_html: row.get(6)?,
        byline: row.get(7)?,
        site_name: row.get(8)?,
        published_at: row.get(9)?,
        fetched_at: row.get(10)?,
        read: row.get(11)?,
        starred: row.get(12)?,
        has_embedding: row.get(13)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> ArticleRepo {
        let conn = Arc::new(std::sync::Mutex::new(crate::open_db_in_memory().unwrap()));
        // Fuentes de ejemplo para que la clave foránea de los tests sea válida.
        conn.lock()
            .unwrap()
            .execute_batch(
                r#"
                INSERT INTO sources (id, url, home_url, title)
                VALUES (1, 'https://a.com/feed', 'https://a.com', 'A');
                INSERT INTO sources (id, url, home_url, title)
                VALUES (5, 'https://b.com/feed', 'https://b.com', 'B');
                "#,
            )
            .unwrap();
        ArticleRepo::new(conn)
    }

    fn article(url: &str, title: &str) -> Article {
        Article {
            id: 0,
            source_id: None,
            url: url.to_string(),
            title: title.to_string(),
            html: format!("<p>{title}</p>"),
            text: format!("{title} cuerpo de prueba"),
            raw_html: "<html><body>raw</body></html>".to_string(),
            byline: None,
            site_name: Some("Example".to_string()),
            published_at: Some("2024-01-01T00:00:00Z".to_string()),
            fetched_at: "2024-01-02T00:00:00Z".to_string(),
            read: false,
            starred: false,
            has_embedding: false,
        }
    }

    #[test]
    fn insert_and_get_roundtrip() {
        let repo = setup();
        let id = repo.upsert(&article("https://a.com/1", "Título uno")).unwrap();
        let got = repo.get(id).unwrap().unwrap();
        assert_eq!(got.url, "https://a.com/1");
        assert_eq!(got.title, "Título uno");
        assert_eq!(got.html, "<p>Título uno</p>");
        assert!(!got.has_embedding);
    }

    #[test]
    fn has_embedding_reflects_embedding_repo() {
        use crate::embedding_repo::{EmbeddingRepo, EmbeddingRepository};

        let conn = Arc::new(std::sync::Mutex::new(crate::open_db_in_memory().unwrap()));
        let articles = ArticleRepo::new(conn.clone());
        let embeddings = EmbeddingRepo::new(conn);

        let id = articles.upsert(&article("https://a.com/1", "Título")).unwrap();
        // Sin embedding: listado y detalle lo reflejan.
        assert!(!articles.list_all().unwrap()[0].has_embedding);
        assert!(!articles.get(id).unwrap().unwrap().has_embedding);

        // Con embedding: ambos lo reflejan.
        embeddings.upsert(id, &vec![0.1; 384], "m", 1, "t").unwrap();
        assert!(articles.list_all().unwrap()[0].has_embedding);
        assert!(articles.get(id).unwrap().unwrap().has_embedding);

        // Al borrar el embedding vuelve a false.
        embeddings.delete(id).unwrap();
        assert!(!articles.list_all().unwrap()[0].has_embedding);
        assert!(!articles.get(id).unwrap().unwrap().has_embedding);
    }

    #[test]
    fn dedupe_by_url_updates_not_duplicates() {
        let repo = setup();
        repo.upsert(&article("https://a.com/dup", "Original")).unwrap();
        repo.upsert(&article("https://a.com/dup", "Actualizado")).unwrap();
        let all = repo.list_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].title, "Actualizado");
    }

    #[test]
    fn fts_search_finds_by_text_and_title() {
        let repo = setup();
        let mut rust = article("https://a.com/1", "Rust async in depth");
        rust.text = "Rust es un lenguaje de sistemas que amamos".to_string();
        repo.upsert(&rust).unwrap();
        repo.upsert(&article("https://a.com/2", "Otro tema")).unwrap();

        let results = repo.search("rust").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].title.contains("Rust"));
        assert!(results[0].snippet.is_some());

        let results = repo.search("Otro").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_empty_query_returns_nothing() {
        let repo = setup();
        repo.upsert(&article("https://a.com/1", "X")).unwrap();
        assert!(repo.search("").unwrap().is_empty());
    }

    #[test]
    fn mark_read_toggle_star_and_delete() {
        let repo = setup();
        let id = repo.upsert(&article("https://a.com/1", "X")).unwrap();

        repo.mark_read(id, true).unwrap();
        assert!(repo.get(id).unwrap().unwrap().read);

        repo.toggle_star(id).unwrap();
        assert!(repo.get(id).unwrap().unwrap().starred);

        repo.delete(id).unwrap();
        assert!(repo.get(id).unwrap().is_none());
    }

    #[test]
    fn mark_all_read_by_source() {
        let repo = setup();
        let a = repo.upsert(&Article {
            source_id: Some(1),
            ..article("https://a.com/1", "uno")
        }).unwrap();
        let b = repo.upsert(&Article {
            source_id: Some(1),
            ..article("https://a.com/2", "dos")
        }).unwrap();
        let c = repo.upsert(&Article {
            source_id: Some(5),
            ..article("https://b.com/3", "tres")
        }).unwrap();

        let changed = repo.mark_all_read(&ReadScope::Source { id: 1 }).unwrap();
        assert_eq!(changed, 2);
        assert!(repo.get(a).unwrap().unwrap().read);
        assert!(repo.get(b).unwrap().unwrap().read);
        assert!(!repo.get(c).unwrap().unwrap().read);

        // Idempotente: marcar de nuevo no cambia nada.
        assert_eq!(repo.mark_all_read(&ReadScope::Source { id: 1 }).unwrap(), 0);
    }

    #[test]
    fn mark_all_read_everything() {
        let repo = setup();
        repo.upsert(&Article {
            source_id: Some(1),
            ..article("https://a.com/1", "uno")
        }).unwrap();
        repo.upsert(&Article {
            source_id: None,
            ..article("https://suelto.com/2", "suelto")
        }).unwrap();

        let changed = repo.mark_all_read(&ReadScope::All).unwrap();
        assert_eq!(changed, 2);
        assert!(repo.list_all().unwrap().iter().all(|a| a.read));
    }

    #[test]
    fn mark_all_read_by_category() {
        use crate::source_repo::SourceRepository;
        let repo = setup();
        let sources = crate::source_repo::SourceRepo::new(repo.conn.clone());
        sources.set_category(1, Some("Tecnología")).unwrap();
        sources.set_category(5, Some("Noticias")).unwrap();

        let tech = repo.upsert(&Article {
            source_id: Some(1),
            ..article("https://a.com/1", "tec")
        }).unwrap();
        let news = repo.upsert(&Article {
            source_id: Some(5),
            ..article("https://b.com/2", "news")
        }).unwrap();
        repo.upsert(&Article {
            source_id: None,
            ..article("https://suelto.com/3", "suelto")
        }).unwrap();

        let changed = repo.mark_all_read(&ReadScope::Category { name: "Tecnología".into() }).unwrap();
        assert_eq!(changed, 1);
        assert!(repo.get(tech).unwrap().unwrap().read);
        assert!(!repo.get(news).unwrap().unwrap().read);
    }

    #[test]
    fn mark_all_read_by_smart_feed() {
        use crate::smart_feed_repo::SmartFeedRepository;
        let repo = setup();
        let smart = crate::smart_feed_repo::SmartFeedRepo::new(repo.conn.clone());
        let sf_id = smart.create("Rust", "rust async", "bm25", "2024-01-01T00:00:00Z").unwrap();

        let mut matching = article("https://a.com/1", "Rust async en acción");
        matching.text = "Rust async en acción".to_string();
        let matching_id = repo.upsert(&matching).unwrap();
        let other = repo.upsert(&Article {
            source_id: Some(1),
            ..article("https://a.com/2", "Python fácil")
        }).unwrap();

        let changed = repo.mark_all_read(&ReadScope::SmartFeed { id: sf_id }).unwrap();
        assert_eq!(changed, 1);
        assert!(repo.get(matching_id).unwrap().unwrap().read);
        assert!(!repo.get(other).unwrap().unwrap().read);
    }

    #[test]
    fn list_by_category_filters_articles() {
        use crate::source_repo::SourceRepository;
        let repo = setup();
        let sources = crate::source_repo::SourceRepo::new(repo.conn.clone());
        sources.set_category(1, Some("Tecnología")).unwrap();
        sources.set_category(5, Some("Noticias")).unwrap();

        repo.upsert(&Article {
            source_id: Some(1),
            ..article("https://a.com/1", "uno")
        }).unwrap();
        repo.upsert(&Article {
            source_id: Some(5),
            ..article("https://b.com/2", "dos")
        }).unwrap();
        repo.upsert(&Article {
            source_id: None,
            ..article("https://suelto.com/3", "suelto")
        }).unwrap();

        let tech = repo.list_by_category("Tecnología").unwrap();
        assert_eq!(tech.len(), 1);
        assert_eq!(tech[0].title, "uno");

        let empty = repo.list_by_category("Inexistente").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn deleting_source_deletes_its_articles() {
        use crate::source_repo::SourceRepository;
        let repo = setup();
        let id = repo.upsert(&Article {
            source_id: Some(1),
            ..article("https://a.com/1", "de fuente")
        }).unwrap();
        assert_eq!(repo.list_all().unwrap().len(), 1);

        let sources = crate::source_repo::SourceRepo::new(repo.conn.clone());
        sources.delete(1).unwrap();

        assert!(repo.get(id).unwrap().is_none());
        assert!(repo.list_all().unwrap().is_empty());
        assert!(repo.list_unassigned().unwrap().is_empty());
    }

    #[test]
    fn mark_read_missing_errors() {
        let repo = setup();
        assert!(matches!(
            repo.mark_read(999, true),
            Err(StorageError::NotFound(_))
        ));
    }

    #[test]
    fn insert_feed_entry_dedupes() {
        let repo = setup();
        let entry = FeedEntry {
            title: "Post".into(),
            link: "https://a.com/post".into(),
            summary: Some("resumen".into()),
            published: Some("2024-01-01T00:00:00Z".into()),
        };
        let first = repo
            .insert_feed_entry(1, &entry, "2024-01-02T00:00:00Z")
            .unwrap();
        let second = repo
            .insert_feed_entry(1, &entry, "2024-01-02T00:00:00Z")
            .unwrap();
        assert!(first.is_some());
        assert!(second.is_none());
        assert_eq!(repo.list_all().unwrap().len(), 1);
    }

    #[test]
    fn unassigned_articles_are_separated() {
        let repo = setup();
        repo.upsert(&article("https://a.com/1", "suelto")).unwrap();
        repo.upsert(&Article {
            source_id: Some(5),
            ..article("https://a.com/2", "de fuente")
        })
        .unwrap();
        let unassigned = repo.list_unassigned().unwrap();
        assert_eq!(unassigned.len(), 1);
        assert_eq!(unassigned[0].title, "suelto");
    }

    #[test]
    fn list_unread_filters_by_read_flag() {
        let repo = setup();
        let id1 = repo.upsert(&article("https://a.com/1", "no leído")).unwrap();
        let id2 = repo.upsert(&article("https://a.com/2", "leído")).unwrap();
        repo.mark_read(id2, true).unwrap();

        let unread = repo.list_unread().unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].id, id1);
    }

    #[test]
    fn list_starred_filters_by_starred_flag() {
        let repo = setup();
        let id1 = repo.upsert(&article("https://a.com/1", "destacado")).unwrap();
        repo.upsert(&article("https://a.com/2", "normal")).unwrap();
        repo.toggle_star(id1).unwrap();

        let starred = repo.list_starred().unwrap();
        assert_eq!(starred.len(), 1);
        assert_eq!(starred[0].id, id1);
    }

    #[test]
    fn list_recent_filters_by_date() {
        let repo = setup();
        repo.upsert(&Article {
            published_at: Some("2024-01-01T00:00:00Z".into()),
            ..article("https://a.com/1", "antiguo")
        }).unwrap();
        let id2 = repo.upsert(&Article {
            published_at: Some("2026-08-01T00:00:00Z".into()),
            ..article("https://a.com/2", "reciente")
        }).unwrap();

        let recent = repo.list_recent(30).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].id, id2);
    }

    fn feed_entry(title: &str, url: &str, summary: &str) -> FeedEntry {
        FeedEntry {
            title: title.into(),
            link: url.into(),
            summary: Some(summary.into()),
            published: Some("2024-01-01T00:00:00Z".into()),
        }
    }

    #[test]
    fn feed_entry_keeps_summary_after_extraction_and_purge() {
        let repo = setup();
        let id = repo
            .insert_feed_entry(1, &feed_entry("Post", "https://a.com/post", "resumen original"), "2024-01-02T00:00:00Z")
            .unwrap()
            .unwrap();

        // Simula la extracción: upsert con contenido completo; `summary` no se toca.
        let mut full = article("https://a.com/post", "Post");
        full.html = "<p>contenido completo</p>".into();
        full.text = "contenido completo en detalle".into();
        repo.upsert(&full).unwrap();
        let got = repo.get(id).unwrap().unwrap();
        assert_eq!(got.html, "<p>contenido completo</p>");

        // Al purgar un artículo leído, vuelve a su resumen original.
        repo.mark_read(id, true).unwrap();
        let purged = repo.purge_extracted_content(0).unwrap();
        assert_eq!(purged, 1);
        let got = repo.get(id).unwrap().unwrap();
        assert_eq!(got.html, "");
        assert_eq!(got.text, "resumen original");
        assert!(got.raw_html.is_empty());
    }

    #[test]
    fn purge_deletes_embedding_and_skips_unread_and_singles() {
        use crate::embedding_repo::{EmbeddingRepo, EmbeddingRepository};
        let repo = setup();
        let embeddings = EmbeddingRepo::new(repo.conn.clone());

        // Artículo de feed leído con contenido y embedding.
        let read_id = repo.upsert(&Article {
            source_id: Some(1),
            read: true,
            ..article("https://a.com/read", "leído")
        }).unwrap();
        embeddings.upsert(read_id, &vec![0.5; 384], "m", 1, "t").unwrap();
        assert!(repo.get(read_id).unwrap().unwrap().has_embedding);

        // Artículo de feed no leído y artículo suelto leído: no se tocan.
        let unread_id = repo.upsert(&Article {
            source_id: Some(1),
            read: false,
            ..article("https://a.com/unread", "no leído")
        }).unwrap();
        let single_id = repo.upsert(&Article {
            source_id: None,
            read: true,
            ..article("https://suelto.com/1", "suelto")
        }).unwrap();

        let purged = repo.purge_extracted_content(0).unwrap();
        assert_eq!(purged, 1);

        let read = repo.get(read_id).unwrap().unwrap();
        assert!(read.html.is_empty());
        assert!(embeddings.get(read_id).unwrap().is_none());
        assert!(!repo.list_all().unwrap().iter().find(|a| a.id == read_id).unwrap().has_embedding);

        assert!(!repo.get(unread_id).unwrap().unwrap().html.is_empty());
        assert!(!repo.get(single_id).unwrap().unwrap().html.is_empty());
    }

    #[test]
    fn purge_respects_age_filter() {
        let repo = setup();
        let old = repo.upsert(&Article {
            source_id: Some(1),
            read: true,
            fetched_at: "2020-01-01T00:00:00Z".into(),
            ..article("https://a.com/old", "viejo")
        }).unwrap();
        let recent = repo.upsert(&Article {
            source_id: Some(1),
            read: true,
            fetched_at: "2099-01-01T00:00:00Z".into(),
            ..article("https://a.com/new", "nuevo")
        }).unwrap();

        let purged = repo.purge_extracted_content(1000).unwrap();
        assert_eq!(purged, 1);
        assert!(repo.get(old).unwrap().unwrap().html.is_empty());
        assert!(!repo.get(recent).unwrap().unwrap().html.is_empty());
    }
}
