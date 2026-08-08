use crate::FeedError;
use reader_domain::FeedEntry;

/// Puerto: dado el cuerpo de un feed, devuelve sus entradas.
pub trait FeedParser: Send + Sync {
    fn parse(&self, body: &str) -> Result<Vec<FeedEntry>, FeedError>;
}

/// Adaptador sobre `feed-rs`: parsea RSS, Atom y JSON Feed con un modelo unificado.
pub struct FeedRsParser;

impl FeedParser for FeedRsParser {
    fn parse(&self, body: &str) -> Result<Vec<FeedEntry>, FeedError> {
        let feed = feed_rs::parser::parse(body.as_bytes())
            .map_err(|e| FeedError::ParseError(e.to_string()))?;

        Ok(feed
            .entries
            .into_iter()
            .map(|entry| {
                let title = entry
                    .title
                    .map(|t| t.content)
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| entry.id.clone());
                let link = entry
                    .links
                    .first()
                    .map(|l| l.href.clone())
                    .filter(|h| !h.is_empty())
                    .unwrap_or_else(|| entry.id.clone());
                let summary = entry.summary.map(|s| s.content);
                let published = entry.published.map(|p| p.to_rfc3339());
                FeedEntry {
                    title,
                    link,
                    summary,
                    published,
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"
    <?xml version="1.0" encoding="UTF-8"?>
    <rss version="2.0">
      <channel>
        <title>Feed de Prueba</title>
        <link>https://ejemplo.com</link>
        <item>
          <title>Artículo Uno</title>
          <link>https://ejemplo.com/1</link>
          <description>Resumen del artículo uno</description>
          <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
        </item>
        <item>
          <title>Artículo Dos</title>
          <link>https://ejemplo.com/2</link>
        </item>
      </channel>
    </rss>
    "#;

    const ATOM: &str = r#"
    <?xml version="1.0" encoding="UTF-8"?>
    <feed xmlns="http://www.w3.org/2005/Atom">
      <title>Atom Feed</title>
      <entry>
        <title>Entrada Atom</title>
        <link href="https://ejemplo.com/a/1"/>
        <id>urn:uuid:1234-5678</id>
        <updated>2024-01-02T03:04:05Z</updated>
        <summary>Tipo resumen de la entrada.</summary>
      </entry>
    </feed>
    "#;

    fn parse(body: &str) -> Vec<FeedEntry> {
        FeedRsParser.parse(body).unwrap()
    }

    #[test]
    fn parses_rss_entries() {
        let entries = parse(RSS);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Artículo Uno");
        assert_eq!(entries[0].link, "https://ejemplo.com/1");
        assert_eq!(entries[0].summary.as_deref(), Some("Resumen del artículo uno"));
        assert!(entries[0].published.is_some());
        assert_eq!(entries[1].published, None);
    }

    #[test]
    fn parses_atom_entries() {
        let entries = parse(ATOM);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Entrada Atom");
        assert_eq!(entries[0].link, "https://ejemplo.com/a/1");
    }

    #[test]
    fn entry_without_link_uses_id() {
        let feed = r#"
        <rss version="2.0"><channel><item>
          <title>Sin link</title>
          <guid>https://ejemplo.com/id-only</guid>
        </item></channel></rss>"#;
        let entries = parse(feed);
        assert_eq!(entries[0].link, "https://ejemplo.com/id-only");
    }

    #[test]
    fn invalid_body_is_error() {
        assert!(FeedRsParser.parse("esto no es un feed").is_err());
    }

    #[test]
    fn empty_body_is_error() {
        assert!(FeedRsParser.parse("").is_err());
    }
}
