use crate::state::{get_embedder, AppState};
use reader_domain::{
    Article, ArticleSummary, IngestResult, ReadScope, ReaderSettings, SearchMode, SmartFeed,
    SourceSummary,
};
use reader_embeddings::{truncate_to_tokens, EmbeddingGenerator};
use reader_pipeline::Pipeline;
use reader_storage::{
    ArticleRepository, EmbeddingRepository, SettingsRepository, SmartFeedRepository, SourceRepository,
};
use tauri::State;

/// Ingiere un URL: descubre el feed (si existe), guarda los posts del feed y
/// extrae el artículo pegado. Devuelve un resumen de lo añadido.
#[tauri::command]
pub async fn add_url(url: String, state: State<'_, AppState>) -> Result<IngestResult, String> {
    pipeline(&state)
        .ingest_url(&url)
        .await
        .map_err(|e| e.to_string())
}

/// Extrae y guarda un artículo concreto por URL (para posts del feed que solo
/// tienen resumen). Devuelve el artículo completo.
#[tauri::command]
pub async fn extract_article(url: String, state: State<'_, AppState>) -> Result<Article, String> {
    pipeline(&state)
        .extract_article(&url)
        .await
        .map_err(|e| e.to_string())
}

/// Lista los sources guardados con sus conteos.
#[tauri::command]
pub fn list_sources(state: State<'_, AppState>) -> Result<Vec<SourceSummary>, String> {
    state.sources.list().map_err(|e| e.to_string())
}

