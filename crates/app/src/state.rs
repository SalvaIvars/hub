use reader_embeddings::{EmbeddingError, FastEmbedGenerator};
use reader_extractor::TrafilaturaExtractor;
use reader_feeds::{FeedRsParser, WebpageDiscoverer};
use reader_pipeline::ReqwestClient;
use reader_storage::{ArticleRepo, EmbeddingRepo, SettingsRepo, SmartFeedRepo, SourceRepo};
use std::sync::{Arc, Mutex};

/// Estado global de la aplicación, gestionado por Tauri.
///
/// Contiene las dependencias concretas. Los comandos construyen un `Pipeline`
/// con ellas en cada llamada. El generador de embeddings (`embedder`) se
/// carga de forma perezosa: el modelo se descarga la primera vez (~80MB), así
/// que no se inicializa hasta que hace falta.
pub struct AppState {
    pub http: ReqwestClient,
    pub extractor: TrafilaturaExtractor,
    pub discoverer: WebpageDiscoverer,
    pub parser: FeedRsParser,
    pub articles: ArticleRepo,
    pub sources: SourceRepo,
    pub settings: SettingsRepo,
    pub smart_feeds: SmartFeedRepo,
    pub embeddings: EmbeddingRepo,
    pub embedder: Arc<Mutex<Option<FastEmbedGenerator>>>,
}

impl AppState {
    pub fn new(conn: Arc<Mutex<rusqlite::Connection>>) -> Self {
        Self {
            http: ReqwestClient(reqwest::Client::new()),
            extractor: TrafilaturaExtractor,
            discoverer: WebpageDiscoverer,
            parser: FeedRsParser,
            articles: ArticleRepo::new(conn.clone()),
            sources: SourceRepo::new(conn.clone()),
            settings: SettingsRepo::new(conn.clone()),
            smart_feeds: SmartFeedRepo::new(conn.clone()),
            embeddings: EmbeddingRepo::new(conn),
            embedder: Arc::new(Mutex::new(None)),
        }
    }
}

/// Devuelve el generador de embeddings, inicializándolo (cargando el modelo)
/// la primera vez. La carga es bloqueante y puede tardar, por eso se hace en
/// un hilo blocking. Se llama solo desde comandos async.
pub async fn get_embedder(state: &AppState) -> Result<FastEmbedGenerator, String> {
    // Comprobación rápida sin retener el lock a través del await.
    if let Some(gen) = state.embedder.lock().unwrap().as_ref() {
        return Ok(gen.clone());
    }
    let gen = tokio::task::spawn_blocking(FastEmbedGenerator::new)
        .await
        .map_err(|e| format!("tarea de carga cancelada: {e}"))?
        .map_err(|e: EmbeddingError| format!("no se pudo cargar el modelo de embeddings: {e}"))?;
    *state.embedder.lock().unwrap() = Some(gen.clone());
    Ok(gen)
}
