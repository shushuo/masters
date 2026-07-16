import { useCallback, useEffect, useState } from "react";

/** The primary views, mirrored in the URL hash so back/forward and deep links work. */
export type View = "chat" | "watch" | "masters" | "projects" | "settings";

export interface Route {
  view: View;
  /** Active chat session (`#/chat/:sessionId`). */
  sessionId?: string;
  /** Selected project (`#/projects/:id`). */
  projectId?: string;
  /** Project detail tab (`#/projects/:id/:tab`). */
  tab?: string;
}

const VIEWS: View[] = ["chat", "watch", "masters", "projects", "settings"];

export function parseHash(hash: string): Route {
  const parts = hash.replace(/^#\/?/, "").split("/").filter(Boolean);
  const view = (VIEWS as string[]).includes(parts[0]) ? (parts[0] as View) : "chat";
  if (view === "chat") return { view, sessionId: parts[1] };
  if (view === "projects") return { view, projectId: parts[1], tab: parts[2] };
  return { view };
}

export function buildHash(route: Route): string {
  const segs: string[] = [route.view];
  if (route.view === "chat" && route.sessionId) segs.push(route.sessionId);
  if (route.view === "projects" && route.projectId) {
    segs.push(route.projectId);
    if (route.tab) segs.push(route.tab);
  }
  return "#/" + segs.join("/");
}

export type Navigate = (route: Route, opts?: { replace?: boolean }) => void;

/**
 * Dependency-free hash router. Keeps a `Route` in sync with `window.location.hash`,
 * updating on back/forward (via `hashchange`) and on `navigate()`. `replace` uses
 * `history.replaceState` (no new history entry) for the initial redirect to a session.
 */
export function useHashRoute(): [Route, Navigate] {
  const [route, setRoute] = useState<Route>(() =>
    parseHash(typeof location !== "undefined" ? location.hash : ""),
  );

  useEffect(() => {
    const onChange = () => setRoute(parseHash(location.hash));
    window.addEventListener("hashchange", onChange);
    return () => window.removeEventListener("hashchange", onChange);
  }, []);

  const navigate = useCallback<Navigate>((next, opts) => {
    const h = buildHash(next);
    if (opts?.replace) history.replaceState(null, "", h);
    else location.hash = h; // fires hashchange (back/forward-able)
    setRoute(parseHash(h)); // sync immediately (replaceState doesn't fire hashchange)
  }, []);

  return [route, navigate];
}