/// Lista artículos. Si `q` está presente, hace búsqueda full-text; si
/// `source_id` está presente, filtra por source; si `filter` está presente,
/// aplica un filtro especial ("unread", "starred", "recent"); si todos son
/// `None`, lista toda la biblioteca.
#[tauri::command]
pub fn list_articles(
    source_id: Option<i64>,
    q: Option<String>,
    filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<ArticleSummary>, String> {
    if let Some(query) = q {
        return state.articles.search(&query).map_err(|e| e.to_string());
    }
    match filter.as_deref() {
        Some("unread") => state.articles.list_unread().map_err(|e| e.to_string()),
        Some("starred") => state.articles.list_starred().map_err(|e| e.to_string()),
        Some("recent") => state.articles.list_recent(7).map_err(|e| e.to_string()),
        Some(other) => Err(format!("filtro desconocido: {other}")),
        None => match source_id {
            Some(id) => state.articles.list_by_source(id).map_err(|e| e.to_string()),
            None => state.articles.list_all().map_err(|e| e.to_string()),
        },
    }
}

/// Lista los artículos sueltos (sin source).
#[tauri::command]
pub fn list_single_articles(state: State<'_, AppState>) -> Result<Vec<ArticleSummary>, String> {
    state.articles.list_unassigned().map_err(|e| e.to_string())
}

/// Lista los artículos de todos los sources con una categoría concreta.
#[tauri::command]
pub fn list_category_articles(
    category: String,
    state: State<'_, AppState>,
) -> Result<Vec<ArticleSummary>, String> {
    state
        .articles
        .list_by_category(&category)
        .map_err(|e| e.to_string())
}

/// Devuelve un artículo completo por id.
#[tauri::command]
pub fn get_article(id: i64, state: State<'_, AppState>) -> Result<Article, String> {
    state
        .articles
        .get(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Artículo {id} no encontrado"))
}

/// Marca un artículo como leído / no leído.
#[tauri::command]
pub fn mark_read(id: i64, read: bool, state: State<'_, AppState>) -> Result<(), String> {
    state.articles.mark_read(id, read).map_err(|e| e.to_string())
}

/// Marca como leídos los artículos del alcance indicado (`ReadScope`): toda la
/// biblioteca, un source, una categoría o un smart feed. Devuelve el número de
/// artículos cambiados. Para smart feeds en modo `vector`/`hybrid` marca los
/// resultados de la búsqueda correspondiente.
#[tauri::command]
pub async fn mark_all_read(scope: ReadScope, state: State<'_, AppState>) -> Result<usize, String> {
    if let ReadScope::SmartFeed { id } = &scope {
        let feed = state
            .smart_feeds
            .get(*id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("smart feed {id} no encontrado"))?;
        if feed.search_mode == SearchMode::Vector || feed.search_mode == SearchMode::Hybrid {
            let embedder = get_embedder(&state).await?;
            let query = truncate_to_tokens(&feed.query, reader_embeddings::DEFAULT_MAX_TOKENS);
            let vec = embedder.embed(&query).await.map_err(|e| e.to_string())?;
            let mut ids: Vec<i64> = Vec::new();
            let vector = state
                .smart_feeds
                .search_vector(&vec, 1000, similarity_threshold(&state)? as f32)
                .map_err(|e| e.to_string())?;
            ids.extend(vector.iter().map(|a| a.id));
            if feed.search_mode == SearchMode::Hybrid {
                let bm25 = state
                    .smart_feeds
                    .get_articles(&feed.query)
                    .unwrap_or_default();
                ids.extend(bm25.iter().map(|a| a.id));
            }
            ids.sort_unstable();
            ids.dedup();
            return state.articles.mark_read_ids(&ids).map_err(|e| e.to_string());
        }
    }
    state
        .articles
        .mark_all_read(&scope)
        .map_err(|e| e.to_string())
}

/// Renombra un source.
#[tauri::command]
pub fn rename_source(id: i64, title: String, state: State<'_, AppState>) -> Result<(), String> {
    state.sources.rename(id, &title).map_err(|e| e.to_string())
}

/// Borra un source. Sus artículos quedan como "sueltos" (`source_id = NULL`).
#[tauri::command]
pub fn delete_source(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    state.sources.delete(id).map_err(|e| e.to_string())
}

/// Conmuta el destacado (star) de un artículo.
#[tauri::command]
pub fn toggle_star(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    state.articles.toggle_star(id).map_err(|e| e.to_string())
}

/// Borra un artículo.
#[tauri::command]
pub fn delete_article(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    state.articles.delete(id).map_err(|e| e.to_string())
}

/// Re-descarga el feed de un source y devuelve el nº de artículos nuevos.
#[tauri::command]
pub async fn refresh_source(id: i64, state: State<'_, AppState>) -> Result<usize, String> {
    pipeline(&state).refresh_source(id).await.map_err(|e| e.to_string())
}

/// Re-descarga todos los sources con feed y devuelve el total de artículos nuevos.
#[tauri::command]
pub async fn refresh_all_sources(state: State<'_, AppState>) -> Result<usize, String> {
    pipeline(&state).refresh_all().await.map_err(|e| e.to_string())
}

/// Devuelve el intervalo de refresco automático en minutos (default 30).
#[tauri::command]
pub fn get_refresh_interval(state: State<'_, AppState>) -> Result<i64, String> {
    let value = state
        .settings
        .get("refresh_interval_minutes")
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "30".to_string());
    value
        .parse::<i64>()
        .map_err(|e| format!("intervalo inválido: {e}"))
}

/// Guarda el intervalo de refresco automático en minutos.
#[tauri::command]
pub fn set_refresh_interval(minutes: i64, state: State<'_, AppState>) -> Result<(), String> {
    if minutes <= 0 {
        return Err("el intervalo debe ser mayor que 0".into());
    }
    state
        .settings
        .set("refresh_interval_minutes", &minutes.to_string())
        .map_err(|e| e.to_string())
}

/// Clave de la purga automática de contenido extraído.
const PURGE_DAYS_KEY: &str = "purge_extracted_days";

/// Devuelve los días tras los que se vacía automáticamente el contenido
/// extraído de los artículos leídos (0 = nunca). Default 0.
#[tauri::command]
pub fn get_content_purge_days(state: State<'_, AppState>) -> Result<i64, String> {
    let value = state
        .settings
        .get(PURGE_DAYS_KEY)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "0".to_string());
    value
        .parse::<i64>()
        .map_err(|e| format!("días de purga inválidos: {e}"))
}

/// Guarda los días de purga automática (0 = nunca; debe ser >= 0).
#[tauri::command]
pub fn set_content_purge_days(days: i64, state: State<'_, AppState>) -> Result<(), String> {
    if days < 0 {
        return Err("los días de purga no pueden ser negativos".into());
    }
    state
        .settings
        .set(PURGE_DAYS_KEY, &days.to_string())
        .map_err(|e| e.to_string())
}

