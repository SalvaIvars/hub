import { useCallback, useRef, useState } from "react";
import * as api from "../api/commands";
import type { Article } from "../types";

/** Mensaje mostrado en la barra de notificación. */
export interface Notice {
  kind: "ok" | "err";
  text: string;
}

/** Dependencias que el hook necesita para aplicar el resultado de la extracción. */
export interface UseAutoExtractOptions {
  /** Devuelve el id del artículo activo en el lector (para descartar resultados obsoletos). */
  getCurrentId: () => number | null;
  /** Muestra un artículo actualizado en el lector. */
  onResult: (article: Article) => void;
  /** Recarga la lista de artículos tras un cambio persistido. */
  onRefreshList: () => Promise<void>;
  /** Muestra un aviso en la barra de notificación (o lo limpia con null). */
  onNotice: (notice: Notice | null) => void;
}

/**
 * Extrae el contenido completo de un artículo bajo demanda (al abrirlo) y lo
 * persiste. Solo se aplica el resultado si el artículo sigue siendo el activo
 * en el lector, y no lanza una segunda extracción mientras otra está en curso
 * (protección contra la navegación rápida por teclado). Tras extraer texto, se
 * regenera el embedding semántico para que la búsqueda vectorial no use el
 * resumen viejo.
 */
export function useAutoExtractArticle(options: UseAutoExtractOptions) {
  const [extracting, setExtracting] = useState(false);
  const inFlightUrlRef = useRef<string | null>(null);

  const extract = useCallback(
    async (article: Article) => {
      if (article.html.trim().length) return;
      if (inFlightUrlRef.current) return;
      inFlightUrlRef.current = article.url;
      setExtracting(true);
      options.onNotice(null);
      try {
        const updated = await api.extractArticle(article.url);
        if (options.getCurrentId() !== article.id) return;
        options.onResult(updated);
        await options.onRefreshList();
        if (updated.text && updated.text.trim().length > 50) {
          options.onNotice({ kind: "ok", text: "Contenido extraído. Generando embedding" });
          try {
            await api.generateEmbedding(updated.id);
            options.onNotice({ kind: "ok", text: "Contenido extraído y embedding generado" });
            const refreshed = await api.getArticle(updated.id);
            if (options.getCurrentId() !== article.id) return;
            options.onResult(refreshed);
            await options.onRefreshList();
          } catch {
            options.onNotice({ kind: "ok", text: "Contenido extraído (embedding pendiente)" });
          }
        }
      } catch (e) {
        if (options.getCurrentId() === article.id) {
          options.onNotice({ kind: "err", text: `No se pudo extraer el contenido: ${e}` });
        }
      } finally {
        inFlightUrlRef.current = null;
        setExtracting(false);
      }
    },
    [options],
  );

  return { extracting, extract };
}
