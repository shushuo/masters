import { useCallback, useEffect, useState } from "react";

/**
 * The primary views (docs/12 §2 — navigation is the user's nouns), mirrored in the URL
 * hash so back/forward and deep links work. The generic surfaces (general chat, projects,
 * masters hub) live under the `lab` (高级工作台) view; legacy hashes (`#/chat`,
 * `#/projects`, `#/masters`) parse into the matching lab sub-route so old links keep working.
 */
export type View = "ask" | "watch" | "briefings" | "settings" | "lab";
export type LabTab = "chat" | "projects" | "masters";

export interface Route {
  view: View;
  /** Active topic (`#/ask/:sessionId`) or lab chat session (`#/lab/chat/:sessionId`). */
  sessionId?: string;
  /** Active lab tab (`#/lab/:tab`). */
  labTab?: LabTab;
  /** Selected project (`#/lab/projects/:id`). */
  projectId?: string;
  /** Project detail tab (`#/lab/projects/:id/:tab`). */
  tab?: string;
}

const VIEWS: View[] = ["ask", "watch", "briefings", "settings", "lab"];
const LAB_TABS: LabTab[] = ["chat", "projects", "masters"];

export function parseHash(hash: string): Route {
  const parts = hash.replace(/^#\/?/, "").split("/").filter(Boolean);
  const head = parts[0];
  // Legacy generic-shell hashes → the lab sub-routes (permanent client-side redirect).
  if (head === "chat") return { view: "lab", labTab: "chat", sessionId: parts[1] };
  if (head === "projects")
    return { view: "lab", labTab: "projects", projectId: parts[1], tab: parts[2] };
  if (head === "masters") return { view: "lab", labTab: "masters" };

  const view = (VIEWS as string[]).includes(head) ? (head as View) : "ask";
  if (view === "ask") return { view, sessionId: parts[1] };
  if (view === "lab") {
    const labTab = (LAB_TABS as string[]).includes(parts[1]) ? (parts[1] as LabTab) : "chat";
    if (labTab === "chat") return { view, labTab, sessionId: parts[2] };
    if (labTab === "projects") return { view, labTab, projectId: parts[2], tab: parts[3] };
    return { view, labTab };
  }
  return { view };
}

export function buildHash(route: Route): string {
  const segs: string[] = [route.view];
  if (route.view === "ask" && route.sessionId) segs.push(route.sessionId);
  if (route.view === "lab") {
    const labTab = route.labTab ?? "chat";
    segs.push(labTab);
    if (labTab === "chat" && route.sessionId) segs.push(route.sessionId);
    if (labTab === "projects" && route.projectId) {
      segs.push(route.projectId);
      if (route.tab) segs.push(route.tab);
    }
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