/// Vacía el contenido extraído de los artículos de feed ya leídos, volviéndolos
/// a su resumen original y borrando sus embeddings. Con `days > 0` solo se
/// purgan los artículos con `fetched_at` anterior a `days`. Devuelve el nº de
/// artículos purgados.
#[tauri::command]
pub fn purge_extracted_content(days: i64, state: State<'_, AppState>) -> Result<usize, String> {
    if days < 0 {
        return Err("los días de purga no pueden ser negativos".into());
    }
    state
        .articles
        .purge_extracted_content(days)
        .map_err(|e| e.to_string())
}

/// Clave del umbral de similitud de la búsqueda semántica.
const SIMILARITY_THRESHOLD_KEY: &str = "vector_similarity_threshold";
/// Umbral por defecto si no está en settings.
const SIMILARITY_THRESHOLD_DEFAULT: f64 = 0.7;

/// Lee el umbral de similitud coseno mínima para la búsqueda semántica (0.0–1.0).
fn similarity_threshold(state: &AppState) -> Result<f64, String> {
    let value = state
        .settings
        .get(SIMILARITY_THRESHOLD_KEY)
        .map_err(|e| e.to_string())?
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(SIMILARITY_THRESHOLD_DEFAULT);
    Ok(value.clamp(0.0, 1.0))
}

/// Devuelve el umbral de similitud de la búsqueda semántica (default 0.7).
#[tauri::command]
pub fn get_vector_similarity_threshold(state: State<'_, AppState>) -> Result<f64, String> {
    similarity_threshold(&state)
}

/// Valida que un umbral esté en [0.0, 1.0].
fn validate_threshold(threshold: f64) -> Result<(), String> {
    if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
        return Err("el umbral debe estar entre 0.0 y 1.0".into());
    }
    Ok(())
}

/// Guarda el umbral en settings (lógica interna testeable).
fn set_vector_similarity_threshold_inner(state: &AppState, threshold: f64) -> Result<(), String> {
    validate_threshold(threshold)?;
    state
        .settings
        .set(SIMILARITY_THRESHOLD_KEY, &threshold.to_string())
        .map_err(|e| e.to_string())
}

/// Guarda el umbral de similitud de la búsqueda semántica (0.0–1.0).
#[tauri::command]
pub fn set_vector_similarity_threshold(
    threshold: f64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    set_vector_similarity_threshold_inner(&state, threshold)
}

// --- Ajustes de apariencia y lectura (persistidos en la tabla `settings`) ---

/// Clave del tema de la interfaz.
const THEME_KEY: &str = "theme";

fn valid_theme(t: &str) -> bool {
    matches!(t, "system" | "light" | "dark" | "sepia")
}

/// Devuelve el tema de la interfaz ("system" | "light" | "dark" | "sepia").
#[tauri::command]
pub fn get_theme(state: State<'_, AppState>) -> Result<String, String> {
    let theme = state
        .settings
        .get(THEME_KEY)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "system".to_string());
    Ok(if valid_theme(&theme) { theme } else { "system".to_string() })
}

/// Guarda el tema de la interfaz ("system" | "light" | "dark" | "sepia").
#[tauri::command]
pub fn set_theme(theme: String, state: State<'_, AppState>) -> Result<(), String> {
    if !valid_theme(&theme) {
        return Err(format!("tema inválido: {theme}"));
    }
    state
        .settings
        .set(THEME_KEY, &theme)
        .map_err(|e| e.to_string())
}

/// Lee los ajustes de lectura desde settings aplicando los valores por defecto.
fn read_reader_settings(state: &AppState) -> Result<ReaderSettings, String> {
    let get = |key: &str, default: &str| -> Result<String, String> {
        Ok(state
            .settings
            .get(key)
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| default.to_string()))
    };
    Ok(ReaderSettings {
        font_size: get("reader_font_size", "19")?
            .parse::<i64>()
            .map(|v| v.clamp(14, 28))
            .unwrap_or(19),
        font_family: get("reader_font_family", "serif")?,
        line_height: get("reader_line_height", "normal")?,
        width: get("reader_width", "medium")?,
        show_snippets: get("show_snippets", "true")? != "false",
    })
}

/// Devuelve los ajustes de lectura del lector (tipografía, ancho, etc.).
#[tauri::command]
pub fn get_reader_settings(state: State<'_, AppState>) -> Result<ReaderSettings, String> {
    read_reader_settings(&state)
}

