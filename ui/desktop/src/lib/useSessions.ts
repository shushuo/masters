import { useCallback, useEffect, useRef, useState } from "react";
import type { MastersClient, SessionDto } from "../api/client";
import type { Navigate, Route } from "./useHashRoute";

/**
 * Owns the chat-session list (lifted out of Chat.tsx so the Sidebar list and the Chat
 * pane share one source of truth). Also ensures a session exists when the user is on the
 * chat view without one — bootstrapping a fresh session and redirecting (replace) to
 * `#/chat/:id`. The bootstrap is guarded against React StrictMode's double-mount.
 */
export function useSessions(client: MastersClient | null, route: Route, navigate: Navigate) {
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

  const newChat = useCallback(
    async (opts?: { replace?: boolean }): Promise<string | null> => {
      if (!client) return null;
      const s = await client.createSession("Desktop chat");
      await refresh();
      navigate({ view: "chat", sessionId: s.id }, opts);
      return s.id;
    },
    [client, refresh, navigate],
  );

  const deleteSession = useCallback(
    async (id: string) => {
      if (!client) return;
      const remaining = sessions.filter((s) => s.id !== id);
      await client.deleteSession(id);
      await refresh();
      // If we deleted the active session, move to the next one (or a fresh chat).
      if (route.view === "chat" && route.sessionId === id) {
        if (remaining[0]) navigate({ view: "chat", sessionId: remaining[0].id });
        else navigate({ view: "chat" });
      }
    },
    [client, sessions, route, refresh, navigate],
  );

  // Initial list load.
  useEffect(() => {
    refresh();
  }, [refresh]);

  // Ensure a session exists whenever we land on chat without one.
  useEffect(() => {
    if (!client || route.view !== "chat" || route.sessionId || creating.current) return;
    creating.current = true;
    newChat({ replace: true }).finally(() => {
      creating.current = false;
    });
  }, [client, route.view, route.sessionId, newChat]);

  return { sessions, refresh, newChat, deleteSession };
}
