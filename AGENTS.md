# AGENTS.md

## Qué es

"Lector": lector personal de feeds (RSS/Atom/JSON). App de escritorio **Tauri 2** con backend Rust (workspace Cargo) y frontend React+TS+Vite. Código, comentarios y textos de UI en **español** — mantener la convención.

## Cómo se ejecuta

- `npm run tauri dev` — la app completa (compila Rust y arranca Vite en `:5173`).
- `npm run dev` — solo el frontend. En un navegador normal los `invoke` de Tauri fallan; la UI depende del backend.
- `cargo test` — todos los tests Rust. Los de integración real requieren red y están `#[ignore]`: `cargo test --test real_ingest -- --ignored`.
- `npm test` — tests del frontend (vitest). Hoy solo hay `src/__tests__/useExternalLinks.test.tsx`.
- `npm run build` = `tsc && vite build`. Typecheck solo: `npx tsc`.
- `cargo build -p reader-app` requiere que exista `dist/` (tauri la empaqueta); ejecuta `npm run build` antes.

## Estructura (arquitectura hexagonal)

- `crates/domain` (`reader-domain`): modelos puros (`Article`, `Source`, `IngestResult`, ...), sin dependencias de infra. Fuente de verdad de los datos serializados.
- `crates/feeds` (`reader-feeds`): descubrimiento + parseo de feeds. Puertos `FeedDiscoverer`, `FeedParser`; adaptadores `WebpageDiscoverer`, `FeedRsParser`.
- `crates/extractor` (`reader-extractor`): extracción de contenido limpio. Puerto `ArticleExtractor`; adaptador `TrafilaturaExtractor`.
- `crates/storage` (`reader-storage`): SQLite + búsqueda FTS5. Puertos `ArticleRepository`, `SourceRepository`, `SmartFeedRepository`; adaptadores `ArticleRepo`, `SourceRepo`, `SmartFeedRepo`. Migraciones con `PRAGMA user_version`.
- `crates/pipeline` (`reader-pipeline`): orquestación asíncrona. `Pipeline` recibe los puertos inyectados (http, extractor, discoverer, parser, articles, sources); el HTTP real es `ReqwestClient`.
- `crates/embeddings` (`reader-embeddings`): embeddings semánticos. `FastEmbedGenerator` carga un modelo ONNX local vía fastembed (no requiere API ni red).
- `crates/app` (`reader-app`): binario Tauri. **OJO: la config Tauri está aquí** (`crates/app/tauri.conf.json`), NO en `src-tauri/`. Comandos en `commands.rs`, wiring en `state.rs`.
- `src/`: frontend. `src/api/commands.ts` envuelve `invoke`; `src/types.ts` es espejo de los modelos Rust.

## Reglas de calidad (obligatorio)

- **Todo cambio debe tener tests**: al añadir o modificar lógica, escribir tests (unitarios y/o de integración según corresponda). Un cambio sin tests no se entrega.
- **Antes de entregar, verificar siempre**:
  1. `cargo test` — todos los tests Rust deben pasar.
  2. `npm test` — los tests del frontend (vitest) deben pasar.
  3. `npm run build` (o al menos `npx tsc`) — el frontend debe compilar sin errores.
  4. `cargo build -p reader-app` — el crate de la app compila (requiere `dist/`, así que ejecuta `npm run build` primero).
- Si algo falla, arreglarlo antes de entregar. **Nunca** entregar código que rompa la compilación o los tests.
- Al añadir un comando Tauri nuevo, probarlo manualmente con `npm run tauri dev` para verificar que funciona end-to-end (no basta con que compile).

## Gotchas

