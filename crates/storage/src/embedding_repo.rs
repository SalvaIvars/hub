use crate::StorageError;
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Arc, Mutex};
/// Puente de acceso a embeddings de artículos (puerto hexagonal).
///
/// Almacena los vectores en dos sitios: la tabla `article_embeddings`
/// (BLOB, para inspección y regeneración) y la virtual table `vec_articles`
/// (sqlite-vec, para búsqueda KNN por similitud).
pub trait EmbeddingRepository: Send + Sync {
    /// Guarda o reemplaza el embedding de un artículo.
    fn upsert(
        &self,
        article_id: i64,
        embedding: &[f32],
        model: &str,
        tokens_used: usize,
        created_at: &str,
    ) -> Result<(), StorageError>;
    /// Elimina el embedding de un artículo (si existe).
    fn delete(&self, article_id: i64) -> Result<(), StorageError>;
    /// Recupera el embedding de un artículo.
    fn get(&self, article_id: i64) -> Result<Option<Vec<f32>>, StorageError>;
    /// Búsqueda por similitud coseno (KNN). Devuelve (article_id, distancia).
    fn search(&self, query_embedding: &[f32], limit: i64) -> Result<Vec<(i64, f32)>, StorageError>;
    /// Conteo (total, sin leer) de artículos con embedding.
    fn count_embedded(&self) -> Result<(i64, i64), StorageError>;
    /// Ids de artículos que todavía no tienen embedding (máx. `limit`).
    fn articles_without_embedding(&self, limit: i64) -> Result<Vec<i64>, StorageError>;
}

/// Adaptador concreto sobre SQLite.
#[derive(Clone)]
pub struct EmbeddingRepo {
    conn: Arc<Mutex<Connection>>,
}

impl EmbeddingRepo {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

impl EmbeddingRepository for EmbeddingRepo {
    fn upsert(
        &self,
        article_id: i64,
        embedding: &[f32],
        model: &str,
        tokens_used: usize,
        created_at: &str,
    ) -> Result<(), StorageError> {
        let blob = crate::embedding_to_blob(embedding);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO article_embeddings (article_id, embedding, model, tokens_used, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(article_id) DO UPDATE SET
                embedding = excluded.embedding,
                model = excluded.model,
                tokens_used = excluded.tokens_used,
                created_at = excluded.created_at
            "#,
            params![article_id, blob, model, tokens_used as i64, created_at],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO vec_articles (article_id, embedding) VALUES (?1, ?2)",
            params![article_id, blob],
        )?;
        Ok(())
    }

