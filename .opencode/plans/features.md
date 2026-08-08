# Features - Lector

## Feature 1: Búsqueda Vectorial Semántica

### Descripción

Añadir capacidad de búsqueda semántica basada en embeddings de texto, permitiendo encontrar artículos por similitud conceptual (no solo palabras clave exactas).

### Estado

- [x] Planificación completada
- [x] Implementación
- [x] Tests
- [ ] Prueba manual con `npm run tauri dev` (requiere descargar el modelo ~80MB)
- [ ] Documentación pendiente

### Decisiones técnicas

| Aspecto | Decisión | Justificación |
|---------|----------|---------------|
| **Embeddings** | `fastembed` (ONNX Runtime) | 100% local, crate Rust maduro, modelo `all-MiniLM-L6-v2` (~80MB, 384 dims) |
| **Vector store** | `sqlite-vec` | Extensión SQLite, integración directa con `rusqlite`, sin servicios externos |
| **Texto a embedar** | Primeros ~512 tokens del `text` extraído | Balance entre precisión y coste computacional |
| **Modos de búsqueda** | BM25 / Vector / Hybrid (elegible por smart feed) | Flexibilidad: keywords exactas, semántica, o combinación |

### Arquitectura

```
crates/embeddings/          ← NUEVO crate
  ├── lib.rs                ← Puerto: trait EmbeddingGenerator
  ├── fastembed.rs          ← Adaptador: FastEmbedGenerator
  └── Cargo.toml            ← deps: fastembed, tokio (para async wrapper)

crates/storage/             ← MODIFICAR
  ├── lib.rs                ← migración user_version 6
  ├── embedding_repo.rs     ← NUEVO: EmbeddingRepository trait + impl
  └── smart_feed_repo.rs    ← MODIFICAR: soporte para búsqueda vectorial/hybrid

crates/pipeline/            ← MODIFICAR
  └── pipeline.rs           ← MODIFICAR: al extraer contenido, generar embedding

crates/app/                 ← MODIFICAR
  ├── commands.rs           ← NUEVOS comandos: generate_embedding, search_vectorial
  └── state.rs              ← NUEVO: embeddings: EmbeddingRepo
```

### Modelo de datos

```sql
-- Migración user_version 6
CREATE TABLE IF NOT EXISTS article_embeddings (
    article_id INTEGER PRIMARY KEY REFERENCES articles(id) ON DELETE CASCADE,
    embedding BLOB NOT NULL,  -- 384 floats = 1536 bytes (f32)
    model TEXT NOT NULL DEFAULT 'all-MiniLM-L6-v2',
    tokens_used INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS vec_articles USING vec0(
    article_id INTEGER PRIMARY KEY,
    embedding float[384]
);

-- Trigger: al borrar artículo, borrar embedding
CREATE TRIGGER IF NOT EXISTS articles_ad_embeddings AFTER DELETE ON articles BEGIN
    DELETE FROM article_embeddings WHERE article_id = old.id;
END;
```

### Smart feeds: campo `search_mode`

```sql
ALTER TABLE smart_feeds ADD COLUMN search_mode TEXT NOT NULL DEFAULT 'bm25';
-- Valores: 'bm25' | 'vector' | 'hybrid'
-- 'hybrid' = combinación ponderada de BM25 + vector (Reciprocal Rank Fusion)
```

### Flujo de generación de embeddings

1. Usuario extrae contenido (`extract_article`) → `pipeline.extract_article()`
2. Si el `text` extraído tiene > 50 caracteres:
   - Truncar a ~512 tokens (aproximadamente 2000 caracteres)
   - Llamar a `EmbeddingGenerator::embed(text)`
   - Guardar en `article_embeddings` + `vec_articles`
3. Si el texto es muy corto o no hay `text`, no generar embedding

### Flujo de búsqueda

#### Smart feed con `search_mode = 'bm25'` (actual)
```sql
SELECT ... FROM articles_fts WHERE articles_fts MATCH ? ORDER BY bm25(articles_fts)
```

#### Smart feed con `search_mode = 'vector'`
```sql
SELECT a.*, v.distance
FROM vec_articles v
JOIN articles a ON a.id = v.article_id
WHERE v.embedding MATCH ?
ORDER BY v.distance
LIMIT 50
```

#### Smart feed con `search_mode = 'hybrid'`
```
1. BM25 search → top 50 con scores
2. Vector search → top 50 con distances
3. Reciprocal Rank Fusion (RRF):
   score(d) = Σ 1/(k + rank_i(d))  donde k=60 por defecto
4. Ordenar por score fusionado descendente
```

