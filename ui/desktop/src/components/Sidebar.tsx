import { useEffect, useState } from "react";
import {
  MessagesSquare,
  Newspaper,
  Star,
  Monitor,
  Moon,
  PanelLeft,
  PanelLeftClose,
  Plus,
  Settings as SettingsIcon,
  Sun,
  Trash2,
  type LucideIcon,
} from "lucide-react";
import type { MastersClient, HealthDto, SessionDto } from "../api/client";
import { IconButton, Wordmark } from "./ui";
import { cn } from "./ui/cn";
import { applyTheme, getTheme, nextTheme, type Theme } from "../lib/theme";
import type { View } from "../lib/useHashRoute";
import { t } from "../lib/i18n";

/** Compact relative time for the topic list. */
function formatRelative(ts: number): string {
  const s = Math.round((Date.now() - ts) / 1000);
  if (s < 60) return "刚刚";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} 分钟前`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} 小时前`;
  const d = Math.round(h / 24);
  if (d < 7) return `${d} 天前`;
  return new Date(ts).toLocaleDateString();
}

/** A topic's display title: the saved title, unless it's a system default. */
function topicTitle(s: SessionDto): string {
  const title = (s.title ?? "").trim();
  if (!title || title.startsWith("group:")) return t("sidebar.untitledTopic");
  return title;
}

/** A single nav-rail entry — shared by the primary nav and the pinned-bottom Settings entry. */
function NavButton({
  active,
  label,
  icon: Icon,
  collapsed,
  dot,
  onClick,
}: {
  active: boolean;
  label: string;
  icon: LucideIcon;
  collapsed: boolean;
  /** A quiet unread hint (docs/12 §4: one 2px dot, never a count badge). */
  dot?: boolean;
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
      <span className="relative inline-flex shrink-0">
        <Icon className="size-4" aria-hidden />
        {dot && (
          <span
            className="absolute -right-0.5 -top-0.5 size-1.5 rounded-full bg-accent"
            aria-hidden
          />
        )}
      </span>
      {!collapsed && <span>{label}</span>}
    </button>
  );
}

/** The 问大师 topic list, shown under the Ask nav item when the Ask view is active. */
function TopicList({
  topics,
  activeSessionId,
  busy,
  onSelect,
  onNewTopic,
  onDelete,
}: {
  topics: SessionDto[];
  activeSessionId: string | null;
  busy: boolean;
  onSelect: (id: string) => void;
  onNewTopic: () => void;
  onDelete: (id: string) => void;
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col px-2">
      <button
        onClick={onNewTopic}
        disabled={busy}
        className="mb-1 flex items-center gap-2 rounded-sm px-2.5 py-1.5 text-sm text-muted transition-colors hover:bg-surface-2 hover:text-text disabled:opacity-50"
      >
        <Plus className="size-4 shrink-0" aria-hidden /> {t("sidebar.newTopic")}
      </button>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {topics.length === 0 ? (
          <p className="px-2.5 py-1 text-xs text-faint">{t("sidebar.noTopics")}</p>
        ) : (
          topics.map((s) => {
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
                    {topicTitle(s)}
                  </span>
                  <span className="text-[11px] text-faint">{formatRelative(s.updated_at)}</span>
                </button>
                <IconButton
                  label={t("sidebar.deleteTopic")}
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
 * The 《大师》 left rail (docs/12 §2/§3): wordmark, the three user-noun nav items
 * (问大师 / 关注 / 动静), the contextual topic list, pinned Settings + theme, and the
 * guardian-status footer. Owns no routing state — App.tsx passes the active view +
 * navigation callbacks down.
 */
export function Sidebar({
  health,
  view,
  collapsed,
  onToggleCollapse,
  onNavigate,
  client,
  topics,
  activeSessionId,
  busy,
  hasNewBriefings,
  onSelectTopic,
  onNewTopic,
  onDeleteTopic,
}: {
  health: HealthDto | null;
  view: View;
  collapsed: boolean;
  onToggleCollapse: () => void;
  onNavigate: (view: View) => void;
  client: MastersClient | null;
  topics: SessionDto[];
  activeSessionId: string | null;
  busy: boolean;
  hasNewBriefings: boolean;
  onSelectTopic: (id: string) => void;
  onNewTopic: () => void;
  onDeleteTopic: (id: string) => void;
}) {
  const showTopics = view === "ask" && !collapsed && client != null;

  const nav: { key: View; label: string; icon: LucideIcon; dot?: boolean }[] = [
    { key: "ask", label: t("nav.ask"), icon: MessagesSquare },
    { key: "watch", label: t("nav.watch"), icon: Star },
    { key: "briefings", label: t("nav.briefings"), icon: Newspaper, dot: hasNewBriefings },
  ];

  // ⌘/Ctrl+N starts a new topic from anywhere.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "n") {
        e.preventDefault();
        if (!busy) onNewTopic();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onNewTopic]);

  return (
    <aside
      className={cn(
        "m-2 mr-0 flex flex-col rounded-lg border border-border bg-surface shadow-sm transition-[width] duration-150",
        collapsed ? "w-14" : "w-60",
      )}
    >
      {/* Brand */}
      <div className="flex items-center gap-2 px-3 py-3">
        {collapsed ? (
          <Wordmark size="sm" className="mx-auto [&>*:not(:first-child)]:hidden" />
        ) : (
          <Wordmark size="md" className="flex-1" />
        )}
        <IconButton
          label={collapsed ? t("sidebar.expand") : t("sidebar.collapse")}
          onClick={onToggleCollapse}
          className={collapsed ? "mx-auto" : ""}
        >
          {collapsed ? <PanelLeft className="size-4" /> : <PanelLeftClose className="size-4" />}
        </IconButton>
      </div>

      {/* Primary nav */}
      {client && (
        <nav className="flex flex-col gap-0.5 px-2 py-1">
          {nav.map(({ key, label, icon, dot }) => (
            <NavButton
              key={key}
              active={view === key}
              label={label}
              icon={icon}
              collapsed={collapsed}
              dot={dot}
              onClick={() => onNavigate(key)}
            />
          ))}
        </nav>
      )}

      {/* Contextual topic list (Ask view only), else a flexible spacer. */}
      {showTopics ? (
        <TopicList
          topics={topics}
          activeSessionId={activeSessionId}
          busy={busy}
          onSelect={onSelectTopic}
          onNewTopic={onNewTopic}
          onDelete={onDeleteTopic}
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
              label={t("nav.settings")}
              icon={SettingsIcon}
              collapsed={collapsed}
              onClick={() => onNavigate("settings")}
            />
          </div>
          {/* Theme toggle: cycle system → light → dark */}
          <ThemeToggle collapsed />
        </div>
      )}

      {/* Guardian status footer (docs/12 §6: 「守护中 · 本地」). */}
      <div
        className={cn(
          "flex items-center gap-2 border-t border-border px-3 py-2.5 text-xs text-faint",
          collapsed && "justify-center",
        )}
        title={
          health
            ? `${t("sidebar.guarding")} · ${health.provider} · v${health.version}`
            : t("sidebar.connecting")
        }
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
            {health ? t("sidebar.guarding") : t("sidebar.connecting")}
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