fn validate_reader_settings(rs: &ReaderSettings) -> Result<(), String> {
    if !matches!(rs.font_family.as_str(), "serif" | "sans" | "mono") {
        return Err(format!("familia tipográfica inválida: {}", rs.font_family));
    }
    if !matches!(rs.line_height.as_str(), "compact" | "normal" | "relaxed") {
        return Err(format!("interlineado inválido: {}", rs.line_height));
    }
    if !matches!(rs.width.as_str(), "narrow" | "medium" | "wide") {
        return Err(format!("ancho inválido: {}", rs.width));
    }
    Ok(())
}

/// Valida y guarda los ajustes de lectura (lógica interna testeable).
fn set_reader_settings_inner(state: &AppState, settings: &ReaderSettings) -> Result<(), String> {
    validate_reader_settings(settings)?;
    let font_size = settings.font_size.clamp(14, 28);
    let set = |key: &str, value: &str| {
        state
            .settings
            .set(key, value)
            .map_err(|e| e.to_string())
    };
    set("reader_font_size", &font_size.to_string())?;
    set("reader_font_family", &settings.font_family)?;
    set("reader_line_height", &settings.line_height)?;
    set("reader_width", &settings.width)?;
    set("show_snippets", &settings.show_snippets.to_string())
}

/// Guarda los ajustes de lectura del lector.
#[tauri::command]
pub fn set_reader_settings(settings: ReaderSettings, state: State<'_, AppState>) -> Result<(), String> {
    set_reader_settings_inner(&state, &settings)
}

// --- Importación / exportación de fuentes en OPML ---

/// Exporta todas las fuentes a un archivo OPML en `path`. Devuelve el nº de
/// fuentes exportadas.
#[tauri::command]
pub fn export_opml(path: String, state: State<'_, AppState>) -> Result<usize, String> {
    let sources = state.sources.list().map_err(|e| e.to_string())?;
    let xml = crate::opml::export_opml_xml(&sources);
    std::fs::write(&path, xml).map_err(|e| format!("No se pudo escribir el archivo: {e}"))?;
    Ok(sources.len())
}

/// Importa fuentes desde un archivo OPML en `path`. Las fuentes nuevas se
/// insertan y las ya existentes (mismo `xmlUrl`) se actualizan. Devuelve el
/// nº de fuentes importadas.
#[tauri::command]
pub fn import_opml(path: String, state: State<'_, AppState>) -> Result<usize, String> {
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("No se pudo leer el archivo: {e}"))?;
    let feeds = crate::opml::parse_opml(&content)?;
    let now = reader_pipeline::utc_now();
    let mut imported = 0usize;
    for feed in feeds {
        let home_url = feed.html_url.clone().unwrap_or_else(|| crate::host_of(&feed.xml_url));
        let source = reader_domain::Source {
            id: 0,
            url: feed.xml_url.clone(),
            home_url,
            title: feed.title,
            description: None,
            feed_url: Some(feed.xml_url),
            last_fetched_at: Some(now.clone()),
            last_error: None,
            last_status: None,
            error_count: 0,
            category: feed.category,
        };
        state.sources.upsert(&source).map_err(|e| e.to_string())?;
        imported += 1;
    }
    Ok(imported)
}

/// Lista las categorías únicas de todos los sources.
#[tauri::command]
pub fn list_categories(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.sources.list_categories().map_err(|e| e.to_string())
}

/// Asigna una categoría a un source (o None para quitarla).
#[tauri::command]
pub fn set_category(id: i64, category: Option<String>, state: State<'_, AppState>) -> Result<(), String> {
    state
        .sources
        .set_category(id, category.as_deref())
        .map_err(|e| e.to_string())
}

/// Elimina una categoría: los sources que la tenían quedan sin categoría
/// (pasan a "Sin categoría"). No borra los sources. Devuelve cuántos se tocaron.
#[tauri::command]
pub fn delete_category(name: String, state: State<'_, AppState>) -> Result<usize, String> {
    state
        .sources
        .clear_category(&name)
        .map_err(|e| e.to_string())
}

