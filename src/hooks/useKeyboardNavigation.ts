import { useEffect, useRef } from "react";

export interface KeyboardActions {
  onNext?: () => void;
  onPrevious?: () => void;
  onMarkRead?: () => void;
  onToggleStar?: () => void;
  onClose?: () => void;
  onSearch?: () => void;
  onNewArticle?: () => void;
  onToggleSidebar?: () => void;
  onToggleFocus?: () => void;
}

function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target.tagName === "INPUT" ||
    target.tagName === "TEXTAREA" ||
    target.tagName === "SELECT"
  );
}

/**
 * Registra atajos de teclado globales para el lector.
 *
 * Se ignoran cuando el foco está en un campo de texto y cuando se pulsan
 * teclas modificadoras (Cmd/Ctrl/Alt), para no pisar atajos del sistema.
 *
 * Atajos: j/k navegar, m leído, s destacar, Esc cerrar, / buscar, n añadir URL,
 * b mostrar/ocultar sidebar, f modo lector.
 */
export function useKeyboardNavigation(actions: KeyboardActions): void {
  const actionsRef = useRef(actions);
  actionsRef.current = actions;

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (isTypingTarget(e.target)) return;

      switch (e.key) {
        case "j":
          actionsRef.current.onNext?.();
          break;
        case "k":
          actionsRef.current.onPrevious?.();
          break;
        case "m":
          actionsRef.current.onMarkRead?.();
          break;
        case "s":
          actionsRef.current.onToggleStar?.();
          break;
        case "Escape":
          actionsRef.current.onClose?.();
          break;
        case "/":
          e.preventDefault();
          actionsRef.current.onSearch?.();
          break;
        case "n":
          actionsRef.current.onNewArticle?.();
          break;
        case "b":
          actionsRef.current.onToggleSidebar?.();
          break;
        case "f":
          actionsRef.current.onToggleFocus?.();
          break;
      }
    };

    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);
}