### Puerto: EmbeddingGenerator

```rust
#[async_trait]
pub trait EmbeddingGenerator: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
    fn dimensions(&self) -> usize;
    fn model_name(&self) -> &str;
}
```

### Adaptador: FastEmbedGenerator

```rust
use fastembed::{TextEmbedding, TextInitOptions, EmbeddingModel};

pub struct FastEmbedGenerator {
    model: TextEmbedding,
}

impl FastEmbedGenerator {
    pub fn new() -> Result<Self, EmbeddingError> {
        let model = TextEmbedding::try_new(
            TextInitOptions::new(EmbeddingModel::AllMiniLML6V2)
        )?;
        Ok(Self { model })
    }
}

impl EmbeddingGenerator for FastEmbedGenerator {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let model = self.model.clone();
        let text = text.to_string();
        tokio::task::spawn_blocking(move || {
            let embeddings = model.embed(vec![text], None)?;
            Ok(embeddings.into_iter().next().unwrap())
        }).await?
    }
    
    fn dimensions(&self) -> usize { 384 }
    fn model_name(&self) -> &str { "all-MiniLM-L6-v2" }
}
```

### Comandos Tauri nuevos

| Comando | Descripción |
|---------|-------------|
| `generate_embedding(article_id)` | Genera embedding para un artículo específico |
| `generate_all_embeddings()` | Genera embeddings para todos los artículos sin embedding |
| `search_vectorial(query, limit)` | Búsqueda vectorial pura |
| `search_hybrid(query, limit)` | Búsqueda hybrid (BM25 + vector) |

### UI: selector de modo de búsqueda

En el diálogo de creación/edición de smart feed, añadir dropdown:
- **Búsqueda por palabras clave (BM25)** — búsqueda exacta en título y texto
- **Búsqueda semántica (Vector)** — búsqueda por similitud conceptual
- **Búsqueda híbrida** — combina ambas, mejor precisión

### Consideraciones de rendimiento

| Operación | Tiempo estimado | Notas |
|-----------|-----------------|-------|
| Generar embedding (1 artículo) | ~50-100ms | CPU, modelo quantizado |
| Generar embeddings (100 artículos) | ~5-10s | batch processing |
| Búsqueda vectorial (k=50) | ~10-20ms | sqlite-vec brute force |
| Búsqueda hybrid (k=50) | ~30-50ms | BM25 + vector + RRF |

### Dependencias nuevas

```toml
# crates/embeddings/Cargo.toml
[dependencies]
fastembed = "5.17"
tokio = { version = "1", features = ["rt-multi-thread"] }
thiserror = "2"
async-trait = "0.1"

# crates/storage/Cargo.toml (añadir)
sqlite-vec = "0.1"
zerocopy = { version = "0.8", features = ["derive"] }
```

### Tests requeridos

1. **Unit tests**: `FastEmbedGenerator` genera embeddings de dimensión correcta
2. **Integration tests**: `EmbeddingRepo` guarda/recupera embeddings
3. **Integration tests**: búsqueda vectorial devuelve artículos relevantes
4. **Integration tests**: búsqueda hybrid combina BM25 + vector correctamente
5. **End-to-end**: crear smart feed con modo "vector", verificar resultados

### Migración de datos existentes

Para artículos ya extraídos sin embedding:
- Nuevo comando `generate_all_embeddings()` que procesa en batch
- Progresión visible en UI (barra de progreso)
- Se puede ejecutar en background sin bloquear la app

### Limitaciones conocidas

1. **Modelo en inglés**: `all-MiniLM-L6-v2` funciona mejor en inglés. Para español funciona pero con menor precisión. Alternativa futura: `paraphrase-multilingual-MiniLM-L12-v2` (384 dims, multilingüe).
2. **Primer uso**: descarga del modelo (~80MB) requiere conexión a internet. Luego 100% offline.
3. **Espacio en disco**: ~1.5KB por artículo embedado (384 floats + metadata). 10K artículos = ~15MB.
4. **CPU**: generación de embeddings puede ser lenta en máquinas antiguas. Vector search es rápido.

### Future improvements

- [ ] Soporte para modelos multilingües (español)
- [ ] Reranking con cross-encoder (`TextRerank` de fastembed) para mejorar precisión
- [ ] Embeddings para imágenes (si se añade soporte de imágenes en el futuro)
- [ ] Búsqueda por similitud contra un artículo seleccionado ("artículos similares")
- [ ] Export/import de embeddings para backup
