import { useEffect, useState } from "react";
import {
  MessagesSquare,
  Pencil,
  Sparkles,
  Star,
  Trash2,
  UserRound,
} from "lucide-react";
import {
  MastersClient,
  type AvailableHarnessDto,
  type MasterDto,
  type MasterSummaryDto,
} from "../api/client";
import { Badge, Button, Card } from "./ui";
import { cn } from "./ui/cn";
import { MasterForm, blankMaster } from "./Masters";
import { GroupChat } from "./GroupChat";

type Tab = "explore" | "mine";

/**
 * **Masters** sidebar hub (top-level, no project required). Two tabs:
 *  - **Explore** — the built-in "system master" template gallery; clone one into your collection.
 *  - **My Masters** — your standalone masters (create / edit / delete with the shared ACP-aware
 *    editor), star a default, and **quick chat** with one or many of them.
 *
 * Masters here are global (`<data_home>/masters/`); quick chat runs them against the system default
 * project and reuses the group-chat streaming UI (a single master = a 1:1 chat).
 */
export function MastersHub({ client }: { client: MastersClient }) {
  const [tab, setTab] = useState<Tab>("mine");
  const [templates, setTemplates] = useState<MasterDto[] | null>(null);
  const [masters, setMasters] = useState<MasterSummaryDto[] | null>(null);
  const [harnesses, setHarnesses] = useState<AvailableHarnessDto[]>([]);
  const [draft, setDraft] = useState<MasterDto>(blankMaster);
  const [defaultSlug, setDefaultSlug] = useState<string>("");
  const [selected, setSelected] = useState<string[]>([]);
  const [chat, setChat] = useState<{ masters: string[] } | null>(null);
  const [error, setError] = useState<string | null>(null);

  function refresh() {
    client.listGlobalMasters().then(setMasters).catch((e) => setError(String(e)));
    client.getDefaultMaster().then(setDefaultSlug).catch(() => {});
  }
  useEffect(() => {
    refresh();
    client.listMasterTemplates().then(setTemplates).catch(() => setTemplates([]));
    client.getHarnesses().then(setHarnesses).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client]);

  async function save(master: MasterDto) {
    try {
      await client.createGlobalMaster(master);
      setDraft(blankMaster());
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  async function useTemplate(tpl: MasterDto) {
    try {
      await client.createGlobalMaster({ ...tpl, origin: "imported" });
      setTab("mine");
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  async function startEdit(slug: string) {
    try {
      setDraft(await client.getGlobalMaster(slug));
      setTab("mine");
    } catch (e) {
      setError(String(e));
    }
  }

  async function remove(slug: string) {
    try {
      await client.deleteGlobalMaster(slug);
      setSelected((s) => s.filter((x) => x !== slug));
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  async function star(slug: string) {
    const next = defaultSlug === slug ? "" : slug;
    setDefaultSlug(next); // optimistic
    try {
      await client.setDefaultMaster(next);
    } catch (e) {
      setError(String(e));
      refresh();
    }
  }

  function toggleSelect(slug: string) {
    setSelected((s) => (s.includes(slug) ? s.filter((x) => x !== slug) : [...s, slug]));
  }

  // Quick chat over an explicit master set (1:1 or group). The session is created lazily by the
  // GroupChat panel via `startQuickChat`; the coordinator is the starred default if it's selected.
  if (chat) {
    const coordinator =
      defaultSlug && chat.masters.includes(defaultSlug) ? defaultSlug : chat.masters[0];
    const title =
      chat.masters.length === 1
        ? `Quick chat · ${chat.masters[0]}`
        : `Quick chat · ${chat.masters.length} masters`;
    return (
      <GroupChat
        client={client}
        title={title}
        backLabel="Masters"
        members={chat.masters}
        coordinator={coordinator}
        openSession={() => client.startQuickChat(chat.masters)}
        onClose={() => setChat(null)}
      />
    );
  }

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-2 border-b border-border px-4 py-3">
        <UserRound className="size-5 text-accent" />
        <h1 className="font-display text-lg font-semibold text-text">Masters</h1>
        <span className="text-sm text-muted">
          Explore system masters and create your own — then chat with one or many.
        </span>
      </header>

      <nav className="flex gap-1 border-b border-border px-3 text-sm">
        {(
          [
            { key: "explore", label: "Explore", icon: Sparkles },
            { key: "mine", label: "My Masters", icon: UserRound },
          ] as { key: Tab; label: string; icon: typeof Sparkles }[]
        ).map(({ key, label, icon: Icon }) => (
          <button
            key={key}
            className={cn(
              "relative flex items-center gap-1.5 px-2.5 py-2 transition-colors",
              tab === key
                ? "font-medium text-text after:absolute after:inset-x-0.5 after:-bottom-px after:h-0.5 after:bg-accent"
                : "text-muted hover:text-text",
            )}
            onClick={() => setTab(key)}
          >
            <Icon className="size-4" aria-hidden />
            {label}
          </button>
        ))}
      </nav>

      {error && <div className="px-4 py-2 text-sm text-danger">{error}</div>}

      {tab === "explore" ? (
        <ExploreGallery templates={templates} onUse={useTemplate} />
      ) : (
        <div className="flex-1 space-y-4 overflow-y-auto p-4 text-sm">
          <MasterForm
            key={draft.slug || "new"}
            initial={draft}
            harnesses={harnesses}
            onSubmit={save}
            onCancel={draft.slug ? () => setDraft(blankMaster()) : undefined}
          />

          {selected.length > 0 && (
            <div className="flex items-center justify-between rounded-sm border border-accent-subtle bg-accent-subtle px-3 py-2">
              <span className="text-text">{selected.length} selected</span>
              <div className="flex gap-2">
                <Button variant="ghost" size="sm" onClick={() => setSelected([])}>
                  Clear
                </Button>
                <Button variant="primary" size="sm" onClick={() => setChat({ masters: selected })}>
                  <MessagesSquare className="size-3.5" /> Quick chat ({selected.length})
                </Button>
              </div>
            </div>
          )}

          {!masters ? (
            <div className="text-muted">Loading masters…</div>
          ) : masters.length === 0 ? (
            <div className="text-muted">
              No masters yet — create one above or clone a template from <b>Explore</b>.
            </div>
          ) : (
            masters.map((m) => (
              <MasterRow
                key={m.slug}
                master={m}
                isDefault={defaultSlug === m.slug}
                selected={selected.includes(m.slug)}
                onToggleSelect={() => toggleSelect(m.slug)}
                onStar={() => star(m.slug)}
                onEdit={() => startEdit(m.slug)}
                onDelete={() => remove(m.slug)}
                onChat={() => setChat({ masters: [m.slug] })}
              />
            ))
          )}
        </div>
      )}
    </div>
  );
}

function ExploreGallery({
  templates,
  onUse,
}: {
  templates: MasterDto[] | null;
  onUse: (tpl: MasterDto) => void;
}) {
  if (!templates) return <div className="p-4 text-sm text-muted">Loading templates…</div>;
  if (templates.length === 0)
    return <div className="p-4 text-sm text-muted">No system masters available.</div>;
  return (
    <div className="grid flex-1 grid-cols-1 gap-3 overflow-y-auto p-4 md:grid-cols-2">
      {templates.map((t, i) => (
        <Card key={t.slug || i} className="flex flex-col gap-2 p-3">
          <div className="flex items-center gap-2">
            <UserRound className="size-4 text-accent" />
            <span className="font-medium text-text">{t.name}</span>
            <Badge variant="neutral">system</Badge>
          </div>
          {t.summary && <div className="text-xs text-muted">{t.summary}</div>}
          {t.persona && <p className="line-clamp-3 text-xs text-faint">{t.persona}</p>}
          <div className="mt-auto">
            <Button variant="secondary" size="sm" onClick={() => onUse(t)}>
              <Sparkles className="size-3.5" /> Use template
            </Button>
          </div>
        </Card>
      ))}
    </div>
  );
}

function MasterRow({
  master,
  isDefault,
  selected,
  onToggleSelect,
  onStar,
  onEdit,
  onDelete,
  onChat,
}: {
  master: MasterSummaryDto;
  isDefault: boolean;
  selected: boolean;
  onToggleSelect: () => void;
  onStar: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onChat: () => void;
}) {
  const isAcp = master.backend === "acp";
  return (
    <Card className="flex items-center gap-3 p-3">
      <input
        type="checkbox"
        checked={selected}
        onChange={onToggleSelect}
        aria-label={`Select ${master.name} for quick chat`}
        className="size-4 shrink-0 accent-accent"
      />
      <button
        onClick={onStar}
        title={isDefault ? "Default master (used when none is picked)" : "Set as default master"}
        aria-label={isDefault ? "Unset default master" : "Set as default master"}
        className={cn("shrink-0", isDefault ? "text-accent" : "text-faint hover:text-text")}
      >
        <Star className={cn("size-4", isDefault && "fill-current")} />
      </button>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 font-medium text-text">
          {master.name}
          {isAcp && <Badge variant="accent">ACP</Badge>}
          {isDefault && <Badge variant="accent">default</Badge>}
        </div>
        <div className="truncate text-xs text-muted">
          <span className="font-mono">
            {isAcp ? "external coding agent" : master.default_model || "default model"}
          </span>
          {master.summary ? ` · ${master.summary}` : ""}
        </div>
      </div>
      <div className="flex shrink-0 gap-1">
        <Button variant="ghost" size="sm" onClick={onChat}>
          <MessagesSquare className="size-3.5" /> Chat
        </Button>
        <Button variant="ghost" size="sm" onClick={onEdit}>
          <Pencil className="size-3.5" /> Edit
        </Button>
        <Button variant="ghost" size="sm" className="text-danger" onClick={onDelete}>
          <Trash2 className="size-3.5" /> Delete
        </Button>
      </div>
    </Card>
  );
}
