// Theme preference: follow the OS ("system") or pin "light"/"dark". The actual palette
// lives in index.css as CSS variables keyed on `data-theme`. To keep the dark palette in a
// single CSS block (no hand-synced media-query copy), we always resolve the preference to a
// concrete "light"/"dark" and stamp it on <html> — "system" is resolved eagerly from
// prefers-color-scheme and re-resolved when the OS theme changes.

export type Theme = "system" | "light" | "dark";

const KEY = "getmasters-theme";

const prefersDark = (): boolean =>
  typeof matchMedia !== "undefined" && matchMedia("(prefers-color-scheme: dark)").matches;

/** The preference the user picked (system/light/dark) — the source of truth for the toggle. */
export function getTheme(): Theme {
  const v = typeof localStorage !== "undefined" ? localStorage.getItem(KEY) : null;
  return v === "light" || v === "dark" ? v : "system";
}

/** Resolve a preference to the concrete palette to render. */
function resolve(theme: Theme): "light" | "dark" {
  return theme === "system" ? (prefersDark() ? "dark" : "light") : theme;
}

export function applyTheme(theme: Theme): void {
  document.documentElement.setAttribute("data-theme", resolve(theme));
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    /* private mode / no storage: in-memory only */
  }
}

/** Apply the stored preference as early as possible to avoid a flash of the wrong theme. */
export function initTheme(): void {
  applyTheme(getTheme());
  // Track OS changes while the preference is still "system".
  if (typeof matchMedia !== "undefined") {
    matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
      if (getTheme() === "system") applyTheme("system");
    });
  }
}

/** Cycle system → light → dark → system. */
export function nextTheme(theme: Theme): Theme {
  return theme === "system" ? "light" : theme === "light" ? "dark" : "system";
}
