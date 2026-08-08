//! Modelos puros del dominio de Lector.
//!
//! Este crate NO tiene dependencias externas de infraestructura (DB, HTTP,
//! parser). Define únicamente las estructuras de datos que comparten los
//! adaptadores y el pipeline, todas serializables para viajar por los
//! comandos Tauri hacia el frontend.

use serde::{Deserialize, Serialize};

/// Un artículo guardado en la biblioteca.
///
/// `source_id = None` indica un "artículo suelto" (single article) que no
/// pertenece a ningún feed descubierto.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Article {
    pub id: i64,
    /// Source al que pertenece; `None` = artículo suelto.
    pub source_id: Option<i64>,
    /// URL canónica.
    pub url: String,
    pub title: String,
    /// HTML estilizado listo para renderizar en el lector.
    pub html: String,
    /// Texto plano (para búsqueda FTS5 y, en el futuro, IA).
    pub text: String,
    /// HTML crudo guardado aparte (anti-404 + futuro troceado/embeddings).
    pub raw_html: String,
    pub byline: Option<String>,
    pub site_name: Option<String>,
    pub published_at: Option<String>,
    pub fetched_at: String,
    pub read: bool,
    pub starred: bool,
    /// True si el artículo tiene un embedding vectorial generado.
    pub has_embedding: bool,
}

/// Resumen de un artículo para listados (sin el HTML completo).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArticleSummary {
    pub id: i64,
    pub source_id: Option<i64>,
    pub source_title: Option<String>,
    pub url: String,
    pub title: String,
    pub site_name: Option<String>,
    pub published_at: Option<String>,
    pub fetched_at: String,
    pub read: bool,
    pub starred: bool,
    /// Fragmento coincidente cuando la lista proviene de búsqueda FTS5.
    pub snippet: Option<String>,
    /// True si el artículo tiene un embedding vectorial generado.
    pub has_embedding: bool,
}

/// Un sitio/feed guardado.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub id: i64,
    /// URL del feed (o del sitio si no tiene feed).
    pub url: String,
    pub home_url: String,
    pub title: String,
    pub description: Option<String>,
    /// Feed descubierto (si existe).
    pub feed_url: Option<String>,
    pub last_fetched_at: Option<String>,
    /// Mensaje de error del último refresh (None si OK).
    pub last_error: Option<String>,
    /// Código HTTP del último refresh (None si no aplica).
    pub last_status: Option<i64>,
    /// Nº de fallos consecutivos.
    pub error_count: i64,
    /// Categoría del source (None si no tiene).
    pub category: Option<String>,
}

/// Resumen de un source con conteo de artículos, para el sidebar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceSummary {
    pub id: i64,
    pub url: String,
    pub home_url: String,
    pub title: String,
    pub description: Option<String>,
    pub feed_url: Option<String>,
    pub last_fetched_at: Option<String>,
    pub article_count: i64,
    pub unread_count: i64,
    /// Mensaje de error del último refresh (None si OK).
    pub last_error: Option<String>,
    /// Nº de fallos consecutivos.
    pub error_count: i64,
    /// Categoría del source (None si no tiene).
    pub category: Option<String>,
}

/// Resultado de parsear un feed (temporal; no se persiste como tal).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedEntry {
    pub title: String,
    pub link: String,
    pub summary: Option<String>,
    pub published: Option<String>,
}

/// Modo de búsqueda de un smart feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    /// Búsqueda por palabras clave (FTS5, ranking BM25).
    #[default]
    Bm25,
    /// Búsqueda semántica por embeddings (similitud de vectores).
    Vector,
    /// Combinación de BM25 + vector (Reciprocal Rank Fusion).
    Hybrid,
}

impl SearchMode {
    /// Parsea un modo desde su representación de texto (columna `search_mode`).
    pub fn from_str(s: &str) -> Self {
        match s {
            "vector" => Self::Vector,
            "hybrid" => Self::Hybrid,
            _ => Self::Bm25,
        }
    }

    /// Representación de texto que se guarda en la columna `search_mode`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bm25 => "bm25",
            Self::Vector => "vector",
            Self::Hybrid => "hybrid",
        }
    }
}

/// Un smart feed (búsqueda guardada).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartFeed {
    pub id: i64,
    pub name: String,
    pub query: String,
    pub created_at: String,
    /// Modo de búsqueda: por palabras clave, semántico o híbrido.
    pub search_mode: SearchMode,
    pub article_count: i64,
    pub unread_count: i64,
}

/// Alcance de "marcar todo leído": qué conjunto de artículos se marca.
///
/// Serializado como objeto con tag `kind` (internally-tagged), de modo que el
/// frontend lo pasa de forma natural desde su modelo de vistas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReadScope {
    /// Toda la biblioteca.
    All,
    /// Los artículos de un source concreto.
    Source { id: i64 },
    /// Los artículos de todos los sources con una categoría.
    Category { name: String },
    /// Los artículos que coinciden con la consulta de un smart feed.
    SmartFeed { id: i64 },
}

