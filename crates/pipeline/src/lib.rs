//! Orquestación asíncrona de la ingesta.
//!
//! `Pipeline` conecta el HTTP, el extractor, el descubridor de feeds y los
//! repositorios, sin conocer los detalles de cada adaptador (arquitectura
//! hexagonal). Un flujo típico:
//!
//! 1. `ingest_url(url)`: descarga la página, descubre el feed, crea/actualiza
//!    el source, guarda los posts nuevos del feed y extrae el artículo pegado.
//! 2. `refresh_source(id)`: re-descarga el feed y añade los posts nuevos.
//! 3. `extract_article(url)`: extrae y guarda un artículo concreto (útil para
//!    los posts del feed que se guardaron solo con su resumen).

mod http;
mod index;
mod pipeline;

pub use http::{FetchError, FetchedPage, HttpClient, ReqwestClient};
pub use pipeline::{Pipeline, PipelineError};

/// Devuelve el timestamp UTC actual en formato RFC3339.
pub fn utc_now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Extrae el host de una URL (para títulos por defecto).
pub fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_else(|| "Sitio desconocido".to_string())
}
