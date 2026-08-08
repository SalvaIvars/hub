use crate::StorageError;
use reader_domain::{ArticleSummary, SearchMode, SmartFeed};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Arc, Mutex};

/// Puente de acceso a smart feeds (puerto hexagonal).
pub trait SmartFeedRepository: Send + Sync {
    fn create(
        &self,
        name: &str,
        query: &str,
        search_mode: &str,
        created_at: &str,
    ) -> Result<i64, StorageError>;
    fn list(&self) -> Result<Vec<SmartFeed>, StorageError>;
    fn get(&self, id: i64) -> Result<Option<SmartFeed>, StorageError>;
    fn delete(&self, id: i64) -> Result<(), StorageError>;
    /// Búsqueda por palabras clave (FTS5, ranking BM25).
    fn get_articles(&self, query: &str) -> Result<Vec<ArticleSummary>, StorageError>;
    /// Búsqueda semántica (KNN sobre `vec_articles`). Solo devuelve artículos
    /// con similitud coseno >= `min_similarity` (0.0–1.0); `0.0` no filtra nada.
    fn search_vector(
        &self,
        query_embedding: &[f32],
        limit: i64,
        min_similarity: f32,
    ) -> Result<Vec<ArticleSummary>, StorageError>;
}

/// Adaptador concreto sobre SQLite.
#[derive(Clone)]
pub struct SmartFeedRepo {
    conn: Arc<Mutex<Connection>>,
}

impl SmartFeedRepo {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

impl SmartFeedRepository for SmartFeedRepo {
    fn create(
        &self,
        name: &str,
        query: &str,
        search_mode: &str,
        created_at: &str,
    ) -> Result<i64, StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "INSERT INTO smart_feeds (name, query, search_mode, created_at) VALUES (?1, ?2, ?3, ?4) RETURNING id",
            params![name, query, search_mode, created_at],
            |row| row.get(0),
        )
        .map_err(StorageError::Sqlite)
    }

    fn list(&self) -> Result<Vec<SmartFeed>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, query, search_mode, created_at FROM smart_feeds ORDER BY name COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SmartFeed {
                id: row.get(0)?,
                name: row.get(1)?,
                query: row.get(2)?,
                search_mode: SearchMode::from_str(&row.get::<_, String>(3)?),
                created_at: row.get(4)?,
                article_count: 0,
                unread_count: 0,
            })
        })?;
        let mut feeds = rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::Sqlite)?;
        for feed in &mut feeds {
            let (total, unread) = count_matches(&conn, &feed.query, feed.search_mode)?;
            feed.article_count = total;
            feed.unread_count = unread;
        }
        Ok(feeds)
    }

    fn get(&self, id: i64) -> Result<Option<SmartFeed>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut feed = conn
            .query_row(
                "SELECT id, name, query, search_mode, created_at FROM smart_feeds WHERE id = ?1",
                params![id],
                |row| {
                    Ok(SmartFeed {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        query: row.get(2)?,
                        search_mode: SearchMode::from_str(&row.get::<_, String>(3)?),
                        created_at: row.get(4)?,
                        article_count: 0,
                        unread_count: 0,
                    })
                },
            )
            .optional()
            .map_err(StorageError::Sqlite)?;
        if let Some(feed) = &mut feed {
            let (total, unread) = count_matches(&conn, &feed.query, feed.search_mode)?;
            feed.article_count = total;
            feed.unread_count = unread;
        }
        Ok(feed)
    }

    fn delete(&self, id: i64) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute("DELETE FROM smart_feeds WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(StorageError::NotFound(format!("smart feed {id}")));
        }
        Ok(())
    }

    fn get_articles(&self, query: &str) -> Result<Vec<ArticleSummary>, StorageError> {
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
        let rows = stmt.query_map(params![fts], |row| {
            let mut summary = row_to_summary(row)?;
            summary.snippet = row.get(11)?;
            Ok(summary)
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::Sqlite)
    }

    fn search_vector(
        &self,
        query_embedding: &[f32],
        limit: i64,
        min_similarity: f32,
    ) -> Result<Vec<ArticleSummary>, StorageError> {
        let blob = crate::embedding_to_blob(query_embedding);
        // sqlite-vec devuelve `distance = 1 - cosine_similarity`, así que un
        // umbral de similitud s equivale a `distance <= 1 - s`.
        let max_distance = 1.0_f32 - min_similarity.clamp(0.0, 1.0);
        let conn = self.conn.lock().unwrap();
        // El LIMIT debe aplicar directamente sobre el escaneo KNN de vec0,
        // así que el MATCH vive en una subconsulta.
        let sql = r#"
            SELECT a.id, a.source_id, s.title, a.url, a.title, a.site_name,
                   a.published_at, a.fetched_at, a.read, a.starred,
                   CASE WHEN e.article_id IS NOT NULL THEN 1 ELSE 0 END
            FROM (
                SELECT article_id, distance
                FROM vec_articles
                WHERE embedding MATCH ?1
                ORDER BY distance
                LIMIT ?2
            ) v
            JOIN articles a ON a.id = v.article_id
            LEFT JOIN sources s ON s.id = a.source_id
            LEFT JOIN article_embeddings e ON e.article_id = a.id
            WHERE v.distance <= ?3
            ORDER BY v.distance
        "#;
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![blob, limit, max_distance], row_to_summary)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::Sqlite)
    }
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