/// Lista todos los smart feeds.
#[tauri::command]
pub fn list_smart_feeds(state: State<'_, AppState>) -> Result<Vec<SmartFeed>, String> {
    state.smart_feeds.list().map_err(|e| e.to_string())
}

/// Crea un nuevo smart feed. `search_mode` puede ser "bm25", "vector" o
/// "hybrid".
#[tauri::command]
pub fn create_smart_feed(
    name: String,
    query: String,
    search_mode: String,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    let mode = SearchMode::from_str(&search_mode);
    let now = reader_pipeline::utc_now();
    state
        .smart_feeds
        .create(&name, &query, mode.as_str(), &now)
        .map_err(|e| e.to_string())
}

/// Borra un smart feed.
#[tauri::command]
pub fn delete_smart_feed(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    state.smart_feeds.delete(id).map_err(|e| e.to_string())
}

/// Devuelve los artículos de un smart feed, respetando su modo de búsqueda:
/// `bm25` (FTS5), `vector` (KNN) o `hybrid` (fusión de ambos con RRF).
#[tauri::command]
pub async fn get_smart_feed_articles(
    id: i64,
    state: State<'_, AppState>,
) -> Result<Vec<ArticleSummary>, String> {
    let smart_feed = state
        .smart_feeds
        .get(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("smart feed {id} no encontrado"))?;
    match smart_feed.search_mode {
        SearchMode::Bm25 => state
            .smart_feeds
            .get_articles(&smart_feed.query)
            .map_err(|e| e.to_string()),
        SearchMode::Vector => {
            let embedder = get_embedder(&state).await?;
            let query = truncate_to_tokens(&smart_feed.query, reader_embeddings::DEFAULT_MAX_TOKENS);
            let vec = embedder.embed(&query).await.map_err(|e| e.to_string())?;
            state
                .smart_feeds
                .search_vector(&vec, 50, similarity_threshold(&state)? as f32)
                .map_err(|e| e.to_string())
        }
        SearchMode::Hybrid => {
            let bm25 = state
                .smart_feeds
                .get_articles(&smart_feed.query)
                .unwrap_or_default();
            let embedder = get_embedder(&state).await?;
            let query = truncate_to_tokens(&smart_feed.query, reader_embeddings::DEFAULT_MAX_TOKENS);
            let vec = embedder.embed(&query).await.map_err(|e| e.to_string())?;
            let vector = state
                .smart_feeds
                .search_vector(&vec, 50, similarity_threshold(&state)? as f32)
                .unwrap_or_default();
            Ok(rrf_fuse(bm25, vector, 50))
        }
    }
}

/// Genera el embedding del contenido de un artículo (si tiene texto).
#[tauri::command]
pub async fn generate_embedding(article_id: i64, state: State<'_, AppState>) -> Result<(), String> {
    generate_embedding_inner(article_id, &state).await
}

/// Lógica de `generate_embedding`, desacoplada de Tauri para poder reutilizarla
/// desde otros comandos y tests.
async fn generate_embedding_inner(article_id: i64, state: &AppState) -> Result<(), String> {
    let article = state
        .articles
        .get(article_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("artículo {article_id} no encontrado"))?;
    let text = truncate_to_tokens(&article.text, reader_embeddings::DEFAULT_MAX_TOKENS);
    if text.trim().is_empty() {
        return Err("el artículo no tiene contenido extraído que embedar".into());
    }
    let embedder = get_embedder(state).await?;
    let vec = embedder.embed(&text).await.map_err(|e| e.to_string())?;
    let now = reader_pipeline::utc_now();
    state
        .embeddings
        .upsert(article_id, &vec, embedder.model_name(), text.chars().count(), &now)
        .map_err(|e| e.to_string())
}

/// Borra el embedding de un artículo y lo regenera desde su contenido actual.
#[tauri::command]
pub async fn regenerate_embedding(
    article_id: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .embeddings
        .delete(article_id)
        .map_err(|e| e.to_string())?;
    generate_embedding_inner(article_id, &state).await
}

/// Genera los embeddings que falten para todos los artículos con texto.
/// Devuelve el nº de embeddings generados. Procesa en lotes para no saturar
/// el modelo.
#[tauri::command]
pub async fn generate_all_embeddings(state: State<'_, AppState>) -> Result<usize, String> {
    let embedder = get_embedder(&state).await?;
    embed_missing_articles(&state.articles, &state.embeddings, &embedder).await
}

