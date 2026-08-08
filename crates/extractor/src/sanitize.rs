//! Sanitización del HTML extraído antes de guardarlo.
//!
//! Protege al lector frente a XSS: el HTML de un feed o página web es
//! contenido no confiable que se renderiza con `dangerouslySetInnerHTML` en
//! el frontend. Aquí se recorta a un conjunto seguro de etiquetas, se
//! eliminan scripts/eventos y se fuerza a que los enlaces abran en pestaña
//! nueva con `rel="noopener noreferrer"`.

use ammonia::Builder;
use std::sync::OnceLock;

/// Etiquetas adicionales sobre el set por defecto de Ammonia.
///
/// Ammonia ya permite las típicas de contenido (`p`, `img`, `blockquote`,
/// `code`, `table`, ...) con `link_rel` = `noopener noreferrer` y
/// `strip_comments` = true. Añadimos las de embebidos multimedia: `iframe`
/// (YouTube/Vimeo), `picture`/`source` (imágenes responsivas) y `video`/
/// `audio` con sus fuentes.
const EXTRA_TAGS: &[&str] = &[
    "iframe",
    "picture",
    "source",
    "video",
    "audio",
    "track",
];

/// Atributos permitidos por etiqueta, además de los que Ammonia ya permite
/// por defecto (`src`, `alt`, `href`, `title`, `colspan`, ...).
const EXTRA_ATTRIBUTES: &[(&str, &[&str])] = &[
    ("iframe", &["src", "title", "width", "height", "allow", "allowfullscreen", "referrerpolicy", "loading", "frameborder"]),
    ("picture", &["media", "srcset", "sizes"]),
    ("source", &["srcset", "sizes", "type", "media"]),
    ("video", &["src", "poster", "controls", "width", "height", "preload", "playsinline"]),
    ("audio", &["src", "controls", "preload"]),
    ("track", &["src", "kind", "srclang", "label", "default"]),
    ("img", &["srcset", "sizes", "loading", "width", "height", "referrerpolicy"]),
];

/// Valores fijos que se fuerzan sobre ciertos atributos.
const FORCED_VALUES: &[(&str, &str, &str)] = &[
    // Los enlaces abren en pestaña nueva; `rel` lo pone Ammonia vía `link_rel`.
    ("a", "target", "_blank"),
    // Carga diferida: las imágenes fuera de pantalla no bloquean la lectura.
    ("img", "loading", "lazy"),
    ("iframe", "loading", "lazy"),
    // YouTube/embedded se permiten solo de forma segura.
    ("iframe", "referrerpolicy", "no-referrer-when-downgrade"),
];

/// Sanitiza un fragmento HTML a un conjunto seguro de etiquetas.
///
/// Devuelve un HTML que se puede renderizar con `dangerouslySetInnerHTML`
/// sin riesgo de ejecución de scripts.
pub fn sanitize_html(html: &str) -> String {
    builder().clean(html).to_string()
}

/// Builder de Ammonia compartido (se construye una sola vez).
fn builder() -> &'static Builder<'static> {
    static BUILDER: OnceLock<Builder<'static>> = OnceLock::new();
    BUILDER.get_or_init(|| {
        let mut b = Builder::new();
        b.add_tags(EXTRA_TAGS.iter().copied());
        for (tag, attrs) in EXTRA_ATTRIBUTES {
            b.add_tag_attributes(tag, attrs.iter().copied());
        }
        for (tag, attr, value) in FORCED_VALUES {
            b.set_tag_attribute_value(tag, attr, *value);
        }
        b
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script_tags() {
        let out = sanitize_html(r#"<p>Texto</p><script>alert('xss')</script>"#);
        assert!(!out.to_lowercase().contains("<script"));
        assert!(out.contains("<p>Texto</p>"));
    }

    #[test]
    fn strips_inline_event_handlers() {
        let out = sanitize_html(r#"<img src="x.png" onerror="alert(1)">"#);
        assert!(!out.contains("onerror"));
        assert!(out.contains("x.png"));
    }

    #[test]
    fn strips_javascript_urls() {
        let out = sanitize_html(r#"<a href="javascript:alert(1)">malo</a>"#);
        assert!(!out.contains("javascript:"));
        assert!(out.contains("malo"));
    }

    #[test]
    fn links_open_blank_with_noopener() {
        let out = sanitize_html(r#"<a href="https://ejemplo.com">Link</a>"#);
        assert!(out.contains(r#"target="_blank""#));
        assert!(out.contains(r#"rel="noopener noreferrer""#));
    }

    #[test]
    fn images_get_lazy_loading() {
        let out = sanitize_html(r#"<img src="foto.jpg" alt="Foto">"#);
        assert!(out.contains(r#"loading="lazy""#));
        assert!(out.contains("foto.jpg"));
        assert!(out.contains(r#"alt="Foto""#));
    }

    #[test]
    fn iframes_allowed_but_lazy() {
        let out = sanitize_html(
            r#"<iframe src="https://www.youtube.com/embed/xyz" allowfullscreen></iframe>"#,
        );
        assert!(out.contains("youtube.com"));
        assert!(out.contains(r#"loading="lazy""#));
        assert!(out.contains("allowfullscreen"));
    }

    #[test]
    fn iframe_strips_script_src() {
        let out = sanitize_html(r#"<iframe src="javascript:alert(1)"></iframe>"#);
        assert!(!out.contains("javascript:"));
    }

    #[test]
    fn preserves_safe_structure() {
        let html = r#"<p>Párrafo con <strong>negrita</strong> y <em>cursiva</em></p><ul><li>uno</li></ul>"#;
        let out = sanitize_html(html);
        assert!(out.contains("<strong>negrita</strong>"));
        assert!(out.contains("<em>cursiva</em>"));
        assert!(out.contains("<ul>"));
        assert!(out.contains("<li>uno</li>"));
    }

    #[test]
    fn strips_style_and_class_scripts() {
        let out = sanitize_html(r#"<div style="background:url(javascript:alert(1))">x</div>"#);
        // Ammonia no permite style por defecto: se elimina el atributo.
        assert!(!out.contains("style="));
    }

    #[test]
    fn empty_html_is_safe() {
        assert_eq!(sanitize_html(""), "");
        assert_eq!(sanitize_html("<script></script>"), "");
    }
}
