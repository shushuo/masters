import { useEffect, useState } from "react";
import { Download, MessagesSquare, Pencil, Play, Plus, Route, Trash2, Upload } from "lucide-react";
import {
  MastersClient,
  type MasterSummaryDto,
  type RouteResultDto,
  type TeamBundle,
  type TeamDto,
  type TeamSummaryDto,
} from "../api/client";
import { GroupChat } from "./GroupChat";
import { Button, Card, Input } from "./ui";
import { cn } from "./ui/cn";

/**
 * Master Teams (Phase 4b, FR-38/40): a group of masters + a coordinator. The router (`route_brief`)
 * ranks the team's masters against a brief — auto-select with manual override. A team run dispatches
 * the chosen master via the gated single-master run; a group chat fans a brief out to the addressed
 * masters (4c+). Teams can be created, edited (re-saved under the same name), exported, and imported.
 */
export function Teams({ client, projectId }: { client: MastersClient; projectId: string }) {
  const [teams, setTeams] = useState<TeamSummaryDto[] | null>(null);
  const [masters, setMasters] = useState<MasterSummaryDto[]>([]);
  const [editing, setEditing] = useState<TeamDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [chatTeam, setChatTeam] = useState<string | null>(null);

  function refresh() {
    client.listTeams(projectId).then(setTeams).catch((e) => setError(String(e)));
  }
  useEffect(() => {
    refresh();
    client.listMasters(projectId).then(setMasters).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client, projectId]);

  async function startEdit(slug: string) {
    try {
      setEditing(await client.getTeam(projectId, slug));
    } catch (e) {
      setError(String(e));
    }
  }

  if (chatTeam)
    return (
      <GroupChat
        client={client}
        projectId={projectId}
        teamSlug={chatTeam}
        backLabel="Teams"
        onClose={() => setChatTeam(null)}
      />
    );

  if (error) return <div className="p-4 text-sm text-danger">{error}</div>;
  if (!teams) return <div className="p-4 text-sm text-muted">Loading teams…</div>;

  return (
    <div className="space-y-4 overflow-y-auto p-4 text-sm">
      {masters.length === 0 ? (
        <div className="text-sm text-muted">Add masters first, then group them into a team.</div>
      ) : (
        <TeamForm
          key={editing?.slug ?? "new"}
          client={client}
          projectId={projectId}
          masters={masters}
          initial={editing}
          onSaved={() => {
            setEditing(null);
            refresh();
          }}
          onCancel={editing ? () => setEditing(null) : undefined}
          onError={setError}
        />
      )}
      <ImportForm client={client} projectId={projectId} onImported={refresh} onError={setError} />
      {teams.length === 0 ? (
        <div className="text-sm text-muted">No teams yet.</div>
      ) : (
        teams.map((t) => (
          <TeamRow
            key={t.slug}
            client={client}
            projectId={projectId}
            team={t}
            onChanged={refresh}
            onEdit={() => startEdit(t.slug)}
            onError={setError}
            onChat={() => setChatTeam(t.slug)}
          />
        ))
      )}
    </div>
  );
}

function TeamForm({
  client,
  projectId,
  masters,
  initial,
  onSaved,
  onCancel,
  onError,
}: {
  client: MastersClient;
  projectId: string;
  masters: MasterSummaryDto[];
  initial: TeamDto | null;
  onSaved: () => void;
  onCancel?: () => void;
  onError: (e: string) => void;
}) {
  const editing = !!initial;
  const [name, setName] = useState(initial?.name ?? "");
  const [members, setMembers] = useState<string[]>(initial?.members ?? []);
  const [coordinator, setCoordinator] = useState(initial?.coordinator_slug ?? "");

  function toggle(slug: string) {
    setMembers((m) => (m.includes(slug) ? m.filter((s) => s !== slug) : [...m, slug]));
  }

  async function save() {
    if (!name || members.length === 0) return;
    try {
      await client.createTeam(projectId, {
        name,
        summary: initial?.summary ?? "",
        coordinator_slug: coordinator || members[0],
        members,
      });
      if (!editing) {
        setName("");
        setMembers([]);
        setCoordinator("");
      }
      onSaved();
    } catch (e) {
      onError(String(e));
    }
  }

  return (
    <Card className="space-y-2 p-3">
      <div className="font-medium text-text">{editing ? `Edit ${initial?.name}` : "New team"}</div>
      <Input placeholder="Team name" value={name} onChange={(e) => setName(e.target.value)} />
      <div className="space-y-1">
        <div className="text-xs text-muted">
          Members (the coordinator answers unaddressed briefs):
        </div>
        {masters.map((x) => (
          <label key={x.slug} className="flex items-center gap-2">
            <input
              type="checkbox"
              className="size-4 accent-accent"
              checked={members.includes(x.slug)}
              onChange={() => toggle(x.slug)}
            />
            <span className="text-text">{x.name}</span>
            {members.includes(x.slug) && (
              <button
                className={cn(
                  "ml-auto rounded-sm px-1.5 py-0.5 text-xs",
                  coordinator === x.slug ? "bg-accent text-accent-fg" : "text-accent hover:bg-surface-2",
                )}
                onClick={() => setCoordinator(x.slug)}
              >
                {coordinator === x.slug ? "coordinator" : "make coordinator"}
              </button>
            )}
          </label>
        ))}
      </div>
      <div className="flex gap-2">
        <Button variant="primary" size="sm" disabled={!name || members.length === 0} onClick={save}>
          {editing ? (
            <>
              <Pencil className="size-3.5" /> Save changes
            </>
          ) : (
            <>
              <Plus className="size-3.5" /> Create team
            </>
          )}
        </Button>
        {onCancel && (
          <Button variant="ghost" size="sm" onClick={onCancel}>
            Cancel
          </Button>
        )}
      </div>
    </Card>
  );
}

