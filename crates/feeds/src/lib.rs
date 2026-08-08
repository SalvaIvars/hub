//! Descubrimiento y parseo de feeds RSS/Atom/JSON.
//!
//! Define dos puertos:
//! - `FeedDiscoverer`: dado el HTML de una página, devuelve los enlaces de
//!   feed que anuncia (`<link rel="alternate" type="...rss|atom|json">`).
//! - `FeedParser`: dado el cuerpo de un feed, devuelve sus entradas.
//!
//! Nota de diseño: se evaluó `webpage-rs` para el descubrimiento, pero su
//! modelo `HTML.link` no conserva el atributo `rel`/`type` y su campo `feed`
//! nunca se puebla (siempre `None` en v2.x). Por eso el adaptador escanea el
//! HTML directamente, con soporte de heurísticas comunes (/feed, /rss, ...).

use url::Url;

pub mod discover;
pub mod parser;

pub use discover::{FeedKind, FeedLink, WebpageDiscoverer};
pub use parser::{FeedRsParser, FeedParser};

/// Error tipado de la capa de feeds.
#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    #[error("no se pudo parsear el feed: {0}")]
    ParseError(String),
    #[error("URL de feed inválida: {0}")]
    InvalidUrl(String),
}

/// Puerto: dado el HTML de una página, descubre los feeds que anuncia.
pub trait FeedDiscoverer: Send + Sync {
    fn discover(&self, html: &str, base_url: &Url) -> Result<Vec<FeedLink>, FeedError>;
}
