(() => {
  "use strict";

  const root = document.documentElement;

  /* ---------- Theme toggle ---------- */

  const storageKey = "lector:theme";

  const themeToggle = document.querySelector("[data-theme-toggle]");
  if (themeToggle) {
    themeToggle.addEventListener("click", () => {
      const current = root.dataset.theme === "dark" ? "dark" : "light";
      const next = current === "dark" ? "light" : "dark";
      root.dataset.theme = next;
      root.style.colorScheme = next;
      localStorage.setItem(storageKey, next);
    });
  }

  /* ---------- Install tabs ---------- */

  const terminal = document.querySelector("[data-terminal]");
  if (terminal) {
    const tabs = Array.from(terminal.querySelectorAll(".term-tab"));
    const panes = Array.from(terminal.querySelectorAll(".term-pane"));

    tabs.forEach((tab) => {
      tab.addEventListener("click", () => {
        tabs.forEach((t) => {
          t.classList.toggle("is-active", t === tab);
          t.setAttribute("aria-selected", String(t === tab));
        });
        panes.forEach((p) => {
          p.classList.toggle("is-active", p.dataset.pane === tab.dataset.tab);
        });
      });
    });

    const platform = (() => {
      const ua = navigator.userAgent;
      if (/mac/i.test(ua)) return "macos";
      if (/win/i.test(ua)) return "windows";
      if (/linux/i.test(ua)) return "linux";
      return null;
    })();

    if (platform) {
      const match = tabs.find((t) => t.dataset.tab === platform);
      if (match) match.click();
    }
  }

  /* ---------- Reveal on scroll ---------- */

  const revealables = Array.from(document.querySelectorAll(".shot, .card, .not-item, .terminal"));
  revealables.forEach((el) => el.classList.add("reveal"));

  if ("IntersectionObserver" in window) {
    const io = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.classList.add("is-visible");
            io.unobserve(entry.target);
          }
        });
      },
      { threshold: 0.08, rootMargin: "0px 0px -40px 0px" }
    );
    revealables.forEach((el) => io.observe(el));
  } else {
    revealables.forEach((el) => el.classList.add("is-visible"));
  }
})();