function TeamRow({
  client,
  projectId,
  team,
  onChanged,
  onEdit,
  onError,
  onChat,
}: {
  client: MastersClient;
  projectId: string;
  team: TeamSummaryDto;
  onChanged: () => void;
  onEdit: () => void;
  onError: (e: string) => void;
  onChat: () => void;
}) {
  const [brief, setBrief] = useState("");
  const [routed, setRouted] = useState<RouteResultDto | null>(null);
  const [output, setOutput] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function route() {
    if (!brief) return;
    setBusy(true);
    setOutput(null);
    try {
      setRouted(await client.routeTeam(projectId, team.slug, brief));
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function run() {
    if (!brief) return;
    setBusy(true);
    setOutput(null);
    try {
      const res = await client.runTeam(projectId, team.slug, brief);
      setRouted(null);
      setOutput(`(${res.selected_slug}) ${res.result.message.content}`);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function remove() {
    try {
      await client.deleteTeam(projectId, team.slug);
      onChanged();
    } catch (e) {
      onError(String(e));
    }
  }
  async function exportBundle() {
    try {
      const bundle = await client.exportTeamBundle(projectId, team.slug);
      const blob = new Blob([JSON.stringify(bundle, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${team.slug}.bundle.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      onError(String(e));
    }
  }

  return (
    <Card className="p-3">
      <div className="flex items-center justify-between">
        <div>
          <div className="font-medium text-text">{team.name}</div>
          <div className="text-xs text-muted">
            {team.member_count} member{team.member_count === 1 ? "" : "s"}
            {team.summary ? ` · ${team.summary}` : ""}
          </div>
        </div>
        <div className="flex gap-1">
          <Button variant="ghost" size="sm" onClick={onChat}>
            <MessagesSquare className="size-3.5" /> Chat
          </Button>
          <Button variant="ghost" size="sm" onClick={onEdit}>
            <Pencil className="size-3.5" /> Edit
          </Button>
          <Button variant="ghost" size="sm" onClick={exportBundle}>
            <Download className="size-3.5" /> Export
          </Button>
          <Button variant="ghost" size="sm" className="text-danger" onClick={remove}>
            <Trash2 className="size-3.5" /> Delete
          </Button>
        </div>
      </div>
      <div className="mt-2 flex gap-2">
        <Input
          className="flex-1"
          placeholder="Brief for the team…"
          value={brief}
          onChange={(e) => setBrief(e.target.value)}
        />
        <Button variant="secondary" size="sm" disabled={!brief || busy} onClick={route}>
          <Route className="size-3.5" /> Route
        </Button>
        <Button variant="primary" size="sm" disabled={!brief || busy} onClick={run}>
          <Play className="size-3.5" /> Run
        </Button>
      </div>
      {routed && (
        <div className="mt-2 text-xs">
          <div className="text-muted">
            Selected: <span className="font-medium text-text">{routed.selected_slug}</span>
          </div>
          {routed.ranked.map((r) => (
            <div key={r.slug} className="flex justify-between text-muted">
              <span>{r.name}</span>
              <span className="text-faint">{r.score.toFixed(1)}</span>
            </div>
          ))}
        </div>
      )}
      {output && (
        <pre className="mt-2 whitespace-pre-wrap rounded bg-surface p-2 font-sans text-text">
          {output}
        </pre>
      )}
    </Card>
  );
}

/** Import a portable team bundle (a `.bundle.json` exported from any project) into this project. */
function ImportForm({
  client,
  projectId,
  onImported,
  onError,
}: {
  client: MastersClient;
  projectId: string;
  onImported: () => void;
  onError: (e: string) => void;
}) {
  const [note, setNote] = useState<string | null>(null);

  async function onFile(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    e.target.value = ""; // allow re-importing the same file
    if (!file) return;
    setNote(null);
    try {
      const bundle = JSON.parse(await file.text()) as TeamBundle;
      const res = await client.importBundle(projectId, bundle);
      setNote(`Imported "${res.team_slug}" + ${res.masters.length} master(s).`);
      onImported();
    } catch (err) {
      onError(`Import failed: ${err}`);
    }
  }

  return (
    <label className="flex cursor-pointer items-center gap-2 rounded-sm border border-dashed border-border-strong px-2.5 py-1.5 text-xs text-accent hover:bg-surface-2">
      <Upload className="size-3.5" /> Import bundle…
      <input type="file" accept="application/json,.json" className="hidden" onChange={onFile} />
      {note && <span className="text-muted">{note}</span>}
    </label>
  );
}
