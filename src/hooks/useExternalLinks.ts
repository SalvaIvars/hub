import { useEffect } from "react";
import { openExternal } from "../api/commands";

/**
 * Intercepta en fase de captura los clics sobre los enlaces del contenido del
 * lector (`.reader-content`), que vienen con `target="_blank"` del sanitizador.
 *
 * Corre a nivel de documento para garantizar que siempre está adjunto. Bloquea
 * la navegación del webview (el contenido viene de feeds externos y es no
 * confiable) y abre el enlace en el navegador del sistema. Los hrefs relativos
 * se resuelven contra la URL del artículo cuando es posible.
 */
export function useExternalLinks(articleUrl: string | null): void {
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      const target = e.target as Element;
      if (!target.closest(".reader-content")) return;
      const anchor = target.closest("a");
      const href = anchor?.getAttribute("href")?.trim();
      if (!href) return;
      e.preventDefault();
      const lower = href.toLowerCase();
      if (lower.startsWith("#")) return;
      if (
        lower.startsWith("http://") ||
        lower.startsWith("https://") ||
        lower.startsWith("mailto:") ||
        lower.startsWith("tel:")
      ) {
        void openExternal(href).catch(() => {});
        return;
      }
      if (articleUrl) {
        try {
          const resolved = new URL(href, articleUrl);
          if (resolved.protocol === "http:" || resolved.protocol === "https:") {
            void openExternal(resolved.href).catch(() => {});
          }
        } catch {
          // Base inválida: el enlace no se abre.
        }
      }
    };
    document.addEventListener("click", handler, true);
    return () => document.removeEventListener("click", handler, true);
  }, [articleUrl]);
}
