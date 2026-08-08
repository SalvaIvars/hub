//! Generación de embeddings locales con `fastembed` (ONNX Runtime).
//!
//! Define el puerto hexagonal `EmbeddingGenerator` y el adaptador
//! `FastEmbedGenerator`. El modelo se descarga a disco la primera vez y luego
//! todo corre offline (100% local, sin APIs externas).

mod fastembed;

pub use fastembed::FastEmbedGenerator;

use async_trait::async_trait;

/// Error tipado de la capa de embeddings.
#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error("error del modelo de embeddings: {0}")]
    Model(String),
    #[error("el texto a embedar está vacío")]
    EmptyText,
    #[error("la tarea de embeddings fue cancelada")]
    TaskCancelled,
}

/// Puerto: genera vectores de embeddings a partir de texto.
#[async_trait]
pub trait EmbeddingGenerator: Send + Sync {
    /// Genera el embedding de un texto (sync interno envuelto en blocking).
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;
    /// Genera los embeddings de varios textos en un solo paso (batch).
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
    /// Dimensión de los vectores generados.
    fn dimensions(&self) -> usize;
    /// Nombre del modelo usado (se persiste junto a los embeddings).
    fn model_name(&self) -> &str;
}

/// Trunca un texto a un máximo de tokens aproximado (heurística de caracteres).
///
/// No se tokeniza de verdad (requeriría exponer el tokenizer del modelo); se
/// usa la regla conservadora de ~4 caracteres por token y se corta limpio en
/// un límite de palabra. El modelo recorta internamente al máximo de su
/// contexto de todas formas.
pub fn truncate_to_tokens(text: &str, max_tokens: usize) -> String {
    const CHARS_PER_TOKEN: usize = 4;
    let max_chars = max_tokens * CHARS_PER_TOKEN;
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    // Corta en el último espacio para no partir palabras por la mitad.
    match truncated.rfind(' ') {
        Some(idx) if idx > 0 => truncated[..idx].to_string(),
        _ => truncated,
    }
}

