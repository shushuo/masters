import { useEffect, useState } from "react";
import {
  ArrowLeft,
  BookOpen,
  Brain,
  CalendarClock,
  ChefHat,
  FileText,
  GraduationCap,
  Puzzle,
  Sparkles,
  UserRound,
  Users,
  type LucideIcon,
} from "lucide-react";
import {
  MastersClient,
  type ExtensionDto,
  type KnowledgeStatusDto,
  type ProjectDto,
} from "../api/client";
import { Memory } from "./Memory";
import { Skills } from "./Skills";
import { Study } from "./Study";
import { Recipes } from "./Recipes";
import { Routines } from "./Routines";
import { Masters } from "./Masters";
import { Teams } from "./Teams";
import { Connectors } from "./Connectors";
import { Button, Textarea } from "./ui";
import { cn } from "./ui/cn";

type Tab =
  | "instructions"
  | "knowledge"
  | "memory"
  | "skills"
  | "study"
  | "recipes"
  | "routines"
  | "masters"
  | "teams"
  | "extensions";

/**
 * The detail view for one project (ADR-0011 context container): editable instructions, a read-only
 * knowledge-index view, the durable Memory + Skills views, the Study decks view, the Recipes
 * automations view, the Routines (scheduled recipes) view, and the FR-19 extensions toggles.
 */
export function ProjectDetail({
  client,
  projectId,
  onBack,
}: {
  client: MastersClient;
  projectId: string;
  onBack: () => void;
}) {
  const [project, setProject] = useState<ProjectDto | null>(null);
  const [tab, setTab] = useState<Tab>("instructions");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    client
      .getProject(projectId)
      .then(setProject)
      .catch((e) => setError(String(e)));
  }, [client, projectId]);

  const tabs: { key: Tab; icon: LucideIcon }[] = [
    { key: "instructions", icon: FileText },
    { key: "knowledge", icon: BookOpen },
    { key: "memory", icon: Brain },
    { key: "skills", icon: Sparkles },
    { key: "study", icon: GraduationCap },
    { key: "recipes", icon: ChefHat },
    { key: "routines", icon: CalendarClock },
    { key: "masters", icon: UserRound },
    { key: "teams", icon: Users },
    { key: "extensions", icon: Puzzle },
  ];

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-border px-4 py-2">
        <Button variant="ghost" size="sm" onClick={onBack}>
          <ArrowLeft className="size-4" /> Projects
        </Button>
        <h2 className="text-sm font-semibold text-text">{project?.name ?? "…"}</h2>
      </div>
      <nav className="flex gap-1 overflow-x-auto border-b border-border px-3 text-sm">
        {tabs.map(({ key, icon: Icon }) => (
          <button
            key={key}
            className={cn(
              "relative flex shrink-0 items-center gap-1.5 px-2.5 py-2 capitalize transition-colors",
              tab === key
                ? "font-medium text-text after:absolute after:inset-x-2.5 after:-bottom-px after:h-0.5 after:bg-accent"
                : "text-muted hover:text-text",
            )}
            onClick={() => setTab(key)}
          >
            <Icon className="size-4" aria-hidden />
            {key}
          </button>
        ))}
      </nav>

      <div className="flex-1 overflow-y-auto">
        {error && <div className="p-4 text-sm text-danger">{error}</div>}
        {tab === "instructions" && <Instructions client={client} projectId={projectId} />}
        {tab === "knowledge" && <Knowledge client={client} projectId={projectId} />}
        {tab === "memory" && <Memory client={client} projectId={projectId} />}
        {tab === "skills" && <Skills client={client} projectId={projectId} />}
        {tab === "study" && <Study client={client} projectId={projectId} />}
        {tab === "recipes" && <Recipes client={client} projectId={projectId} />}
        {tab === "routines" && <Routines client={client} projectId={projectId} />}
        {tab === "masters" && <Masters client={client} projectId={projectId} />}
        {tab === "teams" && <Teams client={client} projectId={projectId} />}
        {tab === "extensions" && <Extensions client={client} projectId={projectId} />}
      </div>
    </div>
  );
}

