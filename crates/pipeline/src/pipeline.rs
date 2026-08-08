use crate::http::{FetchError, HttpClient};
use crate::index::is_index_page;
use crate::utc_now;
use reader_domain::{Article, FeedEntry, IngestResult, Source, SourceSummary};
use reader_extractor::{ArticleExtractor, ExtractorError, ExtractedArticle};
use reader_feeds::{FeedDiscoverer, FeedError, FeedParser};
use reader_storage::{ArticleRepository, SourceRepository, StorageError};
use url::Url;

/// Error tipado de la orquestación.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("URL inválida: {0}")]
    InvalidUrl(String),
    #[error("{0}")]
    Fetch(#[from] FetchError),
    #[error("{0}")]
    Feed(#[from] FeedError),
    #[error("{0}")]
    Extract(#[from] ExtractorError),
    #[error("{0}")]
    Storage(#[from] StorageError),
    #[error("source no encontrado: {0}")]
    SourceNotFound(i64),
    #[error("el source {0} no tiene feed asociado")]
    NoFeed(i64),
}

/// Dependencias inyectadas del pipeline.
///
/// Todas son referencias a puertos, de modo que en tests se pueden sustituir
/// por mocks sin tocar nada más.
pub struct Pipeline<'a> {
    pub http: &'a dyn HttpClient,
    pub extractor: &'a dyn ArticleExtractor,
    pub discoverer: &'a dyn FeedDiscoverer,
    pub parser: &'a dyn FeedParser,
    pub articles: &'a dyn ArticleRepository,
    pub sources: &'a dyn SourceRepository,
}

impl Pipeline<'_> {
    /// Nº máximo de feeds candidatos que se intentan hasta que uno parsee.
    const MAX_FEED_TRIES: usize = 4;

    /// Ingiere un URL: descubre feed, guarda posts y extrae el artículo pegado.
    pub async fn ingest_url(&self, url: &str) -> Result<IngestResult, PipelineError> {
        let base_url = Url::parse(url).map_err(|_| PipelineError::InvalidUrl(url.into()))?;
        if base_url.scheme() != "http" && base_url.scheme() != "https" {
            return Err(PipelineError::InvalidUrl(format!(
                "esquema no soportado: {}",
                base_url.scheme()
            )));
        }

        let page = self.http.fetch(base_url.as_str()).await?;
        let home_url = page.final_url.clone();
        let now = utc_now();

        // Si el contenido es en sí un feed (p. ej. el usuario pegó feed.xml),
        // usarlo directamente.
        let direct_feed = if self.discoverer.discover(&page.html, &base_url)?.is_empty() {
            self.parser.parse(&page.html).ok().filter(|e| !e.is_empty())
        } else {
            None
        };

        let mut source_id: Option<i64> = None;
        let mut feed_added = 0usize;
        let mut first_feed_article_id: Option<i64> = None;
        let is_feed_url = direct_feed.is_some();

        if let Some(entries) = direct_feed {
            let sid = self
                .ensure_source(&home_url, Some(home_url.clone()), None, &now)
                .await?;
            let (added, first_id) = self.persist_feed_entries(sid, entries, &now)?;
            feed_added = added;
            first_feed_article_id = first_id;
            source_id = Some(sid);
        } else {
            let links = self.discoverer.discover(&page.html, &base_url)?;
            for link in links.iter().take(Self::MAX_FEED_TRIES) {
                match self.http.fetch(&link.href).await {
                    Ok(feed_page) => match self.parser.parse(&feed_page.html) {
                        Ok(entries) if !entries.is_empty() => {
                            let sid = self
                                .ensure_source(
                                    &home_url,
                                    Some(link.href.clone()),
                                    link.title.clone(),
                                    &now,
                                )
                                .await?;
                            let (added, first_id) =
                                self.persist_feed_entries(sid, entries, &now)?;
                            feed_added = added;
                            first_feed_article_id = first_id;
                            source_id = Some(sid);
                            break;
                        }
                        _ => continue,
                    },
                    Err(_) => continue,
                }
            }
        }

        // Si el URL pegado era un feed o una portada/índice (página que solo
        // lista otros posts), no hay artículo que extraer: se devuelve el
        // primer post del feed como resultado, o ningún artículo si no hubo.
        let is_index = is_index_page(&page.html, &Url::parse(&home_url).unwrap_or(base_url.clone()));
        if is_feed_url || is_index {
            if let (Some(sid), Some(aid)) = (source_id, first_feed_article_id) {
                let title = self
                    .articles
                    .get(aid)?
                    .map(|a| a.title)
                    .unwrap_or_else(|| "Artículo nuevo".to_string());
                return Ok(IngestResult {
                    source: Some(self.source_summary(sid)?),
                    article_id: Some(aid),
                    article_title: title,
                    feed_articles_added: feed_added,
                });
            }
            return Ok(IngestResult {
                source: if let Some(sid) = source_id {
                    Some(self.source_summary(sid)?)
                } else {
                    None
                },
                article_id: None,
                article_title: String::new(),
                feed_articles_added: feed_added,
            });
        }

        // Extracción del artículo pegado (best effort: puede fallar; aún así se
        // guarda un artículo con el título).
        let extracted = self
            .extractor
            .extract(&page.html, &home_url)
            .unwrap_or_else(|_| fallback_extraction(&page.html, &home_url));

        let article_id = self.articles.upsert(&Article {
            id: 0,
            source_id,
            url: home_url.clone(),
            title: extracted.title.clone(),
            html: extracted.content_html,
            text: extracted.text_content,
            raw_html: page.html,
            byline: extracted.byline,
            site_name: extracted.site_name,
            published_at: extracted.published_time,
            fetched_at: now,
            read: false,
            starred: false,
            has_embedding: false,
        })?;

        Ok(IngestResult {
            source: if let Some(sid) = source_id {
                Some(self.source_summary(sid)?)
            } else {
                None
            },
            article_id: Some(article_id),
            article_title: extracted.title,
            feed_articles_added: feed_added,
        })
    }

    /// Re-descarga el feed de un source y añade los posts nuevos.
    /// Actualiza la salud del feed (último error, status HTTP, contador de errores).
    pub async fn refresh_source(&self, source_id: i64) -> Result<usize, PipelineError> {
        let source = self
            .sources
            .get(source_id)?
            .ok_or(PipelineError::SourceNotFound(source_id))?;
        let feed_url = source
            .feed_url
            .clone()
            .ok_or(PipelineError::NoFeed(source_id))?;

        match self.http.fetch(&feed_url).await {
            Ok(page) => {
                match self.parser.parse(&page.html) {
                    Ok(entries) => {
                        let now = utc_now();
                        let (added, _) = self.persist_feed_entries(source_id, entries, &now)?;
                        self.sources.update_last_fetched(source_id, &now)?;
                        self.sources.update_health(source_id, Some(200), None)?;
                        self.sources.reset_error_count(source_id)?;
                        Ok(added)
                    }
                    Err(e) => {
                        self.sources.update_health(source_id, Some(200), Some(&e.to_string()))?;
                        self.sources.increment_error_count(source_id)?;
                        Err(e.into())
                    }
                }
            }
            Err(e) => {
                let status = match &e {
                    FetchError::HttpStatus(code) => Some(*code as i64),
                    _ => None,
                };
                self.sources.update_health(source_id, status, Some(&e.to_string()))?;
                self.sources.increment_error_count(source_id)?;
                Err(e.into())
            }
        }
    }

    /// Refresca todos los sources que tienen feed. Devuelve el total de
    /// artículos nuevos. Los errores de cada source se ignoran (best effort).
    pub async fn refresh_all(&self) -> Result<usize, PipelineError> {
        let summaries = self.sources.list()?;
        let mut total = 0usize;
        for summary in summaries {
            if summary.feed_url.is_none() {
                continue;
            }
            if let Ok(added) = self.refresh_source(summary.id).await {
                total += added;
            }
        }
        Ok(total)
    }

    /// Extrae y guarda un artículo concreto por URL (sin tocar feeds).
    ///
    /// Devuelve el `Article` guardado. Útil para que el lector obtenga el
    /// contenido completo de un post del feed que se guardó solo con resumen.
    pub async fn extract_article(&self, url: &str) -> Result<Article, PipelineError> {
        let page = self.http.fetch(url).await?;
        let home_url = page.final_url.clone();
        let extracted = self
            .extractor
            .extract(&page.html, &home_url)
            .unwrap_or_else(|_| fallback_extraction(&page.html, &home_url));
        let now = utc_now();

        let id = self.articles.upsert(&Article {
            id: 0,
            source_id: None,
            url: home_url.clone(),
            title: extracted.title.clone(),
            html: extracted.content_html,
            text: extracted.text_content,
            raw_html: page.html,
            byline: extracted.byline,
            site_name: extracted.site_name,
            published_at: extracted.published_time,
            fetched_at: now,
            read: false,
            starred: false,
            has_embedding: false,
        })?;

        self.articles
            .get(id)?
            .ok_or_else(|| StorageError::NotFound(format!("artículo {id}")).into())
    }

    /// Crea el source si no existe, o actualiza su feed/título si ya existe
    /// para el mismo home (dedupe por `home_url`).
    async fn ensure_source(
        &self,
        home_url: &str,
        feed_url: Option<String>,
        title_hint: Option<String>,
        now: &str,
    ) -> Result<i64, PipelineError> {
        let title = title_hint
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| crate::host_of(home_url));

        if let Some(existing) = self.sources.find_by_home_url(home_url)? {
            let updated = Source {
                id: existing.id,
                url: feed_url.clone().unwrap_or(existing.url.clone()),
                home_url: existing.home_url,
                title: if existing.title == crate::host_of(home_url) {
                    title
                } else {
                    existing.title
                },
                description: existing.description,
                feed_url: feed_url.or(existing.feed_url),
                last_fetched_at: Some(now.to_string()),
                last_error: existing.last_error,
                last_status: existing.last_status,
                error_count: existing.error_count,
                category: existing.category,
            };
            self.sources.update(&updated)?;
            return Ok(existing.id);
        }

        let id = self.sources.upsert(&Source {
            id: 0,
            url: feed_url.clone().unwrap_or_else(|| home_url.to_string()),
            home_url: home_url.to_string(),
            title,
            description: None,
            feed_url,
            last_fetched_at: Some(now.to_string()),
            last_error: None,
            last_status: None,
            error_count: 0,
            category: None,
        })?;
        Ok(id)
    }

    /// Devuelve (nº de nuevos, id del primero si se añadió alguno).
    fn persist_feed_entries(
        &self,
        source_id: i64,
        entries: Vec<FeedEntry>,
        fetched_at: &str,
    ) -> Result<(usize, Option<i64>), PipelineError> {
        let mut added = 0;
        let mut first_id = None;
        for entry in entries {
            if let Some(id) = self
                .articles
                .insert_feed_entry(source_id, &entry, fetched_at)?
            {
                if first_id.is_none() {
                    first_id = Some(id);
                }
                added += 1;
            }
        }
        Ok((added, first_id))
    }

    fn source_summary(&self, id: i64) -> Result<SourceSummary, PipelineError> {
        let source = self
            .sources
            .get(id)?
            .ok_or(PipelineError::SourceNotFound(id))?;
        let (article_count, unread_count) = self.articles.count_by_source(id)?;
        Ok(SourceSummary {
            id: source.id,
            url: source.url,
            home_url: source.home_url,
            title: source.title,
            description: source.description,
            feed_url: source.feed_url,
            last_fetched_at: source.last_fetched_at,
            article_count,
            unread_count,
            last_error: source.last_error,
            error_count: source.error_count,
            category: source.category,
        })
    }
}