/// Lógica de `generate_all_embeddings`, desacoplada de Tauri para poder
/// testearla con un embedder mock.
pub(crate) async fn embed_missing_articles(
    articles: &dyn ArticleRepository,
    embeddings: &dyn EmbeddingRepository,
    embedder: &dyn EmbeddingGenerator,
) -> Result<usize, String> {
    let now = reader_pipeline::utc_now();
    let mut generated = 0usize;
    const BATCH: i64 = 32;
    loop {
        let ids = embeddings
            .articles_without_embedding(BATCH)
            .map_err(|e| e.to_string())?;
        if ids.is_empty() {
            break;
        }
        let mut texts = Vec::with_capacity(ids.len());
        let mut embed_ids = Vec::with_capacity(ids.len());
        for id in &ids {
            match articles.get(*id).map_err(|e| e.to_string())? {
                Some(a) => {
                    let t = truncate_to_tokens(&a.text, reader_embeddings::DEFAULT_MAX_TOKENS);
                    if t.trim().is_empty() {
                        continue;
                    }
                    embed_ids.push(*id);
                    texts.push(t);
                }
                None => continue,
            }
        }
        // `vecs` se alinea con `embed_ids` (no con `ids`): solo se embedan los
        // artículos con texto, y cada vector corresponde al id de su texto.
        if !embed_ids.is_empty() {
            let vecs = embedder
                .embed_batch(&texts)
                .await
                .map_err(|e| e.to_string())?;
            for (id, vec) in embed_ids.iter().zip(vecs) {
                embeddings
                    .upsert(*id, &vec, embedder.model_name(), 512, &now)
                    .map_err(|e| e.to_string())?;
                generated += 1;
            }
        }
        if ids.len() < BATCH as usize {
            break;
        }
    }
    Ok(generated)
}

/// Devuelve el nº de artículos con embedding y el total de artículos.
#[tauri::command]
pub fn get_embedding_status(state: State<'_, AppState>) -> Result<(i64, i64), String> {
    let (embedded, _) = state.embeddings.count_embedded().map_err(|e| e.to_string())?;
    let total = state.articles.list_all().map_err(|e| e.to_string())?.len() as i64;
    Ok((embedded, total))
}

fn pipeline(state: &AppState) -> Pipeline<'_> {
    Pipeline {
        http: &state.http,
        extractor: &state.extractor,
        discoverer: &state.discoverer,
        parser: &state.parser,
        articles: &state.articles,
        sources: &state.sources,
    }
}

