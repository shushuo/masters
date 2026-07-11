import { useEffect, useState } from "react";
import {
  FolderKanban,
  MessagesSquare,
  Monitor,
  Moon,
  PanelLeft,
  PanelLeftClose,
  Plus,
  Settings as SettingsIcon,
  Sun,
  Trash2,
  UserRound,
  type LucideIcon,
} from "lucide-react";
import type { MastersClient, HealthDto, SessionDto } from "../api/client";
import { IconButton, PandaMark } from "./ui";
import { cn } from "./ui/cn";
import { applyTheme, getTheme, nextTheme, type Theme } from "../lib/theme";
import type { View } from "../lib/useHashRoute";

const NAV: { key: View; label: string; icon: LucideIcon }[] = [
  { key: "chat", label: "Chat", icon: MessagesSquare },
  { key: "masters", label: "Masters", icon: UserRound },
  { key: "projects", label: "Projects", icon: FolderKanban },
];

/** Compact relative time for the session list (no i18n dep). */
function formatRelative(ts: number): string {
  const s = Math.round((Date.now() - ts) / 1000);
  if (s < 60) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.round(h / 24);
  if (d < 7) return `${d}d ago`;
  return new Date(ts).toLocaleDateString();
}

/** A single nav-rail entry — shared by the top content nav and the pinned-bottom Settings entry. */
function NavButton({
  active,
  label,
  icon: Icon,
  collapsed,
  onClick,
}: {
  active: boolean;
  label: string;
  icon: LucideIcon;
  collapsed: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      title={collapsed ? label : undefined}
      aria-label={label}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex w-full items-center gap-2.5 rounded-sm px-2.5 py-1.5 text-sm transition-colors",
        collapsed && "justify-center",
        active
          ? "bg-accent-subtle font-medium text-accent"
          : "text-muted hover:bg-surface-2 hover:text-text",
      )}
    >
      <Icon className="size-4 shrink-0" aria-hidden />
      {!collapsed && <span>{label}</span>}
    </button>
  );
}

