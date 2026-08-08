import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Group, Panel, Separator } from "react-resizable-panels";
import type { Layout } from "react-resizable-panels";
import * as api from "./api/commands";
import { Reader } from "./components/Reader";
import { Settings } from "./components/Settings";
import { useKeyboardNavigation } from "./hooks/useKeyboardNavigation";
import { useAutoHideScrollbars } from "./hooks/useAutoHideScrollbars";
import { useAutoExtractArticle } from "./hooks/useAutoExtractArticle";
import { useExternalLinks } from "./hooks/useExternalLinks";
import type {
  Article,
  ArticleSummary,
  LayoutMode,
  ReadScope,
  ReaderSettings,
  SearchMode,
  SmartFeed,
  SourceSummary,
  Theme,
  View,
} from "./types";

/** Familias tipográficas del lector por valor del setting. */
const FONT_FAMILIES: Record<ReaderSettings["font_family"], string> = {
  serif: '"Lora", "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif',
  sans: '"Sora", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif',
  mono: '"SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
};

/** Acentos disponibles en el diseño (paleta de acento del tema nórdico). */
type Accent = "teal" | "plum" | "olive" | "copper";

/** Densidad de los elementos del sidebar/lista. */
type Density = "comodo" | "compacto";

const ACCENTS: Accent[] = ["teal", "plum", "olive", "copper"];

/** Interlineados del lector por valor del setting. */
const LINE_HEIGHTS: Record<ReaderSettings["line_height"], string> = {
  compact: "1.4",
  normal: "1.7",
  relaxed: "2.0",
};

/** Anchos de columna del lector por valor del setting. */
const WIDTHS: Record<ReaderSettings["width"], string> = {
  narrow: "600px",
  medium: "760px",
  wide: "900px",
};

/** Anchos de columna del lector en modo lector (un paso por encima del setting). */
const FOCUS_WIDTHS: Record<ReaderSettings["width"], string> = {
  narrow: "760px",
  medium: "980px",
  wide: "1120px",
};

const DEFAULT_READER_SETTINGS: ReaderSettings = {
  font_size: 19,
  font_family: "serif",
  line_height: "normal",
  width: "medium",
  show_snippets: true,
};