/// Fusión de dos rankings con Reciprocal Rank Fusion (RRF).
///
/// `score(d) = Σ 1/(k + rank_i(d))` con k=60. Mantiene el orden del
/// documento con mejor score fusionado y recorta a `limit`. Los empates se
/// deshacen por el mejor rank en la primera lista (BM25) y luego por id,
/// para que el resultado sea determinista.
fn rrf_fuse(
    a: Vec<ArticleSummary>,
    b: Vec<ArticleSummary>,
    limit: usize,
) -> Vec<ArticleSummary> {
    const K: f64 = 60.0;
    // id -> (score, mejor rank en `a` si aparece, artículo)
    let mut scores: std::collections::HashMap<i64, (f64, usize, ArticleSummary)> =
        std::collections::HashMap::new();

    for (rank, item) in a.into_iter().enumerate() {
        let entry = scores
            .entry(item.id)
            .or_insert_with(|| (0.0, usize::MAX, item.clone()));
        entry.0 += 1.0 / (K + rank as f64 + 1.0);
        entry.1 = entry.1.min(rank);
    }
    for (rank, item) in b.into_iter().enumerate() {
        let entry = scores
            .entry(item.id)
            .or_insert_with(|| (0.0, usize::MAX, item.clone()));
        entry.0 += 1.0 / (K + rank as f64 + 1.0);
    }

    let mut items: Vec<(f64, usize, i64, ArticleSummary)> = scores
        .into_iter()
        .map(|(id, (score, best_a_rank, item))| (score, best_a_rank, id, item))
        .collect();
    items.sort_by(|x, y| {
        y.0.partial_cmp(&x.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| x.1.cmp(&y.1))
            .then_with(|| x.2.cmp(&y.2))
    });
    items
        .into_iter()
        .take(limit)
        .map(|(_, _, _, item)| item)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reader_embeddings::EmbeddingError;

    /// Mock de `EmbeddingGenerator`: devuelve un vector 384-dim cuya primera
    /// componente codifica la longitud del texto, para poder comprobar que cada
    /// vector termina en el artículo correcto.
    struct MockEmbedder;

    #[async_trait::async_trait]
    impl EmbeddingGenerator for MockEmbedder {
        async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
            Ok(vec![text.len() as f32; 384])
        }

        async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            Ok(texts.iter().map(|t| vec![t.len() as f32; 384]).collect())
        }

        fn dimensions(&self) -> usize {
            384
        }

        fn model_name(&self) -> &str {
            "mock"
        }
    }

    fn summary(id: i64, title: &str) -> ArticleSummary {
        ArticleSummary {
            id,
            source_id: None,
            source_title: None,
            url: format!("https://a.com/{id}"),
            title: title.to_string(),
            site_name: None,
            published_at: None,
            fetched_at: "2024-01-01T00:00:00Z".to_string(),
            read: false,
            starred: false,
            snippet: None,
            has_embedding: false,
        }
    }

    #[test]
    fn rrf_ranks_documents_in_both_lists_first() {
        let bm25 = vec![summary(1, "uno"), summary(2, "dos"), summary(3, "tres")];
        let vector = vec![summary(3, "tres"), summary(4, "cuatro"), summary(1, "uno")];
        let fused = rrf_fuse(bm25, vector, 10);
        // Los ids 1 y 3 están en ambas listas → deberían liderar la fusión.
        assert_eq!(fused[0].id, 1);
        assert_eq!(fused[1].id, 3);
        assert!(fused.iter().any(|a| a.id == 2));
        assert!(fused.iter().any(|a| a.id == 4));
    }

    #[test]
    fn rrf_respects_limit_and_dedupes() {
        let bm25 = vec![summary(1, "uno"), summary(2, "dos")];
        let vector = vec![summary(2, "dos")];
        let fused = rrf_fuse(bm25, vector, 1);
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].id, 2);
    }

    fn test_state() -> AppState {
        let conn = reader_storage::open_db_in_memory().unwrap();
        AppState::new(std::sync::Arc::new(std::sync::Mutex::new(conn)))
    }

    #[test]
    fn similarity_threshold_defaults_to_0_7() {
        let state = test_state();
        assert_eq!(similarity_threshold(&state).unwrap(), 0.7);
    }

    #[test]
    fn set_and_get_similarity_threshold_roundtrip() {
        let state = test_state();
        set_vector_similarity_threshold_inner(&state, 0.55).unwrap();
        assert_eq!(similarity_threshold(&state).unwrap(), 0.55);
    }

    #[test]
    fn set_similarity_threshold_rejects_out_of_range() {
        let state = test_state();
        assert!(validate_threshold(1.5).is_err());
        assert!(validate_threshold(-0.1).is_err());
        assert!(validate_threshold(f64::NAN).is_err());
        // El valor válido previo se conserva (el fallo no lo corrompe).
        set_vector_similarity_threshold_inner(&state, 0.8).unwrap();
        assert!(validate_threshold(2.0).is_err());
        assert_eq!(similarity_threshold(&state).unwrap(), 0.8);
    }

    #[test]
    fn similarity_threshold_clamps_stored_out_of_range_values() {
        let state = test_state();
        state
            .settings
            .set(SIMILARITY_THRESHOLD_KEY, "2.5")
            .unwrap();
        assert_eq!(similarity_threshold(&state).unwrap(), 1.0);
        state
            .settings
            .set(SIMILARITY_THRESHOLD_KEY, "-3.0")
            .unwrap();
        assert_eq!(similarity_threshold(&state).unwrap(), 0.0);
    }

    #[test]
    fn theme_defaults_to_system_and_validates() {
        let state = test_state();
        assert_eq!(read_reader_settings(&state).is_ok(), true);
        let theme = state.settings.get(THEME_KEY).unwrap().unwrap();
        assert_eq!(theme, "system");
        assert!(valid_theme("sepia"));
        assert!(!valid_theme("neon"));
    }

    #[test]
    fn reader_settings_roundtrip_and_clamp() {
        let state = test_state();
        let rs = ReaderSettings {
            font_size: 42, // se clampea a 28 al guardar
            font_family: "mono".into(),
            line_height: "relaxed".into(),
            width: "narrow".into(),
            show_snippets: false,
        };
        set_reader_settings_inner(&state, &rs).unwrap();        let got = read_reader_settings(&state).unwrap();
        assert_eq!(got.font_size, 28);
        assert_eq!(got.font_family, "mono");
        assert_eq!(got.line_height, "relaxed");
        assert_eq!(got.width, "narrow");
        assert!(!got.show_snippets);
    }

    #[test]
    fn reader_settings_reject_invalid_values() {
        let bad = ReaderSettings {
            font_size: 19,
            font_family: "papyrus".into(),
            line_height: "normal".into(),
            width: "medium".into(),
            show_snippets: true,
        };
        assert!(validate_reader_settings(&bad).is_err());
    }

    #[test]
    fn opml_export_import_roundtrip() {
        use reader_domain::Source;
        let state = test_state();
        let now = reader_pipeline::utc_now();
        for (i, title) in ["Rust Blog", "Noticias"].iter().enumerate() {
            let home = format!("https://site{i}.com/");
            let feed = format!("https://site{i}.com/feed.xml");
            state
                .sources
                .upsert(&Source {
                    id: 0,
                    url: feed.clone(),
                    home_url: home.clone(),
                    title: title.to_string(),
                    description: None,
                    feed_url: Some(feed),
                    last_fetched_at: Some(now.clone()),
                    last_error: None,
                    last_status: None,
                    error_count: 0,
                    category: Some("Tecnología".into()),
                })
                .unwrap();
        }

        let sources = state.sources.list().unwrap();
        let xml = crate::opml::export_opml_xml(&sources);
        let feeds = crate::opml::parse_opml(&xml).unwrap();
        assert_eq!(feeds.len(), 2);
        assert!(feeds.iter().all(|f| f.category.as_deref() == Some("Tecnología")));
        let urls: Vec<&str> = feeds.iter().map(|f| f.xml_url.as_str()).collect();
        assert!(urls.contains(&"https://site0.com/feed.xml"));
        assert!(urls.contains(&"https://site1.com/feed.xml"));
    }

    fn art(url: &str, text: &str) -> Article {
        Article {
            id: 0,
            source_id: None,
            url: url.to_string(),
            title: url.to_string(),
            html: format!("<p>{text}</p>"),
            text: text.to_string(),
            raw_html: String::new(),
            byline: None,
            site_name: None,
            published_at: None,
            fetched_at: "2024-01-01T00:00:00Z".to_string(),
            read: false,
            starred: false,
            has_embedding: false,
        }
    }

    #[tokio::test]
    async fn embed_missing_articles_skips_empty_text_and_matches_ids() {
        // Regression del bug de `zip`: con un artículo de texto vacío en medio,
        // el vector se asignaba al id equivocado y el último artículo se quedaba
        // sin embedding (aunque "generar todos" dijera que no quedaba nada).
        let state = test_state();
        let a1 = state.articles.upsert(&art("https://a.com/1", "texto corto")).unwrap();
        let a2 = state.articles.upsert(&art("https://a.com/2", "")).unwrap();
        let a3 = state.articles.upsert(&art("https://a.com/3", "texto bastante mas largo")).unwrap();

        let generated = embed_missing_articles(&state.articles, &state.embeddings, &MockEmbedder)
            .await
            .unwrap();
        assert_eq!(generated, 2);

        // a1: embedding con la longitud de su propio texto.
        let e1 = state.embeddings.get(a1).unwrap().unwrap();
        assert_eq!(e1[0], 11.0);
        // a2: sin texto → sin embedding.
        assert!(state.embeddings.get(a2).unwrap().is_none());
        // a3: embedding con la longitud de su propio texto (el bug lo dejaba sin).
        let e3 = state.embeddings.get(a3).unwrap().unwrap();
        assert_eq!(e3[0], 24.0);

        // Una segunda pasada no genera nada (ya no quedan artículos con texto).
        let again = embed_missing_articles(&state.articles, &state.embeddings, &MockEmbedder)
            .await
            .unwrap();
        assert_eq!(again, 0);
    }
}
