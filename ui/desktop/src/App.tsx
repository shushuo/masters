import { useEffect, useState } from "react";
import { MastersClient, type HealthDto } from "./api/client";
import { resolveDaemon } from "./lib/daemon";
import { Chat } from "./components/Chat";
import { Settings } from "./components/Settings";
import { Projects } from "./components/Projects";
import { ProjectDetail } from "./components/ProjectDetail";
import { MastersHub } from "./components/MastersHub";
import { Onboarding } from "./components/Onboarding";
import { Sidebar } from "./components/Sidebar";

type View = "chat" | "settings" | "projects" | "masters";

export function App() {
  const [client, setClient] = useState<MastersClient | null>(null);
  const [health, setHealth] = useState<HealthDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>("chat");
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(null);
  const [onboarded, setOnboarded] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  // Lets the user re-launch the setup wizard from Settings (not just on first run).
  const [forceOnboarding, setForceOnboarding] = useState(false);

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

  return (
    <div className="flex h-full bg-bg text-text">
      <Sidebar
        health={health}
        view={view}
        selectedProjectId={selectedProjectId}
        collapsed={collapsed}
        onToggleCollapse={() => setCollapsed((c) => !c)}
        onNavigate={(v) => {
          if (v === "projects") setSelectedProjectId(null);
          setView(v);
        }}
        client={client}
      />

      <main className="flex h-full min-w-0 flex-1 flex-col overflow-hidden">
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
            onClose={() => setView("chat")}
            onRerunSetup={() => setForceOnboarding(true)}
          />
        ) : view === "masters" ? (
          <MastersHub client={client} />
        ) : view === "projects" ? (
          selectedProjectId ? (
            <ProjectDetail
              client={client}
              projectId={selectedProjectId}
              onBack={() => setSelectedProjectId(null)}
            />
          ) : (
            <Projects client={client} onSelect={setSelectedProjectId} />
          )
        ) : (
          <Chat client={client} />
        )}
      </main>
    </div>
  );
}
