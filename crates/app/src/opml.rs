//! Exportación e importación de fuentes en formato OPML 2.0.
//!
//! La exportación agrupa las fuentes por categoría (carpetas anidadas como
//! `<outline>` contenedores). La importación lee las fuentes (`xmlUrl`) y las
//! inserta con `upsert`, por lo que repetir una importación actualiza las
//! fuentes existentes en lugar de duplicarlas.

use reader_domain::SourceSummary;

/// Una fuente leída de un OPML.
#[derive(Debug, Clone, PartialEq)]
pub struct OpmlFeed {
    pub title: String,
    pub xml_url: String,
    pub html_url: Option<String>,
    pub category: Option<String>,
}

/// Escapa un valor para usarlo dentro de un atributo XML (doble comilla).
fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Genera un documento OPML con las fuentes agrupadas por categoría.
pub fn export_opml_xml(sources: &[SourceSummary]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<opml version=\"2.0\">\n<head><title>hub - Fuentes</title></head>\n<body>\n");

    // Agrupa por categoría manteniendo el orden de `sources`; las sin categoría
    // van al final.
    let mut grouped: Vec<(Option<String>, Vec<&SourceSummary>)> = Vec::new();
    for s in sources {
        let cat = s.category.clone();
        if let Some(entry) = grouped.iter_mut().find(|(c, _)| *c == cat) {
            entry.1.push(s);
        } else {
            grouped.push((cat, vec![s]));
        }
    }

    for (category, items) in grouped {
        let cat_open = category
            .as_ref()
            .map(|c| format!("  <outline text=\"{}\">\n", escape_attr(c)))
            .unwrap_or_default();
        out.push_str(&cat_open);
        for s in items {
            let feed_url = s.feed_url.clone().unwrap_or_else(|| s.url.clone());
            let title = if s.title.trim().is_empty() {
                crate::host_of(&feed_url)
            } else {
                s.title.clone()
            };
            out.push_str(&format!(
                "    <outline type=\"rss\" text=\"{}\" xmlUrl=\"{}\" htmlUrl=\"{}\"/>\n",
                escape_attr(&title),
                escape_attr(&feed_url),
                escape_attr(&s.home_url),
            ));
        }
        if category.is_some() {
            out.push_str("  </outline>\n");
        }
    }

    out.push_str("</body>\n</opml>\n");
    out
}

/// Atributos de un elemento `<outline>` que nos interesan.
#[derive(Default)]
struct OutlineAttrs {
    text: Option<String>,
    xml_url: Option<String>,
    html_url: Option<String>,
}

/// Lee los atributos de un `<outline>`, des-escapando los valores.
fn collect_attrs(e: &quick_xml::events::BytesStart) -> Result<OutlineAttrs, String> {
    let mut out = OutlineAttrs::default();
    for attr in e.attributes() {
        let attr = attr.map_err(|er| format!("atributo inválido en OPML: {er}"))?;
        let key = attr.key.into_inner();
        let raw = String::from_utf8_lossy(attr.value.as_ref()).into_owned();
        let value = quick_xml::escape::unescape(&raw)
            .map(|c| c.into_owned())
            .unwrap_or(raw);
        if key.eq_ignore_ascii_case(b"text") || key.eq_ignore_ascii_case(b"title") {
            if out.text.is_none() {
                out.text = Some(value);
            }
        } else if key.eq_ignore_ascii_case(b"xmlurl") {
            out.xml_url = Some(value);
        } else if key.eq_ignore_ascii_case(b"htmlurl") {
            out.html_url = Some(value);
        }
    }
    Ok(out)
}

fn push_feed(feeds: &mut Vec<OpmlFeed>, cat_stack: &[String], attrs: &OutlineAttrs, xml_url: &str) {
    let title = attrs
        .text
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| crate::host_of(xml_url));
    let category = cat_stack
        .last()
        .filter(|c| !c.is_empty())
        .cloned();
    feeds.push(OpmlFeed {
        title,
        xml_url: xml_url.to_string(),
        html_url: attrs.html_url.clone(),
        category,
    });
}

