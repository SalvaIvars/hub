// Modelos espejo del crate `reader-domain` (serializados por los comandos Tauri).

export interface Article {
  id: number;
  source_id: number | null;
  url: string;
  title: string;
  html: string;
  text: string;
  raw_html: string;
  byline: string | null;
  site_name: string | null;
  published_at: string | null;
  fetched_at: string;
  read: boolean;
  starred: boolean;
  has_embedding: boolean;
}

export interface ArticleSummary {
  id: number;
  source_id: number | null;
  source_title: string | null;
  url: string;
  title: string;
  site_name: string | null;
  published_at: string | null;
  fetched_at: string;
  read: boolean;
  starred: boolean;
  snippet: string | null;
  has_embedding: boolean;
}

export interface SourceSummary {
  id: number;
  url: string;
  home_url: string;
  title: string;
  description: string | null;
  feed_url: string | null;
  last_fetched_at: string | null;
  article_count: number;
  unread_count: number;
  last_error: string | null;
  error_count: number;
  category: string | null;
}

export interface IngestResult {
  source: SourceSummary | null;
  /** Id del artículo creado; null si la URL era un feed o una página índice. */
  article_id: number | null;
  article_title: string;
  feed_articles_added: number;
}

/** Modo de búsqueda de un smart feed. Espejo de `SearchMode` en reader-domain. */
export type SearchMode = "bm25" | "vector" | "hybrid";

/** Tema de la interfaz ("system" sigue la preferencia del sistema operativo). */
export type Theme = "system" | "light" | "dark" | "sepia";

/** Modo de layout de la app (distribución de los paneles). */
export type LayoutMode = "three-column" | "two-column" | "focus" | "list-only";

/** Ajustes de lectura del panel de configuración. Espejo de `ReaderSettings`. */
export interface ReaderSettings {
  font_size: number;
  font_family: "serif" | "sans" | "mono";
  line_height: "compact" | "normal" | "relaxed";
  width: "narrow" | "medium" | "wide";
  show_snippets: boolean;
}

export interface SmartFeed {
  id: number;
  name: string;
  query: string;
  created_at: string;
  search_mode: SearchMode;
  article_count: number;
  unread_count: number;
}

/** Alcance de "marcar todo leído". Espejo de `ReadScope` en reader-domain. */
export type ReadScope =
  | { kind: "all" }
  | { kind: "source"; id: number }
  | { kind: "category"; name: string }
  | { kind: "smartFeed"; id: number };

/** Vista de artículos activa en el sidebar: solo una a la vez. */
export type View =
  | { kind: "all" }
  | { kind: "unread" }
  | { kind: "starred" }
  | { kind: "recent" }
  | { kind: "single" }
  | { kind: "source"; id: number }
  | { kind: "category"; name: string }
  | { kind: "smartFeed"; id: number };