    fn delete(&self, article_id: i64) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM article_embeddings WHERE article_id = ?1",
            params![article_id],
        )?;
        conn.execute(
            "DELETE FROM vec_articles WHERE article_id = ?1",
            params![article_id],
        )?;
        Ok(())
    }

    fn get(&self, article_id: i64) -> Result<Option<Vec<f32>>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let blob: Option<Vec<u8>> = conn
            .query_row(
                "SELECT embedding FROM article_embeddings WHERE article_id = ?1",
                params![article_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::Sqlite)?;
        match blob {
            Some(b) => crate::blob_to_embedding(&b).map(Some),
            None => Ok(None),
        }
    }

    fn search(&self, query_embedding: &[f32], limit: i64) -> Result<Vec<(i64, f32)>, StorageError> {
        let blob = crate::embedding_to_blob(query_embedding);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT article_id, distance
            FROM vec_articles
            WHERE embedding MATCH ?1
            ORDER BY distance
            LIMIT ?2
            "#,
        )?;
        let rows = stmt
            .query_map(params![blob, limit], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(StorageError::Sqlite)?;
        rows.collect::<Result<_, _>>().map_err(StorageError::Sqlite)
    }

    fn count_embedded(&self) -> Result<(i64, i64), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            r#"
            SELECT COUNT(*),
                   COALESCE(SUM(CASE WHEN a.read = 0 THEN 1 ELSE 0 END), 0)
            FROM vec_articles v
            JOIN articles a ON a.id = v.article_id
            "#,
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(StorageError::Sqlite)
    }

    fn articles_without_embedding(&self, limit: i64) -> Result<Vec<i64>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT a.id
            FROM articles a
            LEFT JOIN article_embeddings e ON e.article_id = a.id
            WHERE e.article_id IS NULL
            ORDER BY a.id
            LIMIT ?1
            "#,
        )?;
        let rows = stmt
            .query_map(params![limit], |row| row.get(0))
            .map_err(StorageError::Sqlite)?;
        rows.collect::<Result<_, _>>().map_err(StorageError::Sqlite)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::article_repo::{ArticleRepo, ArticleRepository};
    use reader_domain::Article;

    fn setup() -> (EmbeddingRepo, ArticleRepo) {
        let conn = Arc::new(std::sync::Mutex::new(crate::open_db_in_memory().unwrap()));
        (EmbeddingRepo::new(conn.clone()), ArticleRepo::new(conn))
    }

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

    #[test]
    fn upsert_get_delete_roundtrip() {
        let (repo, articles) = setup();
        let id = articles.upsert(&art("https://a.com/1", "uno")).unwrap();
        let emb: Vec<f32> = (0..384).map(|i| i as f32 / 10.0).collect();

        repo.upsert(id, &emb, "test-model", 128, "2024-01-01T00:00:00Z").unwrap();
        let got = repo.get(id).unwrap().unwrap();
        assert_eq!(got.len(), 384);
        assert!((got[100] - 10.0).abs() < 1e-4);

        repo.delete(id).unwrap();
        assert!(repo.get(id).unwrap().is_none());
    }

    #[test]
    fn search_ranks_by_similarity() {
        let (repo, articles) = setup();
        let a1 = articles.upsert(&art("https://a.com/1", "primero")).unwrap();
        let a2 = articles.upsert(&art("https://a.com/2", "segundo")).unwrap();
        let a3 = articles.upsert(&art("https://a.com/3", "tercero")).unwrap();

        // Vector unitario distinto para cada artículo: el 2 es el más parecido a la query.
        let mut v1 = vec![0.0; 384];
        v1[0] = 1.0;
        let mut v2 = vec![0.0; 384];
        v2[1] = 1.0;
        let mut v3 = vec![0.0; 384];
        v3[2] = 1.0;
        repo.upsert(a1, &v1, "m", 1, "t").unwrap();
        repo.upsert(a2, &v2, "m", 1, "t").unwrap();
        repo.upsert(a3, &v3, "m", 1, "t").unwrap();

        // Query = vector de a2: bajo métrica cosine, a2 queda a distancia 0.
        let query = v2;
        let results = repo.search(&query, 10).unwrap();
        assert_eq!(results.len(), 3);
        // El primero es el más parecido (distancia coseno menor).
        assert_eq!(results[0].0, a2);
        assert!(results.iter().any(|(id, _)| *id == a1));
        assert!(results.iter().any(|(id, _)| *id == a3));
    }

    #[test]
    fn count_embedded_and_missing() {
        let (repo, articles) = setup();
        let id1 = articles.upsert(&art("https://a.com/1", "uno")).unwrap();
        let id2 = articles.upsert(&art("https://a.com/2", "dos")).unwrap();
        articles.upsert(&art("https://a.com/3", "tres")).unwrap();

        assert_eq!(repo.count_embedded().unwrap(), (0, 0));
        let missing = repo.articles_without_embedding(10).unwrap();
        assert_eq!(missing, vec![id1, id2, id1 + 2]);

        let emb = vec![0.1; 384];
        repo.upsert(id1, &emb, "m", 1, "t").unwrap();
        assert_eq!(repo.count_embedded().unwrap(), (1, 1));
        let missing = repo.articles_without_embedding(10).unwrap();
        assert_eq!(missing, vec![id2, id1 + 2]);

        articles.mark_read(id1, true).unwrap();
        assert_eq!(repo.count_embedded().unwrap(), (1, 0));
    }
}