/// Cuenta (total, sin leer) de los artículos que matchean un smart feed.
///
/// - `Bm25` / `Hybrid`: cuenta los resultados de la búsqueda FTS5 (la parte
///   léxica; en `Hybrid` es un límite inferior razonable para el badge).
/// - `Vector`: cuenta los artículos con embedding, que es el corpus sobre el
///   que se ejecuta la búsqueda semántica.
///
/// La consulta se normaliza igual que en `get_articles`, de modo que los
/// contadores del sidebar siempre cuadran con los resultados reales.
fn count_matches(
    conn: &Connection,
    query: &str,
    mode: SearchMode,
) -> Result<(i64, i64), StorageError> {
    match mode {
        SearchMode::Vector => conn
            .query_row(
                r#"
                SELECT COUNT(*),
                       COALESCE(SUM(CASE WHEN a.read = 0 THEN 1 ELSE 0 END), 0)
                FROM vec_articles v
                JOIN articles a ON a.id = v.article_id
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(StorageError::Sqlite),
        SearchMode::Bm25 | SearchMode::Hybrid => {
            let fts = crate::to_fts_query(query);
            if fts.trim().is_empty() {
                return Ok((0, 0));
            }
            conn.query_row(
                r#"
                SELECT COUNT(*),
                       COALESCE(SUM(CASE WHEN a.read = 0 THEN 1 ELSE 0 END), 0)
                FROM articles_fts fts
                JOIN articles a ON a.id = fts.rowid
                WHERE articles_fts MATCH ?1
                "#,
                params![fts],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(StorageError::Sqlite)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reader_domain::Article;

    fn setup() -> SmartFeedRepo {
        let conn = Arc::new(std::sync::Mutex::new(crate::open_db_in_memory().unwrap()));
        SmartFeedRepo::new(conn)
    }

    #[test]
    fn create_and_list() {
        let repo = setup();
        let id = repo
            .create("Rust", "rust", "bm25", "2024-01-01T00:00:00Z")
            .unwrap();
        let feeds = repo.list().unwrap();
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].id, id);
        assert_eq!(feeds[0].name, "Rust");
        assert_eq!(feeds[0].query, "rust");
        assert_eq!(feeds[0].search_mode, SearchMode::Bm25);
    }

    #[test]
    fn create_supports_vector_and_hybrid_modes() {
        let repo = setup();
        let vid = repo
            .create("Semántico", "idea", "vector", "2024-01-01T00:00:00Z")
            .unwrap();
        let hid = repo
            .create("Híbrido", "idea", "hybrid", "2024-01-01T00:00:00Z")
            .unwrap();
        assert_eq!(repo.get(vid).unwrap().unwrap().search_mode, SearchMode::Vector);
        assert_eq!(repo.get(hid).unwrap().unwrap().search_mode, SearchMode::Hybrid);
        assert_eq!(repo.get(vid).unwrap().unwrap().query, "idea");
    }

    #[test]
    fn delete_removes_smart_feed() {
        let repo = setup();
        let id = repo
            .create("Test", "test", "bm25", "2024-01-01T00:00:00Z")
            .unwrap();
        repo.delete(id).unwrap();
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
    fn counts_match_results_for_unnormalized_query() {
        use crate::article_repo::ArticleRepository;
        use reader_domain::Article;

        let conn = Arc::new(std::sync::Mutex::new(crate::open_db_in_memory().unwrap()));
        let smart = SmartFeedRepo::new(conn.clone());
        let articles = crate::article_repo::ArticleRepo::new(conn.clone());
        let id = smart
            .create("Rust", "Rust async", "bm25", "2024-01-01T00:00:00Z")
            .unwrap();

        fn art(url: &str, text: &str) -> Article {
            Article {
                id: 0,
                source_id: None,
                url: url.to_string(),
                title: text.to_string(),
                html: format!("<p>{text}</p>"),
                text: text.to_string(),
                raw_html: String::new(),
                byline: None,
                site_name: None,
                published_at: None,
                fetched_at: "2024-01-02T00:00:00Z".to_string(),
                read: false,
                starred: false,
                has_embedding: false,
            }
        }
        articles.upsert(&art("https://a.com/1", "Rust async es genial")).unwrap();
        articles.upsert(&art("https://a.com/2", "Python es fácil")).unwrap();

        let feed = smart.get(id).unwrap().unwrap();
        let results = smart.get_articles("Rust async").unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].has_embedding);
        assert_eq!(feed.article_count, results.len() as i64);
        assert_eq!(feed.unread_count, results.len() as i64);

        let listed = smart.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].article_count, 1);
        assert_eq!(listed[0].unread_count, 1);
    }

    #[test]
    fn vector_mode_counts_embedded_corpus() {
        use crate::article_repo::{ArticleRepo, ArticleRepository};
        use crate::embedding_repo::{EmbeddingRepo, EmbeddingRepository};
        use reader_domain::Article;

        let conn = Arc::new(std::sync::Mutex::new(crate::open_db_in_memory().unwrap()));
        let smart = SmartFeedRepo::new(conn.clone());
        let articles = ArticleRepo::new(conn.clone());
        let embeddings = EmbeddingRepo::new(conn.clone());

        let sf = smart
            .create("Semántico", "idea", "vector", "2024-01-01T00:00:00Z")
            .unwrap();
        let a1 = articles
            .upsert(&Article {
                url: "https://a.com/1".into(),
                title: "uno".into(),
                text: "texto uno".into(),
                ..empty_article()
            })
            .unwrap();
        articles
            .upsert(&Article {
                url: "https://a.com/2".into(),
                title: "dos".into(),
                text: "texto dos".into(),
                ..empty_article()
            })
            .unwrap();

        // Sin embeddings todavía: el corpus vectorial está vacío.
        assert_eq!(smart.get(sf).unwrap().unwrap().article_count, 0);

        embeddings.upsert(a1, &vec![0.1; 384], "m", 1, "t").unwrap();
        let feed = smart.get(sf).unwrap().unwrap();
        assert_eq!(feed.article_count, 1);
        assert_eq!(feed.unread_count, 1);

        // La búsqueda vectorial devuelve el artículo embedado.
        let hits = smart.search_vector(&vec![0.1; 384], 10, 0.0).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, a1);
        assert!(hits[0].has_embedding);
    }

    #[test]
    fn vector_search_filters_by_min_similarity() {
        use crate::article_repo::{ArticleRepo, ArticleRepository};
        use crate::embedding_repo::{EmbeddingRepo, EmbeddingRepository};
        use reader_domain::Article;

        let conn = Arc::new(std::sync::Mutex::new(crate::open_db_in_memory().unwrap()));
        let smart = SmartFeedRepo::new(conn.clone());
        let articles = ArticleRepo::new(conn.clone());
        let embeddings = EmbeddingRepo::new(conn.clone());

        let a1 = articles
            .upsert(&Article {
                url: "https://a.com/1".into(),
                title: "idéntico".into(),
                text: "texto uno".into(),
                ..empty_article()
            })
            .unwrap();
        let a2 = articles
            .upsert(&Article {
                url: "https://a.com/2".into(),
                title: "distinto".into(),
                text: "texto dos".into(),
                ..empty_article()
            })
            .unwrap();

        // a1 casi colineal con la query (distancia ~0.05), a2 casi ortogonal.
        let mut v1 = vec![1.0, 0.0, 0.0, 0.0];
        let mut v2 = vec![0.0, 1.0, 0.0, 0.0];
        v1.resize(384, 0.0);
        v2.resize(384, 0.0);
        embeddings.upsert(a1, &v1, "m", 1, "t").unwrap();
        embeddings.upsert(a2, &v2, "m", 1, "t").unwrap();

        // Sin umbral: aparecen ambos (el 2 por distancia, sin filtrar).
        let mut wide = vec![1.0, 0.0, 0.0, 0.0];
        wide.resize(384, 0.0);
        let hits = smart.search_vector(&wide, 10, 0.0).unwrap();
        assert_eq!(hits.len(), 2);

        // Umbral estricto: solo a1 (similitud >= 0.9) pasa el filtro.
        let strict = smart.search_vector(&wide, 10, 0.9).unwrap();
        assert_eq!(strict.len(), 1);
        assert_eq!(strict[0].id, a1);

        // Umbral máximo (>= 1.0): exige coincidencia perfecta; solo a1, que es
        // idéntico a la query (distancia coseno 0), pasa el filtro.
        let perfect = smart.search_vector(&wide, 10, 1.0).unwrap();
        assert_eq!(perfect.len(), 1);
        assert_eq!(perfect[0].id, a1);
        // Umbral 1.5 se normaliza a 1.0 (mismo comportamiento).
        let clamped = smart.search_vector(&wide, 10, 1.5).unwrap();
        assert_eq!(clamped.len(), 1);
        // Umbral negativo = sin filtrar.
        let all = smart.search_vector(&wide, 10, -1.0).unwrap();
        assert_eq!(all.len(), 2);
    }

    fn empty_article() -> Article {
        Article {
            id: 0,
            source_id: None,
            url: String::new(),
            title: String::new(),
            html: String::new(),
            text: String::new(),
            raw_html: String::new(),
            byline: None,
            site_name: None,
            published_at: None,
            fetched_at: "2024-01-02T00:00:00Z".to_string(),
            read: false,
            starred: false,
            has_embedding: false,
        }
    }
}