- **Nuevo comando Tauri**: registrarlo en DOS sitios — `generate_handler!` en `crates/app/src/lib.rs` y wrapper en `src/api/commands.ts`. El nombre Rust es snake_case; desde TS las claves se pasan en camelCase (`sourceId` → `source_id`). Comandos actuales: `add_url`, `extract_article`, `list_sources`, `list_articles`, `list_single_articles`, `list_category_articles`, `get_article`, `mark_read`, `mark_all_read`, `toggle_star`, `delete_article`, `delete_source`, `rename_source`, `refresh_source`, `refresh_all_sources`, `get_refresh_interval`, `set_refresh_interval`, `get_vector_similarity_threshold`, `set_vector_similarity_threshold`, `get_theme`, `set_theme`, `get_reader_settings`, `set_reader_settings`, `export_opml`, `import_opml`, `list_categories`, `set_category`, `delete_category`, `list_smart_feeds`, `create_smart_feed`, `delete_smart_feed`, `get_smart_feed_articles`, `generate_embedding`, `regenerate_embedding`, `generate_all_embeddings`, `get_embedding_status`. Nota: `generate_all_embeddings` está registrado en Rust pero NO tiene wrapper TS todavía.
- **`src/types.ts` debe reflejar exactamente** los structs de `reader-domain` (serde serializa en snake_case). Al cambiar un modelo Rust, actualizar el espejo.
- **Nuevo método de repositorio**: añadir al trait Y al impl concreto, además de los mocks que implementan los traits en los tests de `crates/pipeline/src/pipeline.rs`.
- **Migraciones**: subir `user_version` y añadir el bloque en `crates/storage/src/lib.rs::migrate`. La tabla FTS5 `articles_fts` se mantiene sincronizada con triggers. Actualmente en `user_version` 8 (1 sources/articles/FTS, 2 settings + intervalo, 3 salud del feed, 4 category, 5 smart_feeds, 6 embeddings + `vec_articles` L2, 7 `vec_articles` recreada con `distance_metric=cosine` + setting `vector_similarity_threshold`, 8 ajustes de apariencia/lectura `theme`/`reader_*`/`show_snippets` en `settings`).
- **Settings y background refresh**: la configuración se guarda en la tabla `settings` (`SettingsRepository`/`SettingsRepo` en `crates/storage/src/settings.rs`). El refresco automático corre en `crates/app/src/lib.rs::spawn_refresh_task` y relee el intervalo en cada ciclo, emitiendo el evento `sources-refreshed` al frontend.
- **Tauri async**: desde callbacks síncronos (como `setup`), usar `tauri::async_runtime::spawn`, NO `tokio::spawn`. `tokio::spawn` exige un runtime tokio activo, pero `setup` corre en el hilo principal sin uno (esto causó un panic "no reactor running").
- **NO usar `window.confirm`/`alert`/`prompt` en macOS**: WKWebView no las soporta y devuelven `false`/no-op silenciosamente (el botón "eliminar source" no funcionaba por esto). Usar confirmaciones inline en React (dos clics) o el plugin de diálogo de Tauri.
- Los posts de feed se guardan con `html`/`raw_html` vacíos; el contenido completo se extrae bajo demanda (`extract_article`, botón "Extraer contenido completo"). `source_id = NULL` = "artículo suelto" (vista "Artículos sueltos" en la nav). Al borrar un source (`delete_source`) se borran TAMBIÉN sus artículos: `delete` hace `DELETE FROM articles WHERE source_id = ?` antes de borrar el source (el trigger `articles_ad` limpia el FTS5; la FK `ON DELETE SET NULL` queda como resguardo pero no llega a aplicarse).
- **Borrar categoría (`delete_category`)**: solo quita la columna `category` de los sources (`UPDATE sources SET category = NULL WHERE category = ?`); los sources se conservan y pasan a "Sin categoría". No es una tabla aparte, así que "borrar carpeta" = desasignar.
- **Modelo de navegación "todo es una vista"**: el sidebar tiene UNA sola vista activa a la vez (`view: View` en `src/types.ts`, unión tipada). Las vistas globales (`all`/`unread`/`starred`/`recent`/`single`) están en la nav; luego vienen "Búsquedas guardadas" y "Fuentes". NO mezclar estados de selección independientes (el bug clásico era smart feed + source a la vez). La búsqueda del toolbar (`query`) es un override temporal que no toca la vista activa.
- **Smart feeds = "Búsquedas guardadas"**: aparecen como sección propia en el sidebar con icono 🔍 (no como fuentes). Se almacenan en la tabla `smart_feeds` con una consulta FTS5. Los conteos (`list`/`get`) y la ejecución (`get_articles`) normalizan SIEMPRE con `to_fts_query`, para que el contador cuadre con los resultados.
- **Categorías = carpetas en la sección "Fuentes"**: los sources con `category` se agrupan en carpetas colapsables; clicar la carpeta abre la vista `{kind:"category"}` (artículos de todos sus sources, vía `list_category_articles`). Los sources sin categoría van bajo "Sin categoría". La categoría es una columna `TEXT` libre en `sources` (sin tabla propia).
- **"Marcar todo leído" es por vista**: `mark_all_read` recibe `ReadScope` (`all`/`source`/`category`/`smartFeed`) y marca solo el alcance activo. En "Artículos sueltos" y durante una búsqueda el botón se oculta.
- **Abrir un post lo marca leído automáticamente**: `openArticle` en `src/App.tsx` llama a `markRead(id, true)` al cargar cualquier artículo no leído (incluida la navegación por teclado). El botón del lector queda para desmarcar. No hay opción "marcar al hacer scroll": se eliminó cuando se introdujo el marcado al abrir.
- **Salud del feed**: cada source tiene `last_error`, `last_status`, `error_count`. Si `error_count > 0`, se muestra un indicador ⚠ en el sidebar.
- DB local de desarrollo: `~/Library/Application Support/com.local.lector/lector.db`.
