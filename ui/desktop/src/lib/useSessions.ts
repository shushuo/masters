import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { MastersClient, SessionDto } from "../api/client";
import type { Navigate, Route } from "./useHashRoute";

/** Headless/system session-title prefixes that must never appear in user-facing lists. */
const HIDDEN_PREFIXES = ["recipe:", "master:", "group:", "quick-"];

function isListable(s: SessionDto): boolean {
  const title = s.title ?? "";
  return !HIDDEN_PREFIXES.some((p) => title.startsWith(p));
}

/**
 * Owns the session lists (lifted out of the panes so the Sidebar and the views share one
 * source of truth). Splits the raw list into the two user-facing lists (docs/12 §2):
 *  - `topics` — 问大师 topics: team-bound sessions of the investing team,
 *  - `labSessions` — generic (non-team) chats for the advanced workbench.
 * Also bootstraps a generic session when the user lands on lab-chat without one.
 */
export function useSessions(
  client: MastersClient | null,
  route: Route,
  navigate: Navigate,
  investingTeamSlug: string | null,
) {
  const [sessions, setSessions] = useState<SessionDto[]>([]);
  const creating = useRef(false);

  const refresh = useCallback(async () => {
    if (!client) return;
    try {
      setSessions(await client.listSessions());
    } catch {
      /* non-fatal: the list just stays as-is */
    }
  }, [client]);

  const topics = useMemo(
    () =>
      sessions.filter(
        (s) =>
          s.team_slug != null &&
          (investingTeamSlug == null || s.team_slug === investingTeamSlug) &&
          isListable(s),
      ),
    [sessions, investingTeamSlug],
  );

  const labSessions = useMemo(
    () => sessions.filter((s) => s.team_slug == null && isListable(s)),
    [sessions],
  );

  const newLabChat = useCallback(
    async (opts?: { replace?: boolean }): Promise<string | null> => {
      if (!client) return null;
      const s = await client.createSession("Desktop chat");
      await refresh();
      navigate({ view: "lab", labTab: "chat", sessionId: s.id }, opts);
      return s.id;
    },
    [client, refresh, navigate],
  );

  const deleteSession = useCallback(
    async (id: string) => {
      if (!client) return;
      await client.deleteSession(id);
      await refresh();
      // If we deleted the active topic/chat, fall back to the view's empty state.
      if (route.sessionId === id) {
        if (route.view === "ask") navigate({ view: "ask" });
        else if (route.view === "lab") navigate({ view: "lab", labTab: "chat" });
      }
    },
    [client, route, refresh, navigate],
  );

  // Initial list load.
  useEffect(() => {
    refresh();
  }, [refresh]);

  // Ensure a generic session exists whenever we land on lab-chat without one.
  // (问大师 deliberately does NOT bootstrap — its empty state is the new-topic screen;
  // the topic session is created on first send.)
  useEffect(() => {
    if (
      !client ||
      route.view !== "lab" ||
      (route.labTab ?? "chat") !== "chat" ||
      route.sessionId ||
      creating.current
    )
      return;
    creating.current = true;
    newLabChat({ replace: true }).finally(() => {
      creating.current = false;
    });
  }, [client, route.view, route.labTab, route.sessionId, newLabChat]);

  return { sessions, topics, labSessions, refresh, newLabChat, deleteSession };
}
