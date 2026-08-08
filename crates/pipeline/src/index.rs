//! Detección de páginas índice: portadas/blogs que solo listan otros posts.
//!
//! Cuando el usuario pega la URL de un sitio (p. ej. `https://www.interconnects.ai/`),
//! la página no es un artículo sino un índice de posts. Guardarla como artículo
//! solo produce ruido (un listado de enlaces). Esta función decide si una página
//! es índice, combinando señales del HTML y de la URL:
//!
//! - Marcadores JSON-LD (`application/ld+json`): si hay un tipo "artículo"
//!   (`Article`, `NewsArticle`, `BlogPosting`, ...) la página es un artículo;
//!   si solo hay tipos de índice (`WebSite`, `WebPage`, `CollectionPage`, ...)
//!   es una portada.
//! - Fallback: si la URL es la raíz del sitio (`/`), se asume portada.
//!
//! Nota: `og:type` NO es fiable (Substack pone `article` hasta en la portada).

use url::Url;

/// Tipos JSON-LD que indican un artículo individual.
const ARTICLE_TYPES: &[&str] = &[
    "Article",
    "NewsArticle",
    "BlogPosting",
    "Report",
    "TechArticle",
    "ScholarlyArticle",
    "Review",
    "OpinionNewsArticle",
    "AnalysisNewsArticle",
];

/// Tipos JSON-LD que indican una página índice / portada.
const INDEX_TYPES: &[&str] = &[
    "WebSite",
    "WebPage",
    "CollectionPage",
    "ItemList",
    "Blog",
    "ProfilePage",
    "SearchResultsPage",
];

/// Decide si `html` (con la URL final `url`) es una página índice y no un artículo.
pub fn is_index_page(html: &str, url: &Url) -> bool {
    if jsonld_has_any(html, ARTICLE_TYPES) {
        return false;
    }
    if jsonld_has_any(html, INDEX_TYPES) {
        return true;
    }
    is_root_path(url)
}

/// `true` si el path de la URL es la raíz del sitio (`/`, `/index.html`, ...).
fn is_root_path(url: &Url) -> bool {
    matches!(
        url.path(),
        "" | "/" | "/index.html" | "/index.htm" | "/index.php"
    )
}

/// `true` si algún bloque `application/ld+json` contiene uno de `types`.
fn jsonld_has_any(html: &str, types: &[&str]) -> bool {
    for block in jsonld_blocks(html) {
        for ty in types {
            if block.contains(&format!("\"@type\":\"{ty}\"")) {
                return true;
            }
        }
    }
    false
}

/// Devuelve el contenido de cada `<script type="application/ld+json">...</script>`.
fn jsonld_blocks<'a>(html: &'a str) -> Vec<&'a str> {
    let open = "<script type=\"application/ld+json\">";
    let close = "</script>";
    let mut blocks = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find(open) {
        let after = &rest[start + open.len()..];
        let Some(end) = after.find(close) else {
            break;
        };
        blocks.push(&after[..end]);
        rest = &after[end + close.len()..];
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    const HOME_JSONLD: &str = r#"<html><head>
        <script type="application/ld+json">{"@type":"WebSite","mainEntityOfPage":{"@type":"WebPage"}}</script>
    </head></html>"#;

    const ARTICLE_JSONLD: &str = r#"<html><head>
        <script type="application/ld+json">{"@type":"NewsArticle","headline":"Título"}</script>
    </head></html>"#;

    const NO_JSONLD: &str = "<html><head><title>Sitio</title></head><body>texto</body></html>";

    #[test]
    fn root_url_without_markers_is_index() {
        assert!(is_index_page(NO_JSONLD, &url("https://site.com/")));
        assert!(is_index_page(NO_JSONLD, &url("https://site.com")));
        assert!(is_index_page(NO_JSONLD, &url("https://site.com/index.html")));
    }

    #[test]
    fn subpath_without_markers_is_article() {
        assert!(!is_index_page(NO_JSONLD, &url("https://site.com/posts/hello")));
    }

    #[test]
    fn website_webpage_markers_are_index() {
        assert!(is_index_page(HOME_JSONLD, &url("https://site.com/")));
    }

    #[test]
    fn article_marker_wins_even_at_root() {
        assert!(!is_index_page(ARTICLE_JSONLD, &url("https://site.com/")));
    }

    #[test]
    fn subpath_collection_is_index() {
        let html = r#"<script type="application/ld+json">{"@type":"CollectionPage"}</script>"#;
        assert!(is_index_page(html, &url("https://site.com/blog")));
    }

    #[test]
    fn substack_home_with_og_article_is_still_index() {
        // `og:type=article` no debe confundir: el JSON-LD marca WebSite/WebPage.
        let html = r#"<meta property="og:type" content="article">
            <script type="application/ld+json">{"@type":"WebSite","mainEntityOfPage":{"@type":"WebPage"}}</script>"#;
        assert!(is_index_page(html, &url("https://site.com/")));
    }

    #[test]
    fn ignores_types_outside_jsonld_scripts() {
        // Un `"@type":"Article"` en otro contexto (p. ej. JS de la app) no cuenta.
        let html = r#"<script>const x = {"@type":"Article"};</script>
            <script type="application/ld+json">{"@type":"WebPage"}</script>"#;
        assert!(is_index_page(html, &url("https://site.com/")));
    }

    #[test]
    fn jsonld_blocks_parses_multiple() {
        let blocks = jsonld_blocks(r#"<script type="application/ld+json">{"a":1}</script>garbage<script type="application/ld+json">{"b":2}</script>"#);
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].contains("a"));
        assert!(blocks[1].contains("b"));
    }
}
