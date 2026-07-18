import { useCallback, useEffect, useState } from "react";
import { MastersClient, type HealthDto, type InvestingWorkspaceDto } from "./api/client";
import { resolveDaemon } from "./lib/daemon";
import { useHashRoute } from "./lib/useHashRoute";
import { useSessions } from "./lib/useSessions";
import { AskHome } from "./components/AskHome";
import { Lab } from "./components/Lab";
import { Settings } from "./components/Settings";
import { Watch } from "./components/Watch";
import { Briefings } from "./components/Briefings";
import SimLab from "./components/SimLab";
import { Onboarding } from "./components/Onboarding";
import { Sidebar } from "./components/Sidebar";
import { checkForUpdate, installUpdate, type Update } from "./lib/updater";

const BRIEFINGS_SEEN_KEY = "getmasters-briefings-seen";

export function App() {
  const [client, setClient] = useState<MastersClient | null>(null);
  const [health, setHealth] = useState<HealthDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [route, navigate] = useHashRoute();
  const { view, sessionId } = route;
  const [onboarded, setOnboarded] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  // Streaming is lifted here so the sidebar can block topic-switching mid-run.
  const [streaming, setStreaming] = useState(false);
  // Lets the user re-launch the setup wizard from Settings (not just on first run).
  const [forceOnboarding, setForceOnboarding] = useState(false);
  // A pending in-app update (Tauri only); surfaced as a dismissible banner.
  const [update, setUpdate] = useState<Update | null>(null);
  const [updating, setUpdating] = useState(false);
  const [updateError, setUpdateError] = useState<string | null>(null);
  // The investing workspace (lazily seeded, idempotent) — drives topic filtering + Watch/Briefings.
  const [workspace, setWorkspace] = useState<InvestingWorkspaceDto | null>(null);
  // A question handed to 问大师 from another view (Watch/Briefings 「就此提问」).
  const [askDraft, setAskDraft] = useState<string | undefined>(undefined);
  // Quiet unread hint on 动静 (docs/12 §4: a 2px dot, never a count).
  const [hasNewBriefings, setHasNewBriefings] = useState(false);

  const { topics, labSessions, refresh, newLabChat, deleteSession } = useSessions(
    client,
    route,
    navigate,
    workspace?.team_slug ?? null,
  );

  // The daemon starts even without a usable provider (reporting `configured: false`); in that
  // state we auto-open the setup wizard so the user can add a provider key. Settings ("Re-run
  // setup") can also launch it on demand.
  const needsSetup = health != null && !health.configured && !onboarded;

  async function handleOnboarded() {
    setOnboarded(true);
    setForceOnboarding(false);
    if (client) {
      try {
        setHealth(await client.health());
      } catch {
        /* keep prior health on refresh failure */
      }
    }
  }

  // Wait for the daemon handshake, then health-check before showing the app.
  useEffect(() => {
    let cancelled = false;
    resolveDaemon()
      .then(async (conn) => {
        const c = new MastersClient(conn);
        const h = await c.health();
        if (!cancelled) {
          setClient(c);
          setHealth(h);
        }
      })
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, []);

  // Seed/load the investing workspace once (idempotent; no LLM call involved) — the sidebar
  // needs its team slug to filter topics, and the unread hint needs its project id.
  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    client
      .ensureInvestingWorkspace()
      .then(async (ws) => {
        if (cancelled) return;
        setWorkspace(ws);
        try {
          const briefings = await client.listBriefings(ws.project_id);
          const latest = briefings[0]?.started_at ?? 0;
          const seen = Number(localStorage.getItem(BRIEFINGS_SEEN_KEY) ?? 0);
          if (!cancelled) setHasNewBriefings(latest > seen);
        } catch {
          /* the dot just stays off */
        }
      })
      .catch(() => {
        /* non-fatal: 问大师 ensures it again on render */
      });
    return () => {
      cancelled = true;
    };
  }, [client]);

  // Check for a newer release once at startup (no-op outside the Tauri shell).
  useEffect(() => {
    let cancelled = false;
    checkForUpdate()
      .then((u) => !cancelled && setUpdate(u))
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  async function applyUpdate() {
    if (!update) return;
    setUpdating(true);
    setUpdateError(null);
    try {
      await installUpdate(update);
    } catch (e) {
      setUpdateError(String(e));
      setUpdating(false);
    }
  }

  /** Jump into 问大师 with a pre-filled question (a fresh topic). */
  const openAsk = useCallback(
    (draft?: string) => {
      setAskDraft(draft);
      navigate({ view: "ask" });
    },
    [navigate],
  );

  const markBriefingsSeen = useCallback((latest: number) => {
    try {
      localStorage.setItem(BRIEFINGS_SEEN_KEY, String(latest));
    } catch {
      /* best-effort */
    }
    setHasNewBriefings(false);
  }, []);

  return (
    <div className="flex h-full gap-1 bg-bg text-text">
      <Sidebar
        health={health}
        view={view}
        collapsed={collapsed}
        onToggleCollapse={() => setCollapsed((c) => !c)}
        onNavigate={(v) => navigate({ view: v })}
        client={client}
        topics={topics}
        activeSessionId={view === "ask" ? (sessionId ?? null) : null}
        busy={streaming}
        hasNewBriefings={hasNewBriefings}
        onSelectTopic={(id) => navigate({ view: "ask", sessionId: id })}
        onNewTopic={() => openAsk()}
        onDeleteTopic={deleteSession}
      />

      <main className="flex h-full min-w-0 flex-1 flex-col overflow-hidden">
        {update && (
          <div className="flex flex-wrap items-center gap-3 border-b border-border bg-surface px-4 py-2 text-sm text-text">
            <span>
              A new version <span className="font-medium">v{update.version}</span> is available.
            </span>
            <button
              className="rounded bg-accent px-2 py-1 text-xs font-medium text-accent-fg disabled:opacity-60"
              onClick={applyUpdate}
              disabled={updating}
            >
              {updating ? "Updating…" : "Update & restart"}
            </button>
            <button
              className="text-xs text-muted hover:text-text"
              onClick={() => setUpdate(null)}
              disabled={updating}
            >
              Later
            </button>
            {updateError && <span className="text-xs text-danger-fg">{updateError}</span>}
          </div>
        )}
        {error ? (
          <div className="m-4 rounded border border-danger bg-danger-bg p-3 text-sm text-danger-fg">
            {error}
          </div>
        ) : !client ? (
          <div className="p-6 text-sm text-muted">正在唤醒守护…</div>
        ) : forceOnboarding || (needsSetup && view === "ask") ? (
          <Onboarding client={client} onDone={handleOnboarded} />
        ) : view === "settings" ? (
          <Settings
            client={client}
            onClose={() => navigate({ view: "ask" })}
            onRerunSetup={() => setForceOnboarding(true)}
            onOpenLab={() => navigate({ view: "lab", labTab: "chat" })}
          />
        ) : view === "watch" ? (
          <Watch client={client} onAsk={openAsk} />
        ) : view === "briefings" ? (
          <Briefings client={client} onAsk={openAsk} onSeen={markBriefingsSeen} />
        ) : view === "simlab" ? (
          <SimLab client={client} onAsk={openAsk} />
        ) : view === "lab" ? (
          <Lab
            client={client}
            route={route}
            navigate={navigate}
            labSessions={labSessions}
            streaming={streaming}
            onStreamingChange={setStreaming}
            onNewChat={() => newLabChat()}
            onActivity={refresh}
          />
        ) : (
          <AskHome
            client={client}
            sessionId={sessionId ?? null}
            draft={askDraft}
            onSessionCreated={(id) => {
              setAskDraft(undefined);
              navigate({ view: "ask", sessionId: id }, { replace: true });
              refresh();
            }}
            onActivity={refresh}
          />
        )}
      </main>
    </div>
  );
}