/// Tamaño de token usado por defecto al truncar textos antes de embedarlos.
pub const DEFAULT_MAX_TOKENS: usize = 512;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_short_text() {
        assert_eq!(truncate_to_tokens("corto", 512), "corto");
        assert_eq!(truncate_to_tokens("", 512), "");
    }

    #[test]
    fn truncate_long_text_cuts_at_word_boundary() {
        let long = "palabra ".repeat(1000);
        let out = truncate_to_tokens(&long, 512);
        // 512 tokens * 4 chars = 2048 chars de presupuesto = 256 palabras de 8 chars.
        assert!(out.len() <= 512 * 4);
        assert!(!out.ends_with(' '));
        assert_eq!(out.split_whitespace().count(), 512 * 4 / 8);
    }

    #[test]
    fn truncate_cuts_even_without_spaces() {
        let long = "a".repeat(10_000);
        let out = truncate_to_tokens(&long, 512);
        assert_eq!(out.len(), 512 * 4);
    }

    /// Demo real de la búsqueda semántica: textos del MISMO tema quedan cerca
    /// y temas distintos quedan lejos, aunque no compartan palabras.
    ///
    /// Ejecutar con: `cargo test -p reader-embeddings -- --ignored --nocapture`
    /// (la primera vez descarga el modelo ~80MB; requiere internet).
    #[tokio::test]
    #[ignore = "requiere descargar el modelo (~80MB) la primera vez"]
    async fn semantic_similarity_demo() {
        use crate::fastembed::FastEmbedGenerator;
        let gen = FastEmbedGenerator::new().unwrap();

        // Pasamos por el trait para probar el camino real (embed + truncate).
        async fn emb(gen: &FastEmbedGenerator, t: &str) -> Vec<f32> {
            gen.embed(&truncate_to_tokens(t, DEFAULT_MAX_TOKENS)).await.unwrap()
        }

        let economia = emb(
            &gen,
            "El gobierno anunció una subida de impuestos sobre los combustibles fósiles para reducir las emisiones.",
        )
        .await;
        let economia2 = emb(
            &gen,
            "Medidas fiscales: se incrementará el gravamen de la gasolina y el diésel.",
        )
        .await;
        let cocina = emb(&gen, "Receta de tortilla de patatas con cebolla y huevos frescos.").await;
        let tech = emb(
            &gen,
            "El lenguaje Rust permite escribir sistemas concurrentes seguros sin recolección de basura.",
        )
        .await;

        // Distancia euclídea normalizada: menor = más parecido.
        let d = |a: &[f32], b: &[f32]| -> f64 {
            a.iter()
                .zip(b)
                .map(|(x, y)| (*x as f64 - *y as f64).powi(2))
                .sum::<f64>()
                .sqrt()
        };

        let d_same = d(&economia, &economia2);
        let d_food = d(&economia, &cocina);
        let d_tech = d(&economia, &tech);

        println!("economía vs economía (paráfrasis): {d_same:.4}");
        println!("economía vs cocina:                 {d_food:.4}");
        println!("economía vs tecnología:             {d_tech:.4}");

        // Mismo tema claramente más cerca que temas distintos.
        assert!(
            d_same < d_food,
            "paráfrasis debería estar más cerca que cocina (d_same={d_same:.4} vs d_food={d_food:.4})"
        );
        assert!(
            d_same < d_tech,
            "paráfrasis debería estar más cerca que tecnología (d_same={d_same:.4} vs d_tech={d_tech:.4})"
        );
        println!("\n✅ La búsqueda semántica funciona: textos del mismo tema quedan cerca.");
    }

    /// Demuestra la limitación del modelo `all-MiniLM-L6-v2` (solo inglés):
    /// un texto en inglés y su traducción al español NO quedan cerca, aunque
    /// signifiquen lo mismo. Documenta el caso de "texto en inglés + query en
    /// español" que falla con el modelo actual.
    #[tokio::test]
    #[ignore = "requiere descargar el modelo (~80MB) la primera vez"]
    async fn english_text_spanish_query_limitation() {
        use crate::fastembed::FastEmbedGenerator;
        let gen = FastEmbedGenerator::new().unwrap();

        async fn emb(gen: &FastEmbedGenerator, t: &str) -> Vec<f32> {
            gen.embed(&truncate_to_tokens(t, DEFAULT_MAX_TOKENS)).await.unwrap()
        }

        let en1 = emb(
            &gen,
            "The government announced a tax increase on fossil fuels to reduce emissions.",
        )
        .await;
        let en2 = emb(
            &gen,
            "New fiscal measures will raise the levy on gasoline and diesel.",
        )
        .await;
        let es = emb(
            &gen,
            "el gobierno subió los impuestos a la gasolina y el diésel para reducir las emisiones",
        )
        .await;
        let cooking_en = emb(&gen, "Spanish omelette recipe with potatoes and onions.").await;

        let d = |a: &[f32], b: &[f32]| -> f64 {
            a.iter()
                .zip(b)
                .map(|(x, y)| (*x as f64 - *y as f64).powi(2))
                .sum::<f64>()
                .sqrt()
        };

        let d_same_lang = d(&en1, &en2);
        let d_cross = d(&en1, &es);
        let d_unrelated = d(&en1, &cooking_en);

        println!("EN vs EN (paráfrasis, mismo idioma): {d_same_lang:.4}");
        println!("EN vs ES (traducción, idiomas cruzados): {d_cross:.4}");
        println!("EN vs EN (tema distinto): {d_unrelated:.4}");

        // En el modelo EN-only, la "traducción" al español queda TAN lejos como
        // un tema completamente distinto. La búsqueda EN→ES falla.
        println!("\nℹ️ Con este modelo, texto en inglés + query en español = la query española no encuentra el artículo inglés.");
    }
}