/** Resuelve el tema efectivo: "system" sigue la preferencia del SO. */
function resolveTheme(theme: Theme): "light" | "dark" | "sepia" {
  if (theme === "system") {
    return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  return theme;
}

export default function App() {
  const [theme, setTheme] = useState<Theme>("system");
  const [accent, setAccent] = useState<Accent>(() => {
    const saved = localStorage.getItem("lector-accent");
    return ACCENTS.includes(saved as Accent) ? (saved as Accent) : "teal";
  });
  const [density, setDensity] = useState<Density>(() => {
    const saved = localStorage.getItem("lector-density");
    return saved === "compacto" ? "compacto" : "comodo";
  });
  const [readerSettings, setReaderSettings] = useState<ReaderSettings>(DEFAULT_READER_SETTINGS);
  const [sources, setSources] = useState<SourceSummary[]>([]);
  const [categories, setCategories] = useState<string[]>([]);
  const [smartFeeds, setSmartFeeds] = useState<SmartFeed[]>([]);
  const [articles, setArticles] = useState<ArticleSummary[]>([]);
  const [view, setView] = useState<View>({ kind: "all" });
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [layoutMode, setLayoutMode] = useState<LayoutMode>(() => {
    const saved = localStorage.getItem("lector-layout-mode");
    return saved === "two-column" || saved === "focus" ? saved : "three-column";
  });
  const prevLayoutModeRef = useRef<LayoutMode>("three-column");
  const [savedLayouts, setSavedLayouts] = useState<Record<string, Layout>>(() => {
    try {
      return JSON.parse(localStorage.getItem("lector-panel-layouts") || "{}") as Record<string, Layout>;
    } catch {
      return {};
    }
  });
  const [current, setCurrent] = useState<Article | null>(null);
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [refreshingId, setRefreshingId] = useState<number | null>(null);
  const [refreshingAll, setRefreshingAll] = useState(false);
  const [renamingId, setRenamingId] = useState<number | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [confirmDeleteId, setConfirmDeleteId] = useState<number | null>(null);
  const [confirmDeleteSmartFeedId, setConfirmDeleteSmartFeedId] = useState<number | null>(null);
  const [confirmDeleteCategory, setConfirmDeleteCategory] = useState<string | null>(null);
  const [smartFeedFormOpen, setSmartFeedFormOpen] = useState(false);
  const [smartFeedName, setSmartFeedName] = useState("");
  const [smartFeedQuery, setSmartFeedQuery] = useState("");
  const [smartFeedMode, setSmartFeedMode] = useState<SearchMode>("bm25");
  const [embeddingSingleBusy, setEmbeddingSingleBusy] = useState(false);
  const [categoryFormId, setCategoryFormId] = useState<number | null>(null);
  const [categoryValue, setCategoryValue] = useState("");
  const [notice, setNotice] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  const [addUrlValue, setAddUrlValue] = useState("");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [intervalMinutes, setIntervalMinutes] = useState(30);
  const [similarityThreshold, setSimilarityThreshold] = useState(0.7);
  const [purgeDays, setPurgeDays] = useState(0);
  const [purging, setPurging] = useState(false);
  const [embeddingStatus, setEmbeddingStatus] = useState<[number, number] | null>(null);
  const [savingSettings, setSavingSettings] = useState(false);
  const currentIdRef = useRef<number | null>(null);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const addUrlInputRef = useRef<HTMLInputElement | null>(null);
  const articleListRef = useRef<HTMLElement | null>(null);
  const readerRef = useRef<HTMLElement | null>(null);
  const [focusTitle, setFocusTitle] = useState(false);

  useAutoHideScrollbars([articleListRef, readerRef]);
  useExternalLinks(current?.url ?? null);

  useEffect(() => {
    document.documentElement.dataset.theme = resolveTheme(theme);
  }, [theme]);

  useEffect(() => {
    localStorage.setItem("lector-accent", accent);
  }, [accent]);

  useEffect(() => {
    localStorage.setItem("lector-density", density);
  }, [density]);

  useEffect(() => {
    localStorage.setItem("lector-layout-mode", layoutMode);
  }, [layoutMode]);

  useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty("--reader-font-size", `${readerSettings.font_size}px`);
    root.style.setProperty(
      "--reader-font-family",
      FONT_FAMILIES[readerSettings.font_family] ?? FONT_FAMILIES.serif,
    );
    root.style.setProperty(
      "--reader-line-height",
      LINE_HEIGHTS[readerSettings.line_height] ?? LINE_HEIGHTS.normal,
    );
    root.style.setProperty(
      "--reader-width",
      WIDTHS[readerSettings.width] ?? WIDTHS.medium,
    );
    root.style.setProperty(
      "--reader-width-focus",
      FOCUS_WIDTHS[readerSettings.width] ?? FOCUS_WIDTHS.medium,
    );
  }, [readerSettings]);

  const reloadSources = useCallback(async () => {
    try {
      const [sourcesData, categoriesData, smartFeedsData] = await Promise.all([
        api.listSources(),
        api.listCategories(),
        api.listSmartFeeds(),
      ]);
      setSources(sourcesData);
      setCategories(categoriesData);
      setSmartFeeds(smartFeedsData);
    } catch (e) {
      setNotice({ kind: "err", text: `No se pudieron cargar los sources: ${e}` });
    }
  }, []);

  const reloadArticles = useCallback(async () => {
    try {
      if (query) {
        setArticles(await api.listArticles(null, query));
        return;
      }
      switch (view.kind) {
        case "all":
          setArticles(await api.listArticles());
          break;
        case "unread":
          setArticles(await api.listArticles(null, null, "unread"));
          break;
        case "starred":
          setArticles(await api.listArticles(null, null, "starred"));
          break;
        case "recent":
          setArticles(await api.listArticles(null, null, "recent"));
          break;
        case "single":
          setArticles(await api.listSingleArticles());
          break;
        case "source":
          setArticles(await api.listArticles(view.id));
          break;
        case "category":
          setArticles(await api.listCategoryArticles(view.name));
          break;
        case "smartFeed":
          setArticles(await api.getSmartFeedArticles(view.id));
          break;
      }
    } catch (e) {
      setNotice({ kind: "err", text: `No se pudieron cargar los artículos: ${e}` });
    }
  }, [query, view]);

  const { extracting, extract } = useAutoExtractArticle({
    getCurrentId: () => currentIdRef.current,
    onResult: setCurrent,
    onRefreshList: reloadArticles,
    onNotice: setNotice,
  });

  useEffect(() => {
    void reloadSources();
  }, [reloadSources]);

  useEffect(() => {
    api.getTheme().then((t) => setTheme(t)).catch(() => {});
    api.getReaderSettings().then(setReaderSettings).catch(() => {});
  }, []);

  useEffect(() => {
    void reloadArticles();
  }, [reloadArticles]);

  // En modo lector no puede quedarse la pantalla en blanco: si se cierra o
  // borra el artículo, se restaura el layout de columnas.
  useEffect(() => {
    if (current === null && layoutMode === "focus") {
      setLayoutMode(prevLayoutModeRef.current);
    }
  }, [current, layoutMode]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let unlistenBackfillDone: (() => void) | undefined;
    let unlistenBackfillError: (() => void) | undefined;
    let unlistenPurged: (() => void) | undefined;
    listen<number>("sources-refreshed", (e) => {
      const added = e.payload;
      if (added > 0) {
        setNotice({ kind: "ok", text: `Refresco automático: ${added} artículos nuevos` });
      }
      void reloadSources();
      void reloadArticles();
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    listen<number>("embedding-backfill-done", (e) => {
      const n = e.payload;
      if (n > 0) {
        setNotice({ kind: "ok", text: `Embeddings generados: ${n} artículos` });
        void reloadArticles();
      }
    })
      .then((fn) => {
        unlistenBackfillDone = fn;
      })
      .catch(() => {});
    listen<string>("embedding-backfill-error", (e) => {
      setNotice({ kind: "err", text: `Error generando embeddings: ${e.payload}` });
    })
      .then((fn) => {
        unlistenBackfillError = fn;
      })
      .catch(() => {});
    listen<number>("content-purged", (e) => {
      const n = e.payload;
      if (n > 0) {
        setNotice({ kind: "ok", text: `Limpieza automática: contenido vaciado en ${n} artículos` });
        void reloadArticles();
      }
    })
      .then((fn) => {
        unlistenPurged = fn;
      })
      .catch(() => {});
    return () => {
      unlisten?.();
      unlistenBackfillDone?.();
      unlistenBackfillError?.();
      unlistenPurged?.();
    };
  }, [reloadSources, reloadArticles]);

  async function handleAddUrl(e: React.FormEvent) {
    e.preventDefault();
    const url = addUrlValue.trim();
    if (!url) return;
    setBusy(true);
    setNotice(null);
    try {
      const result = await api.addUrl(url);
      setNotice({
        kind: "ok",
        text: result.source
          ? `Source "${result.source.title}" listo: ${result.feed_articles_added} posts nuevos del feed`
          : result.article_id !== null
            ? `Artículo guardado: ${result.article_title}`
            : "No se añadió nada: la URL es un feed o un índice del sitio sin feed",
      });
      setAddUrlValue("");
      await reloadSources();
      await reloadArticles();
      // Best-effort: genera el embedding del artículo añadido (en segundo plano).
      if (result.article_id !== null) {
        api.generateEmbedding(result.article_id).catch(() => {});
      }
    } catch (err) {
      setNotice({ kind: "err", text: String(err) });
    } finally {
      setBusy(false);
    }
  }

  async function openArticle(id: number, fromKeyboard = false) {
    currentIdRef.current = id;
    setFocusTitle(fromKeyboard);
    try {
      const a = await api.getArticle(id);
      setCurrent(a);
      if (!a.read) {
        await api.markRead(id, true);
        setCurrent((prev) => (prev && prev.id === id ? { ...prev, read: true } : prev));
        setArticles((prev) => prev.map((x) => (x.id === id ? { ...x, read: true } : x)));
        await reloadSources();
      }
      // Auto-extrae el contenido completo si el post solo tiene resumen.
      void extract(a);
    } catch (e) {
      setNotice({ kind: "err", text: `No se pudo abrir el artículo: ${e}` });
    }
  }

  async function handleRefresh(id: number) {
    setRefreshingId(id);
    setNotice(null);
    try {
      const added = await api.refreshSource(id);
      setNotice({
        kind: "ok",
        text: added > 0 ? `Actualizado: ${added} artículos nuevos` : "Actualizado: sin artículos nuevos",
      });
      await reloadSources();
      await reloadArticles();
    } catch (e) {
      setNotice({ kind: "err", text: `No se pudo actualizar: ${e}` });
    } finally {
      setRefreshingId(null);
    }
  }

  async function handleRefreshAll() {
    setRefreshingAll(true);
    setNotice(null);
    try {
      const added = await api.refreshAllSources();
      setNotice({
        kind: "ok",
        text: added > 0 ? `Actualizado: ${added} artículos nuevos` : "Todo actualizado: sin novedades",
      });
      await reloadSources();
      await reloadArticles();
    } catch (e) {
      setNotice({ kind: "err", text: `No se pudo actualizar: ${e}` });
    } finally {
      setRefreshingAll(false);
    }
  }

  async function handleMarkAllRead() {
    let scope: ReadScope;
    switch (view.kind) {
      case "source":
        scope = { kind: "source", id: view.id };
        break;
      case "category":
        scope = { kind: "category", name: view.name };
        break;
      case "smartFeed":
        scope = { kind: "smartFeed", id: view.id };
        break;
      default:
        scope = { kind: "all" };
    }
    try {
      const n = await api.markAllRead(scope);
      setNotice({ kind: "ok", text: n > 0 ? `${n} marcados como leídos` : "Todo leído" });
      await reloadArticles();
      await reloadSources();
    } catch (e) {
      setNotice({ kind: "err", text: `Error: ${e}` });
    }
  }

  function startRename(s: SourceSummary) {
    setRenamingId(s.id);
    setRenameValue(s.title);
  }

  async function saveRename() {
    const id = renamingId;
    const title = renameValue.trim();
    setRenamingId(null);
    if (id === null || !title) return;
    try {
      await api.renameSource(id, title);
      await reloadSources();
    } catch (e) {
      setNotice({ kind: "err", text: `Error al renombrar: ${e}` });
    }
  }

  async function handleDeleteSource(id: number) {
    try {
      setConfirmDeleteId(null);
      await api.deleteSource(id);
      if (view.kind === "source" && view.id === id) {
        setView({ kind: "all" });
        setCurrent(null);
        currentIdRef.current = null;
      }
      await reloadSources();
      await reloadArticles();
    } catch (e) {
      setNotice({ kind: "err", text: `Error al borrar: ${e}` });
    }
  }

  async function openSettings() {
    try {
      const [interval, threshold, embStatus, th, rs, purge] = await Promise.all([
        api.getRefreshInterval(),
        api.getVectorSimilarityThreshold(),
        api.getEmbeddingStatus(),
        api.getTheme(),
        api.getReaderSettings(),
        api.getContentPurgeDays(),
      ]);
      setIntervalMinutes(interval);
      setSimilarityThreshold(threshold);
      setEmbeddingStatus(embStatus);
      setTheme(th);
      setReaderSettings(rs);
      setPurgeDays(purge);
      setSettingsOpen(true);
    } catch (e) {
      setNotice({ kind: "err", text: `No se pudo leer la configuración: ${e}` });
    }
  }

  /** Aplica en vivo tema y apariencia del lector (preview desde el panel). */
  function applyAppearance(theme: Theme, rs: ReaderSettings) {
    setTheme(theme);
    setReaderSettings(rs);
  }

  async function saveSettings(
    theme: Theme,
    rs: ReaderSettings,
    interval: number,
    threshold: number,
    purge: number,
  ) {
    setSavingSettings(true);
    try {
      await Promise.all([
        api.setTheme(theme),
        api.setReaderSettings(rs),
        api.setRefreshInterval(interval),
        api.setVectorSimilarityThreshold(threshold),
        api.setContentPurgeDays(purge),
      ]);
      setSettingsOpen(false);
      setNotice({ kind: "ok", text: `Configuración guardada` });
    } catch (e) {
      setNotice({ kind: "err", text: `Error al guardar: ${e}` });
    } finally {
      setSavingSettings(false);
    }
  }

  /** Vacía ya el contenido extraído de los artículos de feed leídos. */
  async function handlePurge() {
    setPurging(true);
    setNotice(null);
    try {
      const n = await api.purgeExtractedContent(0);
      setNotice({
        kind: "ok",
        text: n > 0 ? `Contenido vaciado en ${n} artículos` : "No había contenido extraído que vaciar",
      });
      await reloadArticles();
    } catch (e) {
      setNotice({ kind: "err", text: `No se pudo vaciar el contenido: ${e}` });
    } finally {
      setPurging(false);
    }
  }

  async function handleSetCategory(id: number, category: string | null) {
    try {
      await api.setCategory(id, category);
      await reloadSources();
    } catch (e) {
      setNotice({ kind: "err", text: `Error al asignar categoría: ${e}` });
    }
  }

  async function handleCreateSmartFeed() {
    const name = smartFeedName.trim();
    const query = smartFeedQuery.trim();
    if (!name || !query) return;
    try {
      await api.createSmartFeed(name, query, smartFeedMode);
      setSmartFeedFormOpen(false);
      setSmartFeedName("");
      setSmartFeedQuery("");
      setSmartFeedMode("bm25");
      await reloadSources();
      setNotice({ kind: "ok", text: `Smart feed "${name}" creado` });
    } catch (e) {
      setNotice({ kind: "err", text: `Error al crear smart feed: ${e}` });
    }
  }

  async function handleDeleteSmartFeed(id: number) {
    try {
      setConfirmDeleteSmartFeedId(null);
      await api.deleteSmartFeed(id);
      if (view.kind === "smartFeed" && view.id === id) {
        setView({ kind: "all" });
      }
      await reloadSources();
    } catch (e) {
      setNotice({ kind: "err", text: `Error al borrar smart feed: ${e}` });
    }
  }

  async function handleDeleteCategory(name: string) {
    try {
      setConfirmDeleteCategory(null);
      const n = await api.deleteCategory(name);
      setNotice({
        kind: "ok",
        text: n > 0 ? `Categoría "${name}" eliminada: ${n} sources sin categoría` : `Categoría "${name}" eliminada`,
      });
      if (view.kind === "category" && view.name === name) {
        setView({ kind: "all" });
        setCurrent(null);
        currentIdRef.current = null;
      }
      await reloadSources();
      await reloadArticles();
    } catch (e) {
      setNotice({ kind: "err", text: `Error al borrar la categoría: ${e}` });
    }
  }

  function startCategory(id: number, category: string | null) {
    setCategoryFormId(id);
    setCategoryValue(category ?? "");
  }

  function saveCategory() {
    const id = categoryFormId;
    const cat = categoryValue.trim();
    setCategoryFormId(null);
    if (id === null) return;
    void handleSetCategory(id, cat || null);
  }

  function handleExtractFull() {
    if (!current) return;
    void extract(current);
  }

  async function handleRegenerateEmbedding() {
    if (!current) return;
    setEmbeddingSingleBusy(true);
    setNotice(null);
    try {
      await api.regenerateEmbedding(current.id);
      setNotice({ kind: "ok", text: "Embedding regenerado" });
      await reloadArticles();
      const updated = await api.getArticle(current.id);
      setCurrent(updated);
    } catch (e) {
      setNotice({ kind: "err", text: `No se pudo generar el embedding: ${e}` });
    } finally {
      setEmbeddingSingleBusy(false);
    }
  }

  async function handleMarkRead(read: boolean) {
    if (!current) return;
    try {
      await api.markRead(current.id, read);
      setCurrent({ ...current, read });
      setArticles((prev) => prev.map((a) => (a.id === current.id ? { ...a, read } : a)));
      await reloadSources();
    } catch (e) {
      setNotice({ kind: "err", text: `Error: ${e}` });
    }
  }

  async function handleToggleStar() {
    if (!current) return;
    try {
      await api.toggleStar(current.id);
      const updated = await api.getArticle(current.id);
      setCurrent(updated);
      setArticles((prev) =>
        prev.map((a) => (a.id === current.id ? { ...a, starred: updated.starred } : a)),
      );
    } catch (e) {
      setNotice({ kind: "err", text: `Error: ${e}` });
    }
  }

  async function handleDelete() {
    if (!current) return;
    try {
      await api.deleteArticle(current.id);
      setCurrent(null);
      currentIdRef.current = null;
      await reloadArticles();
      await reloadSources();
    } catch (e) {
      setNotice({ kind: "err", text: `Error al borrar: ${e}` });
    }
  }

  function select(v: View) {
    setView(v);
    setQuery("");
    setCurrent(null);
    setConfirmDeleteId(null);
    setConfirmDeleteSmartFeedId(null);
    setConfirmDeleteCategory(null);
    currentIdRef.current = null;
  }

  function toggleCollapse(key: string) {
    setCollapsed((prev) => ({ ...prev, [key]: !prev[key] }));
  }

  /** Muestra/oculta el sidebar (three-column <-> two-column). */
  function toggleSidebar() {
    setLayoutMode((mode) => {
      if (mode === "focus") return "two-column";
      return mode === "three-column" ? "two-column" : "three-column";
    });
  }

  /** Entra/sale del modo lector (solo el panel de lectura visible). */
  function toggleFocus() {
    if (layoutMode === "focus") {
      setLayoutMode(prevLayoutModeRef.current);
    } else {
      prevLayoutModeRef.current = layoutMode;
      setLayoutMode("focus");
    }
  }

  // --- Navegación por teclado ---
  const navigateByOffset = useCallback(
    (offset: number) => {
      if (articles.length === 0) return;
      const curIndex = current ? articles.findIndex((a) => a.id === current.id) : -1;
      let next = curIndex + offset;
      if (curIndex === -1) next = offset > 0 ? 0 : articles.length - 1;
      next = Math.min(Math.max(next, 0), articles.length - 1);
      if (next !== curIndex) {
        void openArticle(articles[next].id, true);
      }
    },
    [articles, current],
  );

  useKeyboardNavigation({
    onNext: () => navigateByOffset(1),
    onPrevious: () => navigateByOffset(-1),
    onMarkRead: () => {
      if (current) void handleMarkRead(!current.read);
    },
    onToggleStar: () => {
      if (current) void handleToggleStar();
    },
    onClose: () => {
      if (layoutMode === "focus") {
        setLayoutMode(prevLayoutModeRef.current);
      } else if (settingsOpen) {
        setSettingsOpen(false);
      } else if (smartFeedFormOpen) {
        setSmartFeedFormOpen(false);
      } else if (confirmDeleteId !== null) {
        setConfirmDeleteId(null);
      } else if (confirmDeleteSmartFeedId !== null) {
        setConfirmDeleteSmartFeedId(null);
      } else if (confirmDeleteCategory !== null) {
        setConfirmDeleteCategory(null);
      } else if (renamingId !== null) {
        setRenamingId(null);
      } else if (categoryFormId !== null) {
        setCategoryFormId(null);
      } else if (current) {
        setCurrent(null);
        currentIdRef.current = null;
      }
    },
    onSearch: () => searchInputRef.current?.focus(),
    onNewArticle: () => addUrlInputRef.current?.focus(),
    onToggleSidebar: toggleSidebar,
    onToggleFocus: toggleFocus,
  });

  const activeKind: View["kind"] | null = query ? null : view.kind;

  const showSidebar = layoutMode === "three-column";
  const showList = layoutMode === "three-column" || layoutMode === "two-column";
  const showReader = true;

  /**
   * Cada modo de layout guarda/restaura su propio layout en localStorage. Se
   * mantienen las claves antiguas del sidebar para conservar los anchos ya
   * guardados.
   */
  const layoutKey =
    layoutMode === "focus" ? "focus" : `sidebar:${layoutMode === "three-column"}`;
  const currentLayout = savedLayouts[layoutKey];

  function handleLayoutChanged(layout: Layout) {
    setSavedLayouts((prev) => {
      const next = { ...prev, [layoutKey]: layout };
      localStorage.setItem("lector-panel-layouts", JSON.stringify(next));
      return next;
    });
  }

  const renderSourceRow = (s: SourceSummary) => (
    <li key={s.id}>
      {renamingId === s.id ? (
        <input
          className="rename-input"
          value={renameValue}
          onChange={(e) => setRenameValue(e.target.value)}
          onBlur={() => void saveRename()}
          onKeyDown={(e) => {
            if (e.key === "Enter") void saveRename();
            if (e.key === "Escape") setRenamingId(null);
          }}
          autoFocus
        />
      ) : (
        <button
          className={`source-item${
            activeKind === "source" && view.kind === "source" && view.id === s.id ? " is-selected" : ""
          }${s.error_count > 0 ? " has-error" : ""}`}
          onClick={() => select({ kind: "source", id: s.id })}
          onDoubleClick={() => startRename(s)}
          title={s.last_error ? `Error: ${s.last_error}` : "Doble clic para renombrar"}
        >
          <span className="source-title">
            {s.error_count > 0 && <span className="error-indicator" title={s.last_error || "Error"} />}
            {s.title}
          </span>
          <span className="source-count">
            {s.unread_count > 0 ? `${s.unread_count} sin leer` : `${s.article_count}`}
          </span>
        </button>
      )}
      <div className="source-actions">
        <button
          className="refresh-btn"
          onClick={() => void handleRefresh(s.id)}
          disabled={refreshingId === s.id || !s.feed_url}
          title="Actualizar source"
        >
          {refreshingId === s.id ? "Actualizando" : "Refrescar"}
        </button>
        {categoryFormId === s.id ? (
          <input
            className="rename-input category-input"
            value={categoryValue}
            onChange={(e) => setCategoryValue(e.target.value)}
            onBlur={saveCategory}
            onKeyDown={(e) => {
              if (e.key === "Enter") saveCategory();
              if (e.key === "Escape") setCategoryFormId(null);
            }}
            placeholder="Categoría (vacío para quitar)"
            autoFocus
          />
        ) : (
          <button
            className="category-btn"
            onClick={() => startCategory(s.id, s.category)}
            title="Asignar categoría"
          >
            Categoría
          </button>
        )}
        {confirmDeleteId === s.id ? (
          <span className="delete-confirm">
            <button
              className="delete-confirm-yes"
              onClick={() => void handleDeleteSource(s.id)}
              title="Confirmar borrado"
            >
              Sí, borrar
            </button>
            <button
              className="delete-btn"
              onClick={() => setConfirmDeleteId(null)}
              title="Cancelar"
            >
              Cancelar
            </button>
          </span>
        ) : (
          <button
            className="delete-btn"
            onClick={() => setConfirmDeleteId(s.id)}
            title="Borrar source"
          >
            Borrar
          </button>
        )}
      </div>
    </li>
  );

  const groups: { key: string; label: string; clickable: boolean; sources: SourceSummary[] }[] = [
    ...categories.map((cat) => ({
      key: `cat:${cat}`,
      label: cat,
      clickable: true,
      sources: sources.filter((s) => s.category === cat),
    })),
    ...(() => {
      const uncategorized = sources.filter((s) => !s.category);
      return uncategorized.length > 0
        ? [{ key: "cat:__none__", label: "Sin categoría", clickable: false, sources: uncategorized }]
        : [];
    })(),
  ];

  return (
    <div className="app" data-accent={accent} data-density={density} data-focus={layoutMode === "focus"}>
      <a className="skip-link" href="#reader-pane">
        Saltar al contenido
      </a>
      <Group
        key={layoutKey}
        orientation="horizontal"
        defaultLayout={currentLayout}
        onLayoutChanged={handleLayoutChanged}
        className="panel-group"
      >
        {showSidebar && (
          <>
            <Panel id="sidebar" defaultSize="22" minSize="14" maxSize="40" className="layout-pane">
              <aside className="sidebar" role="navigation" aria-label="Navegación principal">
        <form className="add-form" onSubmit={handleAddUrl}>
          <input
            ref={addUrlInputRef}
            value={addUrlValue}
            onChange={(e) => setAddUrlValue(e.target.value)}
            placeholder="Pega un URL"
            aria-label="Añadir URL"
          />
          <button type="submit" disabled={busy}>
            {busy ? "Añadiendo" : "Añadir"}
          </button>
        </form>

        <nav className="nav" aria-label="Vistas">
          <button
            className={activeKind === "all" ? "is-selected" : ""}
            onClick={() => select({ kind: "all" })}
            aria-current={activeKind === "all" ? "page" : undefined}
          >
            Todos
          </button>
          <button
            className={activeKind === "unread" ? "is-selected" : ""}
            onClick={() => select({ kind: "unread" })}
            aria-current={activeKind === "unread" ? "page" : undefined}
          >
            Sin leer
          </button>
          <button
            className={activeKind === "starred" ? "is-selected" : ""}
            onClick={() => select({ kind: "starred" })}
            aria-current={activeKind === "starred" ? "page" : undefined}
          >
            Destacados
          </button>
          <button
            className={activeKind === "recent" ? "is-selected" : ""}
            onClick={() => select({ kind: "recent" })}
            aria-current={activeKind === "recent" ? "page" : undefined}
          >
            7 días
          </button>
          <button
            className={activeKind === "single" ? "is-selected" : ""}
            onClick={() => select({ kind: "single" })}
            aria-current={activeKind === "single" ? "page" : undefined}
          >
            Artículos sueltos
          </button>
        </nav>

        <div className="section-label">
          <h2>Búsquedas guardadas</h2>
          <div className="section-actions">
            <button
              className="icon-btn"
              onClick={() => setSmartFeedFormOpen((v) => !v)}
              title="Crear búsqueda guardada"
            >
              +
            </button>
          </div>
        </div>
        {smartFeedFormOpen && (
          <div className="smart-feed-form">
            <input
              value={smartFeedName}
              onChange={(e) => setSmartFeedName(e.target.value)}
              placeholder="Nombre"
              autoFocus
            />
            <input
              value={smartFeedQuery}
              onChange={(e) => setSmartFeedQuery(e.target.value)}
              placeholder="Consulta (ej: rust async)"
              onKeyDown={(e) => {
                if (e.key === "Enter") void handleCreateSmartFeed();
                if (e.key === "Escape") setSmartFeedFormOpen(false);
              }}
            />
            <label className="modal-field">
              <span>Modo de búsqueda</span>
              <select
                value={smartFeedMode}
                onChange={(e) => setSmartFeedMode(e.target.value as SearchMode)}
              >
                <option value="bm25">Palabras clave (BM25)</option>
                <option value="vector">Semántico (vector)</option>
                <option value="hybrid">Híbrido (ambos)</option>
              </select>
            </label>
            {(smartFeedMode === "vector" || smartFeedMode === "hybrid") && (
              <div className="smart-feed-warning">
                El modelo de embeddings (all-MiniLM-L6-v2) solo funciona bien con texto en
                inglés. Las búsquedas en español no encontrarán artículos en inglés y
                viceversa.
              </div>
            )}
            <div className="smart-feed-form-actions">
              <button onClick={() => void handleCreateSmartFeed()}>Crear</button>
              <button onClick={() => setSmartFeedFormOpen(false)}>Cancelar</button>
            </div>
          </div>
        )}
        <ul className="smart-feed-list">
          {smartFeeds.map((sf) => (
            <li key={sf.id}>
              <button
                className={`smart-feed-item${
                  activeKind === "smartFeed" && view.kind === "smartFeed" && view.id === sf.id
                    ? " is-selected"
                    : ""
                }`}
                onClick={() => select({ kind: "smartFeed", id: sf.id })}
              >
                <span className="smart-feed-name">{sf.name}</span>
                <span className="smart-feed-count">
                  {sf.unread_count > 0 ? `${sf.unread_count} sin leer` : `${sf.article_count}`}
                </span>
              </button>
              {confirmDeleteSmartFeedId === sf.id ? (
                <span className="delete-confirm">
                  <button
                    className="delete-confirm-yes"
                    onClick={() => void handleDeleteSmartFeed(sf.id)}
                    title="Confirmar borrado"
                  >
                    Sí, borrar
                  </button>
                  <button
                    className="delete-btn"
                    onClick={() => setConfirmDeleteSmartFeedId(null)}
                    title="Cancelar"
                  >
                    Cancelar
                  </button>
                </span>
              ) : (
                <button
                  className="delete-btn"
                  onClick={() => setConfirmDeleteSmartFeedId(sf.id)}
                  title="Borrar búsqueda guardada"
                >
                  Borrar
                </button>
              )}
            </li>
          ))}
          {smartFeeds.length === 0 && !smartFeedFormOpen && (
            <li className="empty-hint">Sin búsquedas guardadas</li>
          )}
        </ul>

          <div className="section-label">
            <h2>Fuentes</h2>
            <div className="section-actions">
              <button
                className="icon-btn"
                onClick={() => void handleRefreshAll()}
                disabled={refreshingAll}
                title="Actualizar todo"
              >
                {refreshingAll ? "Actualizando" : "Actualizar"}
              </button>
            </div>
          </div>
        {groups.length === 0 ? (
          <p className="empty-hint">Sin sources todavía</p>
        ) : (
          <ul className="source-list">
            {groups.map((group) => {
              const folderUnread = group.sources.reduce((acc, s) => acc + s.unread_count, 0);
              const folderTotal = group.sources.reduce((acc, s) => acc + s.article_count, 0);
              const isCategorySelected =
                group.clickable && activeKind === "category" && view.kind === "category" && view.name === group.label;
              return (
                <li key={group.key} className="category-folder">
                  <div className={`category-header${isCategorySelected ? " is-selected" : ""}`}>
                    {group.clickable ? (
                      <button
                        className="category-open-btn"
                        onClick={() => select({ kind: "category", name: group.label })}
                        title="Ver artículos de la categoría"
                      >
                        <span className="category-name">{group.label}</span>
                        <span className="category-count">
                          {folderUnread > 0 ? `${folderUnread} sin leer` : `${folderTotal}`}
                        </span>
                      </button>
                    ) : (
                      <div className="category-label">
                        <span className="category-name">{group.label}</span>
                        <span className="category-count">
                          {folderUnread > 0 ? `${folderUnread} sin leer` : `${folderTotal}`}
                        </span>
                      </div>
                    )}
                    <div className="category-actions">
                      <button
                        className="collapse-btn"
                        onClick={() => toggleCollapse(group.key)}
                        title={collapsed[group.key] ? "Desplegar" : "Plegar"}
                      >
                        {collapsed[group.key] ? "Abrir" : "Cerrar"}
                      </button>
                      {group.clickable &&
                        (confirmDeleteCategory === group.label ? (
                          <span className="delete-confirm">
                            <button
                              className="delete-confirm-yes"
                              onClick={() => void handleDeleteCategory(group.label)}
                              title="Confirmar borrado"
                            >
                              Sí, borrar
                            </button>
                            <button
                              className="delete-btn"
                              onClick={() => setConfirmDeleteCategory(null)}
                              title="Cancelar"
                            >
                              Cancelar
                            </button>
                          </span>
                        ) : (
                          <button
                            className="delete-btn"
                            onClick={() => setConfirmDeleteCategory(group.label)}
                            title="Borrar categoría (los sources se quedan sin categoría)"
                          >
                            Borrar
                          </button>
                        ))}
                    </div>
                  </div>
                  {!collapsed[group.key] && (
                    <ul className="folder-sources">
                      {group.sources.map((s) => renderSourceRow(s))}
                    </ul>
                  )}
                </li>
              );
            })}
          </ul>
        )}

        <div className="sidebar-footer">
          <button
            className="theme-toggle"
            onClick={() => {
              const resolved = resolveTheme(theme);
              const next: Theme = resolved === "dark" ? "light" : "dark";
              setTheme(next);
              api.setTheme(next).catch(() => {});
            }}
            title="Cambiar tema"
          >
            {resolveTheme(theme) === "dark" ? "Tema claro" : "Tema oscuro"}
          </button>
          <button className="settings-btn" onClick={() => void openSettings()}>
            Configuración
          </button>
        </div>
            </aside>
            </Panel>
            <Separator className="panel-resize-handle" />
          </>
        )}
        {showList && (
          <>
            <Panel id="list" defaultSize="30" minSize="18" maxSize="55" className="layout-pane">
              <section className="article-list-pane" ref={articleListRef} aria-label="Lista de artículos">
        <div className="list-toolbar">
          <input
            ref={searchInputRef}
            className="search-input"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Buscar en artículos"
            aria-label="Buscar"
          />
          {!query && view.kind !== "single" && (
            <button className="mark-all-btn" onClick={() => void handleMarkAllRead()}>
              Marcar todo leído
            </button>
          )}
        </div>
        <ul className="article-list" role="listbox" aria-label="Artículos">
          {articles.map((a) => (
            <li key={a.id} role="option" aria-selected={current?.id === a.id}>
              <button
                className={`article-item${current?.id === a.id ? " is-selected" : ""}${
                  a.read ? " is-read" : ""
                }`}
                onClick={() => openArticle(a.id)}
                aria-label={`${a.title || "Sin título"}${a.read ? ", leído" : ""}${
                  a.starred ? ", destacado" : ""
                }`}
              >
                <div className="article-item-title">
                  {a.starred && <span className="star">Destacado</span>}
                  {a.has_embedding && (
                    <span className="embedding-indicator" title="Tiene embedding semántico">
                      Embedding
                    </span>
                  )}
                  {a.title || "Sin título"}
                </div>
                <div className="article-item-sub">
                  <span>{a.site_name ?? a.source_title ?? ""}</span>
                  {readerSettings.show_snippets && a.snippet && (
                    <span className="snippet">{a.snippet}</span>
                  )}
                </div>
              </button>
            </li>
          ))}
          {articles.length === 0 && (
            <li className="empty-hint">
              {query ? "Sin resultados" : "Añade un URL para empezar"}
            </li>
          )}
        </ul>
          </section>
            </Panel>
            {showReader && <Separator className="panel-resize-handle" />}
          </>
        )}
        {showReader && (
          <Panel id="reader" minSize="20" className="layout-pane">
            <main className="reader-pane" ref={readerRef} id="reader-pane" aria-label="Lector">
        {notice && (
          <div className={`notice ${notice.kind}`} onClick={() => setNotice(null)}>
            {notice.text}
          </div>
        )}
        {current ? (
          <Reader
            article={current}
            onMarkRead={handleMarkRead}
            onToggleStar={handleToggleStar}
            onDelete={handleDelete}
            onExtractFull={handleExtractFull}
            onOpenOriginal={() => void api.openExternal(current.url)}
            extracting={extracting}
            onRegenerateEmbedding={() => void handleRegenerateEmbedding()}
            embeddingBusy={embeddingSingleBusy}
            focusTitle={focusTitle}
            onToggleFocus={toggleFocus}
            focusMode={layoutMode === "focus"}
          />
        ) : (
          <div className="reader-placeholder">
            Selecciona un artículo para leerlo.
          </div>
        )}
            </main>
          </Panel>
        )}
      </Group>

      {settingsOpen && (
        <Settings
          theme={theme}
          accent={accent}
          density={density}
          readerSettings={readerSettings}
          intervalMinutes={intervalMinutes}
          similarityThreshold={similarityThreshold}
          purgeDays={purgeDays}
          embeddingStatus={embeddingStatus}
          saving={savingSettings}
          purging={purging}
          onApplyAppearance={applyAppearance}
          onAccentChange={setAccent}
          onDensityChange={setDensity}
          onSave={saveSettings}
          onPurge={() => void handlePurge()}
          onNotice={setNotice}
          onReloadSources={() => void reloadSources()}
          onClose={() => setSettingsOpen(false)}
        />
      )}
    </div>
  );
}
