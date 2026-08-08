import { useEffect } from "react";

/**
 * Muestra scrollbars ultrafinas (4px) al hacer scroll o hover sobre los
 * contenedores indicados, y las oculta tras `delay` ms de inactividad.
 * Los contenedores deben tener `overflow-y: auto|scroll`.
 */
export function useAutoHideScrollbars(
  refs: React.RefObject<HTMLElement | null>[],
  delay = 800,
) {
  useEffect(() => {
    const cleanups: (() => void)[] = [];

    for (const ref of refs) {
      const el = ref.current;
      if (!el) continue;

      let timeout: ReturnType<typeof setTimeout>;

      const onScroll = () => {
        el.classList.add("is-scrolling");
        clearTimeout(timeout);
        timeout = setTimeout(() => el.classList.remove("is-scrolling"), delay);
      };

      el.addEventListener("scroll", onScroll, { passive: true });
      cleanups.push(() => {
        el.removeEventListener("scroll", onScroll);
        clearTimeout(timeout);
      });
    }

    return () => cleanups.forEach((fn) => fn());
  }, [refs, delay]);
}
