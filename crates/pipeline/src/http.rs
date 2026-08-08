use std::time::Duration;

/// Página descargada por un `HttpClient`.
#[derive(Debug, Clone)]
pub struct FetchedPage {
    /// URL efectiva tras redirecciones.
    pub final_url: String,
    pub html: String,
}

/// Error tipado de la capa HTTP.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("error de red: {0}")]
    Http(#[from] reqwest::Error),
    #[error("el servidor respondió con estado {0}")]
    HttpStatus(u16),
    #[error("URL inválida: {0}")]
    InvalidUrl(String),
}

/// Puerto: descarga una URL y devuelve su HTML y la URL efectiva.
#[async_trait::async_trait]
pub trait HttpClient: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<FetchedPage, FetchError>;
}

const USER_AGENT: &str =
    "hub/0.1 (lector de artículos personal; +https://localhost)";

/// Adaptador sobre `reqwest`.
pub struct ReqwestClient(pub reqwest::Client);

impl ReqwestClient {
    pub fn new() -> Self {
        Self(reqwest::Client::new())
    }
}

impl Default for ReqwestClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl HttpClient for ReqwestClient {
    async fn fetch(&self, url: &str) -> Result<FetchedPage, FetchError> {
        let url = url::Url::parse(url).map_err(|e| FetchError::InvalidUrl(e.to_string()))?;
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err(FetchError::InvalidUrl(format!(
                "esquema no soportado: {}",
                url.scheme()
            )));
        }

        let response = self
            .0
            .get(url.clone())
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .timeout(Duration::from_secs(20))
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            return Err(FetchError::HttpStatus(status.as_u16()));
        }

        let final_url = response.url().to_string();
        let html = response.text().await?;
        Ok(FetchedPage { final_url, html })
    }
}