/// Extracción mínima cuando trafilatura no logra extraer contenido:
/// conserva el `<title>` y el texto de la página tal cual.
fn fallback_extraction(html: &str, url: &str) -> ExtractedArticle {
    ExtractedArticle {
        title: html_title(html).unwrap_or_else(|| crate::host_of(url)),
        content_html: String::new(),
        text_content: strip_html(html),
        byline: None,
        site_name: None,
        published_time: None,
        lang: None,
    }
}

fn html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title>")? + "<title>".len();
    let end = lower[start..].find("</title>")? + start;
    let title = html[start..end].trim().to_string();
    (!title.is_empty()).then_some(title)
}

/// Elimina las etiquetas HTML de forma simple para el texto plano de reserva.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let mut prev_ws = false;
    let mut clean = String::with_capacity(out.len());
    for c in out.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                clean.push(' ');
                prev_ws = true;
            }
        } else {
            clean.push(c);
            prev_ws = false;
        }
    }
    clean.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::FetchedPage;
    use reader_feeds::FeedLink;

    // --- Mocks ligeros (sin dependencias externas) ---

    struct MockHttp(Vec<(String, String)>); // (url -> html)
    impl MockHttp {
        fn new(pairs: Vec<(&str, &str)>) -> Self {
            Self(pairs.into_iter().map(|(a, b)| (a.into(), b.into())).collect())
        }
        fn get(&self, url: &str) -> Option<String> {
            self.0
                .iter()
                .find(|(u, _)| u == url)
                .map(|(_, h)| h.clone())
        }
    }
    #[async_trait::async_trait]
    impl HttpClient for MockHttp {
        async fn fetch(&self, url: &str) -> Result<FetchedPage, FetchError> {
            match self.get(url) {
                Some(html) => Ok(FetchedPage {
                    final_url: url.to_string(),
                    html,
                }),
                None => Err(FetchError::HttpStatus(404)),
            }
        }
    }

    struct FakeExtractor;
    impl ArticleExtractor for FakeExtractor {
        fn extract(&self, _html: &str, _url: &str) -> Result<ExtractedArticle, ExtractorError> {
            Ok(ExtractedArticle {
                title: "Artículo extraído".into(),
                content_html: "<p>contenido</p>".into(),
                text_content: "contenido".into(),
                byline: None,
                site_name: Some("Sitio".into()),
                published_time: None,
                lang: None,
            })
        }
    }

    struct NoFeedDiscoverer;
    impl FeedDiscoverer for NoFeedDiscoverer {
        fn discover(&self, _html: &str, _base: &Url) -> Result<Vec<FeedLink>, FeedError> {
            Ok(Vec::new())
        }
    }

    struct FeedDiscovererStub;
    impl FeedDiscoverer for FeedDiscovererStub {
        fn discover(&self, _html: &str, base: &Url) -> Result<Vec<reader_feeds::FeedLink>, FeedError> {
            Ok(vec![reader_feeds::FeedLink {
                href: base.join("/feed.xml").unwrap().to_string(),
                title: None,
                kind: reader_feeds::FeedKind::Rss,
            }])
        }
    }

    struct FakeParser;
    impl FeedParser for FakeParser {
        fn parse(&self, body: &str) -> Result<Vec<FeedEntry>, FeedError> {
            let lower = body.to_ascii_lowercase();
            if !lower.contains("<rss") && !lower.contains("<feed") {
                return Err(FeedError::ParseError("no es un feed".into()));
            }
            Ok(vec![
                FeedEntry {
                    title: "Post 1".into(),
                    link: "https://site.com/p1".into(),
                    summary: Some("resumen 1".into()),
                    published: None,
                },
                FeedEntry {
                    title: "Post 2".into(),
                    link: "https://site.com/p2".into(),
                    summary: None,
                    published: None,
                },
            ])
        }
    }

    struct FakeStorage {
        articles: reader_storage::ArticleRepo,
        sources: reader_storage::SourceRepo,
    }
    fn storage() -> FakeStorage {
        let conn = std::sync::Arc::new(std::sync::Mutex::new(
            reader_storage::open_db_in_memory().unwrap(),
        ));
        FakeStorage {
            articles: reader_storage::ArticleRepo::new(conn.clone()),
            sources: reader_storage::SourceRepo::new(conn),
        }
    }

    const PAGE_HTML: &str = "<html><head><title>Sitio</title></head><body>página</body></html>";
    const FEED_XML: &str = "<rss version='2.0'><channel><title>Feed</title></channel></rss>";

    fn pipeline<'a>(http: &'a dyn HttpClient, storage: &'a FakeStorage) -> Pipeline<'a> {
        Pipeline {
            http,
            extractor: &FakeExtractor,
            discoverer: &NoFeedDiscoverer,
            parser: &FakeParser,
            articles: &storage.articles,
            sources: &storage.sources,
        }
    }

    #[tokio::test]
    async fn ingest_without_feed_creates_single_article() {
        let http = MockHttp::new(vec![("https://site.com/post", PAGE_HTML)]);
        let st = storage();
        let result = pipeline(&http, &st).ingest_url("https://site.com/post").await.unwrap();

        assert!(result.source.is_none());
        assert_eq!(result.article_title, "Artículo extraído");
        let article = st.articles.get(result.article_id.unwrap()).unwrap().unwrap();
        assert_eq!(article.source_id, None);
        assert_eq!(article.text, "contenido");
        assert!(!article.raw_html.is_empty());
    }

    #[tokio::test]
    async fn ingest_with_feed_creates_source_and_posts() {
        let http = MockHttp::new(vec![
            ("https://site.com/", PAGE_HTML),
            ("https://site.com/feed.xml", FEED_XML),
        ]);
        let st = storage();
        let pl = Pipeline {
            http: &http,
            extractor: &FakeExtractor,
            discoverer: &FeedDiscovererStub,
            parser: &FakeParser,
            articles: &st.articles,
            sources: &st.sources,
        };
        let result = pl.ingest_url("https://site.com/").await.unwrap();

        let summary = result.source.unwrap();
        assert_eq!(summary.feed_url.as_deref(), Some("https://site.com/feed.xml"));
        // La URL raíz es una portada: solo se guardan los posts del feed, no la página.
        assert_eq!(summary.article_count, 2);
        assert_eq!(result.feed_articles_added, 2);
        assert!(result.article_id.is_some()); // el primer post del feed como resultado

        let all = st.articles.list_all().unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|a| a.source_id == Some(summary.id)));
    }

    #[tokio::test]
    async fn ingest_root_index_without_feed_saves_nothing() {
        // Una portada sin feed no genera ni source ni artículo basura.
        let http = MockHttp::new(vec![("https://site.com/", PAGE_HTML)]);
        let st = storage();
        let result = pipeline(&http, &st).ingest_url("https://site.com/").await.unwrap();

        assert!(result.source.is_none());
        assert_eq!(result.article_id, None);
        assert_eq!(result.feed_articles_added, 0);
        assert!(st.articles.list_all().unwrap().is_empty());
    }

    #[tokio::test]
    async fn ingest_article_subpath_is_saved() {
        // Una subruta sin marcadores de índice es un artículo: se guarda.
        let http = MockHttp::new(vec![("https://site.com/posts/hello", PAGE_HTML)]);
        let st = storage();
        let result = pipeline(&http, &st)
            .ingest_url("https://site.com/posts/hello")
            .await
            .unwrap();

        assert!(result.source.is_none());
        let article = st.articles.get(result.article_id.unwrap()).unwrap().unwrap();
        assert_eq!(article.url, "https://site.com/posts/hello");
        assert_eq!(article.title, "Artículo extraído");
    }

    #[tokio::test]
    async fn ingest_pasted_feed_url_uses_it_directly() {
        let http = MockHttp::new(vec![("https://site.com/feed.xml", FEED_XML)]);
        let st = storage();
        let pl = Pipeline {
            http: &http,
            extractor: &FakeExtractor,
            discoverer: &NoFeedDiscoverer, // no descubre <link>, pero el body es un feed
            parser: &FakeParser,
            articles: &st.articles,
            sources: &st.sources,
        };
        let result = pl.ingest_url("https://site.com/feed.xml").await.unwrap();

        let summary = result.source.unwrap();
        assert_eq!(summary.url, "https://site.com/feed.xml");
        assert_eq!(summary.article_count, 2);
    }

    #[tokio::test]
    async fn refresh_adds_only_new_posts() {
        let http = MockHttp::new(vec![
            ("https://site.com/", PAGE_HTML),
            ("https://site.com/feed.xml", FEED_XML),
        ]);
        let st = storage();
        let pl = Pipeline {
            http: &http,
            extractor: &FakeExtractor,
            discoverer: &FeedDiscovererStub,
            parser: &FakeParser,
            articles: &st.articles,
            sources: &st.sources,
        };
        pl.ingest_url("https://site.com/").await.unwrap();
        let source_id = st.sources.list().unwrap()[0].id;

        let added = pl.refresh_source(source_id).await.unwrap();
        assert_eq!(added, 0); // mismo feed, nada nuevo
        assert_eq!(st.articles.list_all().unwrap().len(), 2);

        // El source queda marcado como actualizado
        let src = st.sources.get(source_id).unwrap().unwrap();
        assert!(src.last_fetched_at.is_some());
    }

    #[tokio::test]
    async fn invalid_url_is_rejected() {
        let http = MockHttp::new(vec![]);
        let st = storage();
        let err = pipeline(&http, &st).ingest_url("not-a-url").await;
        assert!(matches!(err, Err(PipelineError::InvalidUrl(_))));
    }

    #[tokio::test]
    async fn non_http_scheme_is_rejected() {
        let http = MockHttp::new(vec![]);
        let st = storage();
        let err = pipeline(&http, &st).ingest_url("file:///etc/passwd").await;
        assert!(matches!(err, Err(PipelineError::InvalidUrl(_))));
    }

    #[tokio::test]
    async fn refresh_missing_source_errors() {
        let http = MockHttp::new(vec![]);
        let st = storage();
        let err = pipeline(&http, &st).refresh_source(999).await;
        assert!(matches!(err, Err(PipelineError::SourceNotFound(999))));
    }

    #[tokio::test]
    async fn refresh_all_refreshes_every_feed() {
        let http = MockHttp::new(vec![
            ("https://site.com/", PAGE_HTML),
            ("https://site.com/feed.xml", FEED_XML),
        ]);
        let st = storage();
        let pl = Pipeline {
            http: &http,
            extractor: &FakeExtractor,
            discoverer: &FeedDiscovererStub,
            parser: &FakeParser,
            articles: &st.articles,
            sources: &st.sources,
        };
        pl.ingest_url("https://site.com/").await.unwrap();
        let source_id = st.sources.list().unwrap()[0].id;
        let source = st.sources.get(source_id).unwrap().unwrap();
        assert!(source.feed_url.is_some());

        let total = pl.refresh_all().await.unwrap();
        assert_eq!(total, 0); // nada nuevo
        assert_eq!(st.articles.list_all().unwrap().len(), 2);
        assert!(st.sources.get(source_id).unwrap().unwrap().last_fetched_at.is_some());
    }

    #[tokio::test]
    async fn refresh_source_without_feed_errors() {
        let http = MockHttp::new(vec![]);
        let st = storage();
        let source_id = st
            .sources
            .upsert(&Source {
                id: 0,
                url: "https://site.com/".into(),
                home_url: "https://site.com/".into(),
                title: "Site".into(),
                description: None,
                feed_url: None,
                last_fetched_at: None,
                last_error: None,
                last_status: None,
                error_count: 0,
                category: None,
            })
            .unwrap();
        let err = pipeline(&http, &st).refresh_source(source_id).await;
        assert!(matches!(err, Err(PipelineError::NoFeed(_))));
    }

    #[test]
    fn strip_html_removes_tags() {
        assert_eq!(strip_html("<p>Hola <b>mundo</b></p>"), "Hola mundo");
        assert_eq!(strip_html("sin etiquetas"), "sin etiquetas");
    }
}