/// Ajustes de lectura y apariencia del panel de configuración.
///
/// Se persisten en la tabla `settings` como pares clave-valor y viajan como un
/// solo objeto por los comandos Tauri. `font_family` vale "serif" | "sans" |
/// "mono"; `line_height` "compact" | "normal" | "relaxed"; `width` "narrow" |
/// "medium" | "wide".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReaderSettings {
    /// Tamaño de la fuente del cuerpo del lector, en px (14–28).
    pub font_size: i64,
    pub font_family: String,
    pub line_height: String,
    /// Ancho de la columna de lectura.
    pub width: String,
    /// Mostrar el snippet de los artículos en la lista.
    pub show_snippets: bool,
}

/// Resultado de ingerir un URL: el source (si lo hay) y los artículos creados.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngestResult {
    pub source: Option<SourceSummary>,
    /// Id del artículo creado por la URL pegada. `None` si el URL no era un
    /// artículo (feed o página índice): entonces no se guardó ningún artículo.
    pub article_id: Option<i64>,
    pub article_title: String,
    /// Nº de posts del feed que se guardaron por primera vez.
    pub feed_articles_added: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_article() -> Article {
        Article {
            id: 1,
            source_id: None,
            url: "https://example.com/post".into(),
            title: "Título".into(),
            html: "<p>Hola</p>".into(),
            text: "Hola".into(),
            raw_html: "<html>...</html>".into(),
            byline: Some("Autor".into()),
            site_name: Some("Example".into()),
            published_at: Some("2024-01-01T00:00:00Z".into()),
            fetched_at: "2024-01-02T00:00:00Z".into(),
            read: false,
            starred: true,
            has_embedding: false,
        }
    }

    #[test]
    fn article_serde_roundtrip() {
        let article = sample_article();
        let json = serde_json::to_string(&article).unwrap();
        let back: Article = serde_json::from_str(&json).unwrap();
        assert_eq!(article, back);
    }

    #[test]
    fn source_summary_counts() {
        let summary = SourceSummary {
            id: 1,
            url: "https://example.com/feed.xml".into(),
            home_url: "https://example.com".into(),
            title: "Example".into(),
            description: None,
            feed_url: Some("https://example.com/feed.xml".into()),
            last_fetched_at: None,
            article_count: 12,
            unread_count: 4,
            last_error: None,
            error_count: 0,
            category: None,
        };
        let json = serde_json::to_string(&summary).unwrap();
        let back: SourceSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(back.article_count, 12);
        assert_eq!(back.unread_count, 4);
    }

    #[test]
    fn article_defaults_are_sane() {
        let a = sample_article();
        assert!(!a.read);
        assert!(a.starred);
        assert_eq!(a.source_id, None);
    }

    #[test]
    fn search_mode_roundtrip_and_default() {
        assert_eq!(SearchMode::default(), SearchMode::Bm25);
        assert_eq!(serde_json::to_string(&SearchMode::Vector).unwrap(), r#""vector""#);
        assert_eq!(serde_json::from_str::<SearchMode>(r#""hybrid""#).unwrap(), SearchMode::Hybrid);
        assert_eq!(SearchMode::from_str("vector"), SearchMode::Vector);
        assert_eq!(SearchMode::from_str("hybrid"), SearchMode::Hybrid);
        assert_eq!(SearchMode::from_str("bm25"), SearchMode::Bm25);
        assert_eq!(SearchMode::from_str("desconocido"), SearchMode::Bm25);
        assert_eq!(SearchMode::Vector.as_str(), "vector");
    }

    #[test]
    fn reader_settings_serde_roundtrip() {
        let rs = ReaderSettings {
            font_size: 22,
            font_family: "sans".into(),
            line_height: "relaxed".into(),
            width: "wide".into(),
            show_snippets: false,
        };
        let json = serde_json::to_string(&rs).unwrap();
        assert!(json.contains("\"show_snippets\":false"));
        let back: ReaderSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(rs, back);
    }

    #[test]
    fn read_scope_serde_roundtrip() {
        let scopes = [
            ReadScope::All,
            ReadScope::Source { id: 7 },
            ReadScope::Category { name: "Tecnología".into() },
            ReadScope::SmartFeed { id: 3 },
        ];
        for scope in scopes {
            let json = serde_json::to_string(&scope).unwrap();
            let back: ReadScope = serde_json::from_str(&json).unwrap();
            assert_eq!(scope, back);
        }
        // El tag es `kind` y los nombres van en camelCase, como los manda el frontend.
        assert_eq!(
            serde_json::to_string(&ReadScope::SmartFeed { id: 3 }).unwrap(),
            r#"{"kind":"smartFeed","id":3}"#
        );
        assert_eq!(
            serde_json::from_str::<ReadScope>(r#"{"kind":"source","id":7}"#).unwrap(),
            ReadScope::Source { id: 7 }
        );
    }
}