/// Parsea un documento OPML y devuelve sus fuentes.
///
/// Un `<outline>` sin `xmlUrl` se interpreta como carpeta/categoría: sus hijos
/// heredan su `text` como categoría. Se soportan contenedores anidados.
pub fn parse_opml(xml: &str) -> Result<Vec<OpmlFeed>, String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut feeds: Vec<OpmlFeed> = Vec::new();
    let mut cat_stack: Vec<String> = vec![String::new()];

    loop {
        match reader.read_event() {
            Err(e) => return Err(format!("OPML inválido: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) if e.local_name().as_ref().eq_ignore_ascii_case(b"outline") => {
                let attrs = collect_attrs(&e)?;
                match attrs.xml_url {
                    Some(ref xu) if !xu.trim().is_empty() => {
                        push_feed(&mut feeds, &cat_stack, &attrs, xu);
                        // Marcador: el End de este outline no cambia la categoría.
                        cat_stack.push(String::new());
                    }
                    _ => cat_stack.push(attrs.text.unwrap_or_default()),
                }
            }
            Ok(Event::Empty(e)) if e.local_name().as_ref().eq_ignore_ascii_case(b"outline") => {
                let attrs = collect_attrs(&e)?;
                if let Some(ref xu) = attrs.xml_url {
                    if !xu.trim().is_empty() {
                        push_feed(&mut feeds, &cat_stack, &attrs, xu);
                    }
                }
            }
            Ok(Event::End(e)) if e.local_name().as_ref().eq_ignore_ascii_case(b"outline") => {
                cat_stack.pop();
            }
            _ => {}
        }
    }

    Ok(feeds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(id: i64, title: &str, category: Option<&str>, feed_url: Option<&str>) -> SourceSummary {
        SourceSummary {
            id,
            url: format!("https://site{id}.com/feed.xml"),
            home_url: format!("https://site{id}.com/"),
            title: title.to_string(),
            description: None,
            feed_url: feed_url.map(|f| f.to_string()),
            last_fetched_at: None,
            article_count: 0,
            unread_count: 0,
            last_error: None,
            error_count: 0,
            category: category.map(|c| c.to_string()),
        }
    }

    #[test]
    fn export_groups_by_category() {
        let sources = vec![
            summary(1, "Rust", Some("Tecnología"), None),
            summary(2, "Sin tema", None, None),
            summary(3, "Go", Some("Tecnología"), None),
        ];
        let xml = export_opml_xml(&sources);
        assert!(xml.contains("<opml version=\"2.0\">"));
        assert!(xml.contains("<outline text=\"Tecnología\">"));
        assert!(xml.contains("text=\"Rust\""));
        assert!(xml.contains("xmlUrl=\"https://site1.com/feed.xml\""));
        // "Sin tema" está fuera de la carpeta de categoría.
        let tec_start = xml.find("text=\"Tecnología\"").unwrap();
        let sin = xml.find("text=\"Sin tema\"").unwrap();
        assert!(sin > tec_start);
    }

    #[test]
    fn export_escapes_attributes() {
        let s = summary(1, "A & <B> \"C\"", None, None);
        let xml = export_opml_xml(&[s]);
        assert!(xml.contains("A &amp; &lt;B&gt; &quot;C&quot;"));
    }

    #[test]
    fn parse_handles_categories_and_entities() {
        let xml = r#"<?xml version="1.0"?>
        <opml version="2.0">
          <body>
            <outline text="Tecnología">
              <outline type="rss" text="Rust &amp; Cía" xmlUrl="https://a.com/feed.xml" htmlUrl="https://a.com/"/>
            </outline>
            <outline type="rss" text="Sueltos" xmlUrl="https://b.com/rss"/>
          </body>
        </opml>"#;
        let feeds = parse_opml(xml).unwrap();
        assert_eq!(feeds.len(), 2);
        assert_eq!(feeds[0].title, "Rust & Cía");
        assert_eq!(feeds[0].xml_url, "https://a.com/feed.xml");
        assert_eq!(feeds[0].category.as_deref(), Some("Tecnología"));
        assert_eq!(feeds[1].category, None);
        assert_eq!(feeds[1].xml_url, "https://b.com/rss");
    }

    #[test]
    fn parse_rejects_invalid_xml() {
        assert!(parse_opml("<opml><broken").is_err());
    }
}
