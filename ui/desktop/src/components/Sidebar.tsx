import { useState } from "react";
import {
  FolderKanban,
  MessagesSquare,
  Monitor,
  Moon,
  PanelLeft,
  PanelLeftClose,
  Settings as SettingsIcon,
  Sun,
  UserRound,
  type LucideIcon,
} from "lucide-react";
import type { MastersClient, HealthDto } from "../api/client";
import { IconButton, PandaMark } from "./ui";
import { cn } from "./ui/cn";
import { applyTheme, getTheme, nextTheme, type Theme } from "../lib/theme";

type View = "chat" | "settings" | "projects" | "masters";

const NAV: { key: View; label: string; icon: LucideIcon }[] = [
  { key: "chat", label: "Chat", icon: MessagesSquare },
  { key: "masters", label: "Masters", icon: UserRound },
  { key: "projects", label: "Projects", icon: FolderKanban },
];

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

/**
 * Notion-style left rail: brand, primary navigation, and a daemon-status footer.
 * Owns no routing state — App.tsx remains the single source of truth and passes
 * the active view + navigation callback down.
 */
export function Sidebar({
  health,
  view,
  collapsed,
  onToggleCollapse,
  onNavigate,
  client,
}: {
  health: HealthDto | null;
  view: View;
  selectedProjectId: string | null;
  collapsed: boolean;
  onToggleCollapse: () => void;
  onNavigate: (view: View) => void;
  client: MastersClient | null;
}) {
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
          <span className="flex-1 text-base font-semibold tracking-tight text-text">
            Masters
          </span>
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

      <div className="flex-1" />

      {/* Settings (pinned bottom) with the theme toggle to its right */}
      {client && (
        <div
          className={cn(
            "flex gap-0.5 px-2 py-1",
            collapsed ? "flex-col items-center" : "items-center",
          )}
        >
          <div className={collapsed ? "" : "flex-1"}>
            <NavButton
              active={false}
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
