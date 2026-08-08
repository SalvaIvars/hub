use crate::{EmbeddingError, EmbeddingGenerator};
use async_trait::async_trait;
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use std::sync::{Arc, Mutex};

/// Adaptador concreto sobre `fastembed` (ONNX Runtime local).
///
/// El modelo se descarga a `~/.cache/fastembed` (o `FASTEMBED_CACHE_DIR`) la
/// primera vez y luego se carga desde ahí, funcionando 100% offline.
#[derive(Clone)]
pub struct FastEmbedGenerator {
    model: Arc<Mutex<TextEmbedding>>,
}

impl FastEmbedGenerator {
    /// Carga el modelo `all-MiniLM-L6-v2` (384 dims, ~80MB). Puede tardar la
    /// primera vez (descarga) y en máquinas lentas. Llámese desde un hilo
    /// blocking.
    pub fn new() -> Result<Self, EmbeddingError> {
        let model = TextEmbedding::try_new(
            TextInitOptions::new(EmbeddingModel::AllMiniLML6V2)
                .with_show_download_progress(false),
        )
        .map_err(|e| EmbeddingError::Model(e.to_string()))?;
        Ok(Self {
            model: Arc::new(Mutex::new(model)),
        })
    }
}

#[async_trait]
impl EmbeddingGenerator for FastEmbedGenerator {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let texts = vec![text.to_string()];
        let mut batch = self.embed_batch(&texts).await?;
        batch.pop().ok_or(EmbeddingError::Model("sin salida".into()))
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.iter().all(|t| t.trim().is_empty()) {
            return Err(EmbeddingError::EmptyText);
        }
        let model = self.model.clone();
        let texts: Vec<String> = texts.to_vec();
        tokio::task::spawn_blocking(move || {
            let mut model = model.lock().unwrap();
            model
                .embed(texts, None)
                .map_err(|e| EmbeddingError::Model(e.to_string()))
        })
        .await
        .map_err(|_| EmbeddingError::TaskCancelled)?
    }

    fn dimensions(&self) -> usize {
        384
    }

    fn model_name(&self) -> &str {
        "all-MiniLM-L6-v2"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requiere descargar el modelo (~80MB) la primera vez"]
    fn dimensions_and_model_name() {
        let gen = FastEmbedGenerator::new().unwrap();
        assert_eq!(gen.dimensions(), 384);
        assert_eq!(gen.model_name(), "all-MiniLM-L6-v2");
    }
}
