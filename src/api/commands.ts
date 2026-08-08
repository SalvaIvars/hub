import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Article, ArticleSummary, IngestResult, ReadScope, ReaderSettings, SearchMode, SmartFeed, SourceSummary, Theme } from "../types";

/**
 * Abre una URL en el navegador del sistema. En Tauri usa el plugin opener
 * oficial; en un navegador normal (dev sin backend) cae a window.open.
 */
export async function openExternal(url: string): Promise<void> {
  try {
    await openUrl(url);
  } catch {
    window.open(url, "_blank", "noopener,noreferrer");
  }
}

/** Ingiere un URL: descubre feed + guarda posts + extrae el artículo. */
export function addUrl(url: string): Promise<IngestResult> {
  return invoke<IngestResult>("add_url", { url });
}

/** Extrae y guarda un artículo concreto (para posts del feed con solo resumen). */
export function extractArticle(url: string): Promise<Article> {
  return invoke<Article>("extract_article", { url });
}

export function listSources(): Promise<SourceSummary[]> {
  return invoke<SourceSummary[]>("list_sources");
}

export function listArticles(
  sourceId?: number | null,
  q?: string | null,
  filter?: string | null,
): Promise<ArticleSummary[]> {
  return invoke<ArticleSummary[]>("list_articles", {
    sourceId: sourceId ?? null,
    q: q ?? null,
    filter: filter ?? null,
  });
}

export function listSingleArticles(): Promise<ArticleSummary[]> {
  return invoke<ArticleSummary[]>("list_single_articles");
}

export function listCategoryArticles(category: string): Promise<ArticleSummary[]> {
  return invoke<ArticleSummary[]>("list_category_articles", { category });
}

export function getArticle(id: number): Promise<Article> {
  return invoke<Article>("get_article", { id });
}

export function markRead(id: number, read: boolean): Promise<void> {
  return invoke<void>("mark_read", { id, read });
}

/** Marca como leídos los artículos del alcance indicado (biblioteca, source, categoría o smart feed). */
export function markAllRead(scope: ReadScope): Promise<number> {
  return invoke<number>("mark_all_read", { scope });
}

export function toggleStar(id: number): Promise<void> {
  return invoke<void>("toggle_star", { id });
}

export function deleteArticle(id: number): Promise<void> {
  return invoke<void>("delete_article", { id });
}

export function renameSource(id: number, title: string): Promise<void> {
  return invoke<void>("rename_source", { id, title });
}

export function deleteSource(id: number): Promise<void> {
  return invoke<void>("delete_source", { id });
}

export function refreshSource(id: number): Promise<number> {
  return invoke<number>("refresh_source", { id });
}

export function refreshAllSources(): Promise<number> {
  return invoke<number>("refresh_all_sources");
}

export function getRefreshInterval(): Promise<number> {
  return invoke<number>("get_refresh_interval");
}

export function setRefreshInterval(minutes: number): Promise<void> {
  return invoke<void>("set_refresh_interval", { minutes });
}

/** Devuelve los días tras los que se vacía automáticamente el contenido extraído (0 = nunca). */
export function getContentPurgeDays(): Promise<number> {
  return invoke<number>("get_content_purge_days");
}

/** Guarda los días de purga automática del contenido extraído (0 = nunca). */
export function setContentPurgeDays(days: number): Promise<void> {
  return invoke<void>("set_content_purge_days", { days });
}

/** Vacía el contenido extraído de artículos leídos (days=0: todos; >0: anteriores a `days` días). */
export function purgeExtractedContent(days: number): Promise<number> {
  return invoke<number>("purge_extracted_content", { days });
}

/** Devuelve el umbral de similitud de la búsqueda semántica (0.0–1.0). */
export function getVectorSimilarityThreshold(): Promise<number> {
  return invoke<number>("get_vector_similarity_threshold");
}

/** Guarda el umbral de similitud de la búsqueda semántica (0.0–1.0). */
export function setVectorSimilarityThreshold(threshold: number): Promise<void> {
  return invoke<void>("set_vector_similarity_threshold", { threshold });
}

/** Devuelve el tema de la interfaz ("system" | "light" | "dark" | "sepia"). */
export function getTheme(): Promise<Theme> {
  return invoke<Theme>("get_theme");
}

/** Guarda el tema de la interfaz. */
export function setTheme(theme: Theme): Promise<void> {
  return invoke<void>("set_theme", { theme });
}

/** Devuelve los ajustes de lectura (tipografía, ancho, etc.). */
export function getReaderSettings(): Promise<ReaderSettings> {
  return invoke<ReaderSettings>("get_reader_settings");
}

/** Guarda los ajustes de lectura. */
export function setReaderSettings(settings: ReaderSettings): Promise<void> {
  return invoke<void>("set_reader_settings", { settings });
}

/** Exporta las fuentes a un archivo OPML en `path`. Devuelve el nº exportado. */
export function exportOpml(path: string): Promise<number> {
  return invoke<number>("export_opml", { path });
}

/** Importa fuentes desde un archivo OPML en `path`. Devuelve el nº importado. */
export function importOpml(path: string): Promise<number> {
  return invoke<number>("import_opml", { path });
}

export function listCategories(): Promise<string[]> {
  return invoke<string[]>("list_categories");
}

export function setCategory(id: number, category: string | null): Promise<void> {
  return invoke<void>("set_category", { id, category });
}

export function deleteCategory(name: string): Promise<number> {
  return invoke<number>("delete_category", { name });
}

export function listSmartFeeds(): Promise<SmartFeed[]> {
  return invoke<SmartFeed[]>("list_smart_feeds");
}

export function createSmartFeed(name: string, query: string, searchMode: SearchMode): Promise<number> {
  return invoke<number>("create_smart_feed", { name, query, searchMode });
}

export function deleteSmartFeed(id: number): Promise<void> {
  return invoke<void>("delete_smart_feed", { id });
}

export function getSmartFeedArticles(id: number): Promise<ArticleSummary[]> {
  return invoke<ArticleSummary[]>("get_smart_feed_articles", { id });
}

/** Genera el embedding del contenido de un artículo concreto. */
export function generateEmbedding(articleId: number): Promise<void> {
  return invoke<void>("generate_embedding", { articleId });
}

/** Borra y regenera el embedding de un artículo desde su contenido actual. */
export function regenerateEmbedding(articleId: number): Promise<void> {
  return invoke<void>("regenerate_embedding", { articleId });
}

/** Devuelve (artículos con embedding, total de artículos). */
export function getEmbeddingStatus(): Promise<[number, number]> {
  return invoke<[number, number]>("get_embedding_status");
}
