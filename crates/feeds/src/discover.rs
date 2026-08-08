use crate::{FeedDiscoverer, FeedError};
use url::Url;

/// Tipo de feed descubierto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedKind {
    Rss,
    Atom,
    Json,
}

impl FeedKind {
    fn from_content_type(content_type: &str) -> Option<FeedKind> {
        let ct = content_type.to_ascii_lowercase();
        if ct.contains("rss") {
            Some(FeedKind::Rss)
        } else if ct.contains("atom") {
            Some(FeedKind::Atom)
        } else if ct.contains("json") {
            Some(FeedKind::Json)
        } else {
            None
        }
    }
}

/// Enlace a un feed descubierto.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedLink {
    pub href: String,
    pub title: Option<String>,
    pub kind: FeedKind,
}

impl FeedLink {
    fn new(href: String, title: Option<String>, kind: FeedKind) -> Self {
        Self { href, title, kind }
    }
}

/// Descubre feeds anunciados explícitamente en el HTML
/// (`<link rel="alternate" type="...rss|atom|json" ...>`).
///
/// Solo devuelve feeds que la página anuncia: la heurística de rutas comunes
/// (`/feed`, `/rss`, ...) NO vive aquí, sino en el pipeline, que solo la aplica
/// cuando la página parece un índice del sitio.
pub struct WebpageDiscoverer;

impl FeedDiscoverer for WebpageDiscoverer {
    fn discover(&self, html: &str, base_url: &Url) -> Result<Vec<FeedLink>, FeedError> {
        let mut feeds = Vec::new();

        for tag in find_link_tags(html) {
            if let Some(link) = parse_alternate_feed_link(tag) {
                let resolved = base_url
                    .join(&link.href)
                    .map_err(|e| FeedError::InvalidUrl(e.to_string()))?;
                feeds.push(FeedLink::new(resolved.to_string(), link.title, link.kind));
            }
        }

        Ok(feeds)
    }
}

struct AlternateLink {
    href: String,
    title: Option<String>,
    kind: FeedKind,
}

/// Devuelve los subtags `<link ...>` del documento.
fn find_link_tags(html: &str) -> Vec<&str> {
    let mut tags = Vec::new();
    let mut rest = html;
    while let Some(rel) = rest.find("<link") {
        let after = &rest[rel + "<link".len()..];
        // Debe ser un tag `<link` (seguido de espacio o '>'), no `<!--` etc.
        let next = after.chars().next().filter(|c| c.is_whitespace() || *c == '>');
        if next.is_none() {
            break;
        }
        let end = after.find('>').unwrap_or(after.len());
        tags.push(&rest[rel..rel + "<link".len() + end + 1]);
        rest = &rest[rel + "<link".len() + end + 1..];
    }
    tags
}

/// Extrae el valor de un atributo de un tag (`name="valor"`).
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    let start = lower.find(&name)?;
    let after = &lower[start + name.len()..];
    // Permitir espacios entre el nombre y el '='.
    let eq = after.find('=')?;
    let after_eq = &tag[start + name.len() + eq + 1..];
    let after_eq_trimmed = after_eq.trim_start();
    let mut chars = after_eq_trimmed.chars();
    match chars.next() {
        Some(q) if q == '"' || q == '\'' => {
            let rest = &after_eq_trimmed[1..];
            let end = rest.find(q)?;
            Some(&rest[..end])
        }
        Some(_) => {
            let end = after_eq_trimmed
                .find(|c: char| c.is_whitespace() || c == '>')
                .unwrap_or(after_eq_trimmed.len());
            Some(&after_eq_trimmed[..end])
        }
        None => None,
    }
}

/// Si el tag `<link>` anuncia un feed, devuelve sus datos.
fn parse_alternate_feed_link(tag: &str) -> Option<AlternateLink> {
    let rel = attr(tag, "rel")?;
    if !rel.split_whitespace().any(|r| r.eq_ignore_ascii_case("alternate")) {
        return None;
    }
    let content_type = attr(tag, "type")?;
    let kind = FeedKind::from_content_type(content_type)?;
    let href = attr(tag, "href")?.to_string();
    let title = attr(tag, "title").map(ToString::to_string);
    Some(AlternateLink { href, title, kind })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discover(html: &str, base: &str) -> Vec<FeedLink> {
        let url = Url::parse(base).unwrap();
        WebpageDiscoverer.discover(html, &url).unwrap()
    }

    #[test]
    fn finds_rss_alternate_link() {
        let html = r#"
        <html><head>
          <link rel="alternate" type="application/rss+xml" title="Feed"
                href="/feed.xml">
        </head></html>"#;
        let feeds = discover(html, "https://ejemplo.com");
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].href, "https://ejemplo.com/feed.xml");
        assert_eq!(feeds[0].kind, FeedKind::Rss);
    }

    #[test]
    fn finds_atom_and_json() {
        let html = r#"
        <link rel="alternate" type="application/atom+xml" href="atom.xml">
        <link rel="alternate" type="application/json" href="/feed.json">"#;
        let feeds = discover(html, "https://ejemplo.com/blog");
        assert!(feeds.iter().any(|f| f.kind == FeedKind::Atom));
        assert!(feeds.iter().any(|f| f.kind == FeedKind::Json));
    }

    #[test]
    fn resolves_relative_urls() {
        let html = r#"<link rel="alternate" type="application/rss+xml" href="/rss">"#;
        let feeds = discover(html, "https://ejemplo.com/blog");
        assert_eq!(feeds[0].href, "https://ejemplo.com/rss");
    }

    #[test]
    fn ignores_non_alternate_and_non_feed_links() {
        let html = r#"
        <link rel="stylesheet" href="/style.css">
        <link rel="canonical" href="https://ejemplo.com/post">
        <link rel="alternate" href="https://ejemplo.com/fr" hreflang="fr">"#;
        // Ninguna de esas etiquetas es un feed: sin feeds explícitos, el
        // descubridor no inventa candidatos (la heurística es del pipeline).
        let feeds = discover(html, "https://ejemplo.com");
        assert!(feeds.is_empty());
    }

    #[test]
    fn returns_empty_when_no_explicit_feed() {
        let feeds = discover("<html><body>sin feed</body></html>", "https://ejemplo.com");
        assert!(feeds.is_empty());
    }

    #[test]
    fn captures_title_attribute() {
        let html = r#"<link rel="alternate" type="application/rss+xml" title="RSS Feed" href="/feed">"#;
        let feeds = discover(html, "https://ejemplo.com");
        assert_eq!(feeds[0].title.as_deref(), Some("RSS Feed"));
    }

    #[test]
    fn attr_parsing() {
        assert_eq!(attr(r#"<link href="/x">"#, "href"), Some("/x"));
        assert_eq!(attr(r#"<link href='/y'>"#, "href"), Some("/y"));
        assert_eq!(attr(r#"<link href = "/z" >"#, "href"), Some("/z"));
        assert_eq!(attr(r#"<link rel="nofollow">"#, "class"), None);
    }
}
