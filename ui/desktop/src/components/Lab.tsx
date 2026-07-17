import { Plus } from "lucide-react";
import type { MastersClient, SessionDto } from "../api/client";
import type { LabTab, Navigate, Route } from "../lib/useHashRoute";
import { Chat } from "./Chat";
import { Projects } from "./Projects";
import { ProjectDetail } from "./ProjectDetail";
import { MastersHub } from "./MastersHub";
import { Button, Select } from "./ui";
import { cn } from "./ui/cn";
import { t } from "../lib/i18n";

const TABS: { key: LabTab; labelKey: "lab.chat" | "lab.projects" | "lab.masters" }[] = [
  { key: "chat", labelKey: "lab.chat" },
  { key: "projects", labelKey: "lab.projects" },
  { key: "masters", labelKey: "lab.masters" },
];

/**
 * 高级工作台 (docs/12 §3.4) — the thin shell that keeps the generic product surfaces
 * (general single-agent chat, projects, the masters hub) fully available while the primary
 * navigation stays investing-first. The inner components are unchanged; this only hosts them.
 */
export function Lab({
  client,
  route,
  navigate,
  labSessions,
  streaming,
  onStreamingChange,
  onNewChat,
  onActivity,
}: {
  client: MastersClient;
  route: Route;
  navigate: Navigate;
  labSessions: SessionDto[];
  streaming: boolean;
  onStreamingChange: (v: boolean) => void;
  onNewChat: () => void;
  onActivity: () => void;
}) {
  const tab = route.labTab ?? "chat";

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-border px-4 py-2">
        <span className="text-sm font-medium text-text">{t("lab.title")}</span>
        <div className="ml-2 flex gap-1">
          {TABS.map(({ key, labelKey }) => (
            <button
              key={key}
              onClick={() => navigate({ view: "lab", labTab: key })}
              aria-current={tab === key ? "page" : undefined}
              className={cn(
                "rounded-sm px-2.5 py-1 text-sm transition-colors",
                tab === key
                  ? "bg-accent-subtle font-medium text-accent"
                  : "text-muted hover:bg-surface-2 hover:text-text",
              )}
            >
              {t(labelKey)}
            </button>
          ))}
        </div>
        {tab === "chat" && (
          <div className="ml-auto flex items-center gap-2">
            <Select
              className="w-auto max-w-56"
              value={route.sessionId ?? ""}
              disabled={streaming}
              onChange={(e) =>
                e.target.value &&
                navigate({ view: "lab", labTab: "chat", sessionId: e.target.value })
              }
            >
              {!route.sessionId && <option value="">…</option>}
              {labSessions.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.title || "Untitled"}
                </option>
              ))}
            </Select>
            <Button variant="secondary" size="sm" disabled={streaming} onClick={onNewChat}>
              <Plus className="size-4" /> {t("lab.newChat")}
            </Button>
          </div>
        )}
      </div>

      <div className="min-h-0 flex-1">
        {tab === "chat" ? (
          <Chat
            client={client}
            sessionId={route.sessionId ?? null}
            streaming={streaming}
            onStreamingChange={onStreamingChange}
            onActivity={onActivity}
          />
        ) : tab === "projects" ? (
          route.projectId ? (
            <ProjectDetail
              client={client}
              projectId={route.projectId}
              onBack={() => navigate({ view: "lab", labTab: "projects" })}
            />
          ) : (
            <Projects
              client={client}
              onSelect={(id) => navigate({ view: "lab", labTab: "projects", projectId: id })}
            />
          )
        ) : (
          <MastersHub client={client} />
        )}
      </div>
    </div>
  );
}