function Instructions({ client, projectId }: { client: MastersClient; projectId: string }) {
  const [text, setText] = useState("");
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    client.getProject(projectId).then((p) => setText(p.instructions ?? ""));
  }, [client, projectId]);

  async function save() {
    setStatus("Saving…");
    try {
      await client.setInstructions(projectId, text);
      setStatus("Saved. Applies on the project's next session.");
    } catch (e) {
      setStatus(`Error: ${String(e)}`);
    }
  }

  return (
    <div className="space-y-2 p-4 text-sm">
      <p className="text-muted">
        Instructions are auto-injected into every session under this project (project-scoped, ranked
        above global).
      </p>
      <Textarea
        className="h-48 font-mono text-xs"
        value={text}
        onChange={(e) => setText(e.target.value)}
      />
      <Button variant="primary" onClick={save}>
        Save
      </Button>
      {status && <p className="text-muted">{status}</p>}
    </div>
  );
}

function Knowledge({ client, projectId }: { client: MastersClient; projectId: string }) {
  const [status, setStatus] = useState<KnowledgeStatusDto | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    client
      .getKnowledgeStatus(projectId)
      .then(setStatus)
      .catch((e) => setError(String(e)));
  }, [client, projectId]);

  if (error) return <div className="p-4 text-sm text-danger">{error}</div>;
  if (!status) return <div className="p-4 text-sm text-muted">Loading knowledge…</div>;

  return (
    <div className="space-y-3 p-4 text-sm">
      <div className="text-muted">
        {status.documents} document(s) · {status.chunks} chunk(s) indexed. Ingest documents by asking
        the agent in a chat (it stays gated/audited).
      </div>
      {status.paths.length === 0 ? (
        <p className="text-faint">Nothing indexed yet.</p>
      ) : (
        <ul className="space-y-1">
          {status.paths.map((d) => (
            <li key={d.path} className="rounded border border-border p-2">
              <div className="truncate font-mono text-xs text-text">{d.path}</div>
              <div className="text-xs text-faint">{d.mime ?? "?"}</div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Extensions({ client, projectId }: { client: MastersClient; projectId: string }) {
  const [exts, setExts] = useState<ExtensionDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    client
      .getExtensions(projectId)
      .then(setExts)
      .catch((e) => setError(String(e)));
  }, [client, projectId]);

  async function toggle(name: string, enabled: boolean) {
    try {
      const updated = await client.setExtension(projectId, name, enabled);
      setExts((prev) => prev?.map((e) => (e.name === name ? updated : e)) ?? null);
    } catch (e) {
      setError(String(e));
    }
  }

  if (error) return <div className="p-4 text-sm text-danger">{error}</div>;
  if (!exts) return <div className="p-4 text-sm text-muted">Loading extensions…</div>;

  return (
    <div className="space-y-2 p-4 text-sm">
      <p className="text-muted">
        Toggle which built-in tools this project's sessions can use (FR-19). Disabling{" "}
        <code>files</code> removes all file read/write tools.
      </p>
      {exts.map((e) => (
        <label
          key={e.name}
          className={cn(
            "flex cursor-pointer items-center justify-between rounded border border-border p-2",
            !e.implemented && "cursor-default opacity-50",
          )}
        >
          <span className="capitalize text-text">
            {e.name}
            {!e.implemented && <span className="ml-2 text-xs text-faint">(coming soon)</span>}
          </span>
          <input
            type="checkbox"
            className="size-4 accent-accent"
            checked={e.enabled}
            disabled={!e.implemented}
            onChange={(ev) => toggle(e.name, ev.target.checked)}
          />
        </label>
      ))}
      <Connectors client={client} projectId={projectId} />
    </div>
  );
}
