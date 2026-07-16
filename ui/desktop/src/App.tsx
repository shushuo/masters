import { useEffect, useState } from "react";
import { MastersClient, type HealthDto } from "./api/client";
import { resolveDaemon } from "./lib/daemon";
import { useHashRoute } from "./lib/useHashRoute";
import { useSessions } from "./lib/useSessions";
import { Chat } from "./components/Chat";
import { Settings } from "./components/Settings";
import { Projects } from "./components/Projects";
import { ProjectDetail } from "./components/ProjectDetail";
import { MastersHub } from "./components/MastersHub";
import { Watch } from "./components/Watch";
import { Briefings } from "./components/Briefings";
import { Onboarding } from "./components/Onboarding";
import { Sidebar } from "./components/Sidebar";
import { checkForUpdate, installUpdate, type Update } from "./lib/updater";

export function App() {
  const [client, setClient] = useState<MastersClient | null>(null);
  const [health, setHealth] = useState<HealthDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [route, navigate] = useHashRoute();
  const { view, sessionId, projectId } = route;
  const [onboarded, setOnboarded] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  // Streaming is lifted here so the sidebar can block session-switching mid-run.
  const [streaming, setStreaming] = useState(false);
  // Lets the user re-launch the setup wizard from Settings (not just on first run).
  const [forceOnboarding, setForceOnboarding] = useState(false);
  // A pending in-app update (Tauri only); surfaced as a dismissible banner.
  const [update, setUpdate] = useState<Update | null>(null);
  const [updating, setUpdating] = useState(false);
  const [updateError, setUpdateError] = useState<string | null>(null);

  const { sessions, refresh, newChat, deleteSession } = useSessions(client, route, navigate);

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

  // Wait for the daemon handshake, then health-check before showing the chat.
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

  return (
    <div className="flex h-full bg-bg text-text">
      <Sidebar
        health={health}
        view={view}
        collapsed={collapsed}
        onToggleCollapse={() => setCollapsed((c) => !c)}
        onNavigate={(v) => navigate({ view: v })}
        client={client}
        sessions={sessions}
        activeSessionId={sessionId ?? null}
        busy={streaming}
        onSelectSession={(id) => navigate({ view: "chat", sessionId: id })}
        onNewChat={() => newChat()}
        onDeleteSession={deleteSession}
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
          <div className="p-6 text-sm text-muted">Starting the Masters daemon…</div>
        ) : forceOnboarding || (needsSetup && view === "chat") ? (
          <Onboarding client={client} onDone={handleOnboarded} />
        ) : view === "settings" ? (
          <Settings
            client={client}
            onClose={() => navigate({ view: "chat" })}
            onRerunSetup={() => setForceOnboarding(true)}
          />
        ) : view === "watch" ? (
          <Watch client={client} />
        ) : view === "briefings" ? (
          <Briefings client={client} />
        ) : view === "masters" ? (
          <MastersHub client={client} />
        ) : view === "projects" ? (
          projectId ? (
            <ProjectDetail
              client={client}
              projectId={projectId}
              onBack={() => navigate({ view: "projects" })}
            />
          ) : (
            <Projects
              client={client}
              onSelect={(id) => navigate({ view: "projects", projectId: id })}
            />
          )
        ) : (
          <Chat
            client={client}
            sessionId={sessionId ?? null}
            streaming={streaming}
            onStreamingChange={setStreaming}
            onActivity={refresh}
          />
        )}
      </main>
    </div>
  );
}
