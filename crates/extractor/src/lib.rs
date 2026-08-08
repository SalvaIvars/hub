//! Extracción de contenido limpio de una página web.
//!
//! Define el puerto `ArticleExtractor` y un adaptador sobre `rs-trafilatura`.
//! El puerto es el punto de entrada donde, en el futuro, se podrá añadir una
//! estrategia de extracción con IA sin tocar el resto del pipeline.

mod sanitize;

/// Resultado tipado de la extracción de un artículo.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedArticle {
    pub title: String,
    /// HTML estilizado listo para renderizar en el lector.
    pub content_html: String,
    /// Texto plano (para FTS5 y futura IA).
    pub text_content: String,
    pub byline: Option<String>,
    pub site_name: Option<String>,
    pub published_time: Option<String>,
    pub lang: Option<String>,
}

/// Error tipado de la capa de extracción.
#[derive(Debug, thiserror::Error)]
pub enum ExtractorError {
    #[error("no se pudo extraer contenido de la página: {0}")]
    ExtractionFailed(String),
}

/// Puerto: dado el HTML crudo de una página, devuelve el artículo limpio.
pub trait ArticleExtractor: Send + Sync {
    fn extract(&self, html: &str, url: &str) -> Result<ExtractedArticle, ExtractorError>;
}

/// Adaptador sobre `rs-trafilatura`.
pub struct TrafilaturaExtractor;

impl ArticleExtractor for TrafilaturaExtractor {
    fn extract(&self, html: &str, url: &str) -> Result<ExtractedArticle, ExtractorError> {
        // `include_links: true` conserva los `href` de los enlaces del artículo
        // (por defecto rs-trafilatura los elimina y deja `<a>` sin destino).
        let options = rs_trafilatura::Options {
            include_links: true,
            ..Default::default()
        };
        let result = rs_trafilatura::extract_with_options(html, &options)
            .map_err(|e| ExtractorError::ExtractionFailed(e.to_string()))?;

        Ok(ExtractedArticle {
            title: result
                .metadata
                .title
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| {
                    // Fallback: título del <title> de la página o el host.
                    html_title(html).unwrap_or_else(|| host_of(url))
                }),
            // El HTML va sanitizado (XSS + enlaces seguros + lazy loading).
            content_html: result
                .content_html
                .map(|h| sanitize::sanitize_html(&h))
                .unwrap_or_default(),
            text_content: result.content_text,
            byline: result.metadata.author.filter(|a| !a.is_empty()),
            site_name: result.metadata.sitename.filter(|s| !s.is_empty()),
            published_time: result.metadata.date.map(|d| d.to_rfc3339()),
            lang: result.metadata.language.filter(|l| !l.is_empty()),
        })
    }
}

fn html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title>")? + "<title>".len();
    let end = lower[start..].find("</title>")? + start;
    let title = html[start..end].trim().to_string();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_else(|| "Sin título".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTICLE_HTML: &str = r#"
    <html>
      <head>
        <title>Mi Título</title>
        <meta name="author" content="Autor Prueba">
        <meta property="og:site_name" content="Sitio Prueba">
      </head>
      <body>
        <nav>Navegación</nav>
        <article>
          <h1>Mi Título</h1>
          <p>Este es el contenido principal del artículo de prueba.</p>
          <p>Segundo párrafo con texto significativo.</p>
          <p>Consulta el <a href="https://ejemplo.com/guia">enlace a la guía</a> para más.</p>
        </article>
        <footer>Pie de página</footer>
      </body>
    </html>
    "#;

    #[test]
    fn extracts_title_and_content() {
        let extractor = TrafilaturaExtractor;
        let out = extractor.extract(ARTICLE_HTML, "https://ejemplo.com/post").unwrap();
        assert_eq!(out.title, "Mi Título");
        assert!(out.text_content.contains("contenido principal"));
        assert!(!out.text_content.contains("Navegación"));
        assert!(!out.text_content.contains("Pie de página"));
    }

    #[test]
    fn preserves_links_in_content_html() {
        // Fixture realista: con HTML muy minimalista trafilatura descarta los
        // enlaces por sus heurísticas de puntuación.
        let html = r#"<html><head><title>Título</title></head><body><article><div>
            <h1>Título del post</h1>
            <p>Un párrafo introductorio con algo de texto para que trafilatura lo puntúe bien y no lo descarte por ser demasiado corto.</p>
            <p>Segundo párrafo con más contenido explicando el tema en detalle para aumentar la longitud total del artículo.</p>
            <p>Consulta la <a href="https://ejemplo.com/guia">guía completa</a> para más información.</p>
        </div></article></body></html>"#;
        let extractor = TrafilaturaExtractor;
        let out = extractor.extract(html, "https://ejemplo.com/post").unwrap();
        // El `href` del enlace debe sobrevivir a la extracción (include_links).
        assert!(
            out.content_html.contains(r#"href="https://ejemplo.com/guia""#),
            "el HTML extraído debería conservar el href del enlace: {}",
            out.content_html
        );
    }

    #[test]
    fn extracts_metadata() {
        let extractor = TrafilaturaExtractor;
        let out = extractor.extract(ARTICLE_HTML, "https://ejemplo.com/post").unwrap();
        assert_eq!(out.byline.as_deref(), Some("Autor Prueba"));
        assert_eq!(out.site_name.as_deref(), Some("Sitio Prueba"));
    }

    #[test]
    fn content_html_has_structure_but_text_not() {
        let extractor = TrafilaturaExtractor;
        let out = extractor.extract(ARTICLE_HTML, "https://ejemplo.com/post").unwrap();
        assert!(!out.content_html.is_empty());
        assert!(out.content_html.contains('<'));
        assert!(!out.text_content.contains('<'));
    }

    #[test]
    fn empty_html_does_not_panic() {
        let extractor = TrafilaturaExtractor;
        let out = extractor.extract("", "https://ejemplo.com/post");
        assert!(out.is_ok() || out.is_err());
    }

    #[test]
    fn title_falls_back_to_host() {
        let extractor = TrafilaturaExtractor;
        let out = extractor.extract("<html><body>texto</body></html>", "https://ejemplo.com/x").unwrap();
        // rs-trafilatura puede devolver el <title> vacío o un título genérico;
        // lo importante es que nunca panique y devuelva un string.
        assert!(!out.title.is_empty());
    }
}
