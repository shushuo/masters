// Theme preference: follow the OS ("system") or pin "light"/"dark". The actual palette
// lives in index.css as CSS variables; here we only set `data-theme` on <html> and persist
// the choice. "system" removes the attribute so the prefers-color-scheme media query wins.

export type Theme = "system" | "light" | "dark";

const KEY = "getmasters-theme";

export function getTheme(): Theme {
  const v = typeof localStorage !== "undefined" ? localStorage.getItem(KEY) : null;
  return v === "light" || v === "dark" ? v : "system";
}

export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  if (theme === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", theme);
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    /* private mode / no storage: in-memory only */
  }
}

/** Apply the stored preference as early as possible to avoid a flash of the wrong theme. */
export function initTheme(): void {
  applyTheme(getTheme());
}

/** Cycle system → light → dark → system. */
export function nextTheme(theme: Theme): Theme {
  return theme === "system" ? "light" : theme === "light" ? "dark" : "system";
}