/** The chat-session list, shown under the Chat nav item when the Chat view is active. */
function SessionList({
  sessions,
  activeSessionId,
  busy,
  onSelect,
  onNewChat,
  onDelete,
}: {
  sessions: SessionDto[];
  activeSessionId: string | null;
  busy: boolean;
  onSelect: (id: string) => void;
  onNewChat: () => void;
  onDelete: (id: string) => void;
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col px-2">
      <button
        onClick={onNewChat}
        disabled={busy}
        className="mb-1 flex items-center gap-2 rounded-sm px-2.5 py-1.5 text-sm text-muted transition-colors hover:bg-surface-2 hover:text-text disabled:opacity-50"
      >
        <Plus className="size-4 shrink-0" aria-hidden /> New chat
      </button>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {sessions.length === 0 ? (
          <p className="px-2.5 py-1 text-xs text-faint">No chats yet.</p>
        ) : (
          sessions.map((s) => {
            const active = s.id === activeSessionId;
            return (
              <div
                key={s.id}
                className={cn(
                  "group flex items-center gap-1 rounded-sm pr-1 text-sm transition-colors",
                  active ? "bg-accent-subtle" : "hover:bg-surface-2",
                )}
              >
                <button
                  onClick={() => !busy && onSelect(s.id)}
                  disabled={busy}
                  aria-current={active ? "page" : undefined}
                  className="flex min-w-0 flex-1 flex-col items-start px-2.5 py-1.5 text-left disabled:opacity-60"
                >
                  <span
                    className={cn(
                      "w-full truncate",
                      active ? "font-medium text-accent" : "text-text",
                    )}
                  >
                    {s.title || "Untitled"}
                  </span>
                  <span className="text-[11px] text-faint">{formatRelative(s.updated_at)}</span>
                </button>
                <IconButton
                  label="Delete chat"
                  onClick={() => onDelete(s.id)}
                  className="opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
                >
                  <Trash2 className="size-3.5" />
                </IconButton>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}

/**
 * Notion-style left rail: brand, primary navigation, a contextual chat-session list, and a
 * daemon-status footer. Owns no routing state — App.tsx passes the active view + navigation
 * callbacks (and, for the Chat view, the session list + its actions) down.
 */
export function Sidebar({
  health,
  view,
  collapsed,
  onToggleCollapse,
  onNavigate,
  client,
  sessions,
  activeSessionId,
  busy,
  onSelectSession,
  onNewChat,
  onDeleteSession,
}: {
  health: HealthDto | null;
  view: View;
  collapsed: boolean;
  onToggleCollapse: () => void;
  onNavigate: (view: View) => void;
  client: MastersClient | null;
  sessions: SessionDto[];
  activeSessionId: string | null;
  busy: boolean;
  onSelectSession: (id: string) => void;
  onNewChat: () => void;
  onDeleteSession: (id: string) => void;
}) {
  const showSessions = view === "chat" && !collapsed && client != null;

  // ⌘/Ctrl+N starts a new chat from anywhere.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "n") {
        e.preventDefault();
        if (!busy) onNewChat();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onNewChat]);

  return (
    <aside
      className={cn(
        "flex h-full flex-col border-r border-border bg-surface transition-[width] duration-150",
        collapsed ? "w-14" : "w-60",
      )}
    >
      {/* Brand */}
      <div className="flex items-center gap-2 px-3 py-3">
        <PandaMark className="size-6 shrink-0" />
        {!collapsed && (
          <span className="flex-1 text-base font-semibold tracking-tight text-text">Masters</span>
        )}
        <IconButton
          label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          onClick={onToggleCollapse}
          className={collapsed ? "mx-auto" : ""}
        >
          {collapsed ? <PanelLeft className="size-4" /> : <PanelLeftClose className="size-4" />}
        </IconButton>
      </div>

      {/* Primary nav */}
      {client && (
        <nav className="flex flex-col gap-0.5 px-2 py-1">
          {NAV.map(({ key, label, icon }) => (
            <NavButton
              key={key}
              active={view === key}
              label={label}
              icon={icon}
              collapsed={collapsed}
              onClick={() => onNavigate(key)}
            />
          ))}
        </nav>
      )}

      {/* Contextual chat-session list (Chat view only), else a flexible spacer. */}
      {showSessions ? (
        <SessionList
          sessions={sessions}
          activeSessionId={activeSessionId}
          busy={busy}
          onSelect={onSelectSession}
          onNewChat={onNewChat}
          onDelete={onDeleteSession}
        />
      ) : (
        <div className="flex-1" />
      )}

      {/* Settings (pinned bottom) with the theme toggle to its right */}
      {client && (
        <div
          className={cn(
            "flex gap-0.5 border-t border-border px-2 py-1",
            collapsed ? "flex-col items-center" : "items-center",
          )}
        >
          <div className={collapsed ? "" : "flex-1"}>
            <NavButton
              active={view === "settings"}
              label="Settings"
              icon={SettingsIcon}
              collapsed={collapsed}
              onClick={() => onNavigate("settings")}
            />
          </div>
          {/* Theme toggle: cycle system → light → dark */}
          <ThemeToggle collapsed />
        </div>
      )}

      {/* Daemon status footer */}
      <div
        className={cn(
          "flex items-center gap-2 border-t border-border px-3 py-2.5 text-xs text-faint",
          collapsed && "justify-center",
        )}
        title={health ? `daemon ok · ${health.provider} · v${health.version}` : "connecting…"}
      >
        <span
          className={cn(
            "size-2 shrink-0 rounded-full",
            health ? "bg-success" : "animate-pulse bg-faint",
          )}
          aria-hidden
        />
        {!collapsed && (
          <span className="truncate">
            {health ? `${health.provider} · v${health.version}` : "connecting…"}
          </span>
        )}
      </div>
    </aside>
  );
}

const THEME_META: Record<Theme, { icon: LucideIcon; label: string }> = {
  system: { icon: Monitor, label: "Theme: system" },
  light: { icon: Sun, label: "Theme: light" },
  dark: { icon: Moon, label: "Theme: dark" },
};

function ThemeToggle({ collapsed }: { collapsed: boolean }) {
  const [theme, setTheme] = useState<Theme>(getTheme);
  const { icon: Icon, label } = THEME_META[theme];

  function cycle() {
    const next = nextTheme(theme);
    applyTheme(next);
    setTheme(next);
  }

  if (collapsed) {
    return (
      <IconButton label={label} onClick={cycle}>
        <Icon className="size-4" />
      </IconButton>
    );
  }
  return (
    <button
      onClick={cycle}
      className="flex w-full items-center gap-2.5 rounded-sm px-2.5 py-1.5 text-sm text-muted transition-colors hover:bg-surface-2 hover:text-text"
    >
      <Icon className="size-4 shrink-0" aria-hidden />
      <span className="capitalize">{theme}</span>
    </button>
  );
}
