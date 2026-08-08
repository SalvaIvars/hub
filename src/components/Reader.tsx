import { useEffect, useMemo, useRef } from "react";
import type { Article } from "../types";

interface ReaderProps {
  article: Article;
  onMarkRead: (read: boolean) => void;
  onToggleStar: () => void;
  onDelete: () => void;
  onExtractFull: () => void;
  onOpenOriginal: () => void;
  extracting: boolean;
  onRegenerateEmbedding: () => void;
  embeddingBusy: boolean;
  /** Mueve el foco al título cuando cambia el artículo (navegación por teclado). */
  focusTitle?: boolean;
  /** Entra/sale del modo lector (ocultar columnas). */
  onToggleFocus: () => void;
  /** Si el modo lector está activo (solo el panel de lectura visible). */
  focusMode?: boolean;
}

function formatDate(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleDateString("es", {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

/**
 * Sustituye una imagen rota por un placeholder accesible, evitando que el
 * lector muestre el icono de imagen rota nativo.
 */
function handleImageError(img: HTMLImageElement): void {
  if (img.dataset.fallbackSet) return;
  img.dataset.fallbackSet = "1";
  img.style.display = "none";
  const placeholder = document.createElement("div");
  placeholder.className = "image-placeholder";
  placeholder.textContent = "Imagen no disponible";
  img.parentNode?.insertBefore(placeholder, img.nextSibling);
}

export function Reader({
  article,
  onMarkRead,
  onToggleStar,
  onDelete,
  onExtractFull,
  onOpenOriginal,
  extracting,
  onRegenerateEmbedding,
  embeddingBusy,
  focusTitle = false,
  onToggleFocus,
  focusMode = false,
}: ReaderProps) {
  const hasBody = article.html.trim().length > 0;
  const snippet = useMemo(() => {
    const text = article.text.trim();
    return text.length > 0 ? text : null;
  }, [article.text]);
  const contentRef = useRef<HTMLElement | null>(null);
  const titleRef = useRef<HTMLHeadingElement | null>(null);

  useEffect(() => {
    if (focusTitle) titleRef.current?.focus();
  }, [article.id, focusTitle]);

  useEffect(() => {
    const el = contentRef.current;
    if (!el) return;
    el.querySelectorAll("img").forEach((img) => {
      img.addEventListener("error", () => handleImageError(img), { once: true });
      if (img.complete && img.naturalWidth === 0 && img.src) {
        handleImageError(img);
      }
    });
  }, [article.html, article.id]);

  return (
    <div className="reader">
      {focusMode && (
        <button
          className="focus-exit-btn"
          onClick={onToggleFocus}
          title="Volver a las columnas"
        >
          Salir del modo lector
        </button>
      )}
      <header className="reader-header">
        <div className="reader-kicker" />
        <h1 className="reader-title" tabIndex={-1} ref={titleRef}>
          {article.title || "Sin título"}
        </h1>
        <div className="reader-meta">
          {article.site_name && <span>{article.site_name}</span>}
          {article.byline && <span>{article.byline}</span>}
          {article.published_at && <span>{formatDate(article.published_at)}</span>}
        </div>
        <div className="reader-actions">
          <button
            className={`action-btn${article.read ? " is-active" : ""}`}
            onClick={() => onMarkRead(!article.read)}
            aria-label={article.read ? "Marcar como no leído" : "Marcar como leído"}
            aria-pressed={article.read}
            title={article.read ? "Marcar como no leído" : "Marcar como leído"}
          >
            {article.read ? "✓ Leído" : "Marcar leído"}
          </button>
          <button
            className={`action-btn${article.starred ? " is-active" : ""}`}
            onClick={onToggleStar}
            aria-label={article.starred ? "Quitar destacado" : "Destacar artículo"}
            aria-pressed={article.starred}
            title="Destacar"
          >
            {article.starred ? "Destacado" : "Destacar"}
          </button>
          {!hasBody && (
            <button className="action-btn" onClick={onExtractFull} disabled={extracting}>
              {extracting ? "Extrayendo" : "Extraer contenido completo"}
            </button>
          )}
          <button
            className={`action-btn${article.has_embedding ? " is-active" : ""}`}
            onClick={onRegenerateEmbedding}
            disabled={embeddingBusy}
            aria-label={
              article.has_embedding
                ? "Regenerar embedding semántico"
                : "Generar embedding semántico"
            }
            title={
              article.has_embedding
                ? "Regenerar embedding semántico"
                : "Generar embedding semántico"
            }
          >
            {embeddingBusy ? "Generando" : article.has_embedding ? "Regenerar embedding" : "Generar embedding"}
          </button>
          <button className="action-btn" onClick={onOpenOriginal} title="Abrir original">
            Abrir original
          </button>
          {!focusMode && (
            <button
              className="action-btn"
              onClick={onToggleFocus}
              title="Ocultar columnas y leer a pantalla completa"
            >
              Modo lector
            </button>
          )}
          <button className="action-btn danger" onClick={onDelete} title="Borrar">
            Borrar
          </button>
        </div>
      </header>

      {hasBody ? (
        <article
          className="reader-content"
          ref={contentRef}
          dangerouslySetInnerHTML={{ __html: article.html }}
        />
      ) : snippet ? (
        <div className="reader-content reader-fallback">
          <p>{snippet}</p>
          {!extracting && (
            <p className="reader-hint">
              Este post del feed solo tiene su resumen. Pulsa "Extraer contenido
              completo" para leerlo sin anuncios.
            </p>
          )}
        </div>
      ) : (
        <div className="reader-empty">Sin contenido disponible para este artículo.</div>
      )}
    </div>
  );
}
