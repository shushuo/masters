import { useEffect, useState } from "react";
import { Cpu, Pencil, Play, Plus, Trash2 } from "lucide-react";
import {
  MastersClient,
  type AvailableHarnessDto,
  type MasterDto,
  type MasterSummaryDto,
} from "../api/client";
import { Badge, Button, Card, Input, Select, Textarea } from "./ui";

/**
 * Masters (Phase 4a/4i, FR-39/46): persona-over-Skill role descriptors stored as `masters/<slug>.md`.
 * Define a master — either an **internal** agent (persona + a provider-qualified model + an optional
 * tool allow-list) or an **ACP** external coding harness (Claude Code / Codex / OpenCode / Gemini,
 * ADR-0014) driven over stdio with its file/permission callbacks routed through the gate. Then hand it
 * a brief — it runs as an isolated, gated subagent. Existing masters can be edited or removed.
 */
export function Masters({ client, projectId }: { client: MastersClient; projectId: string }) {
  const [masters, setMasters] = useState<MasterSummaryDto[] | null>(null);
  const [harnesses, setHarnesses] = useState<AvailableHarnessDto[]>([]);
  // The form's working draft: a blank master (create) or a loaded one (edit).
  const [draft, setDraft] = useState<MasterDto>(blankMaster);
  const [error, setError] = useState<string | null>(null);

  function refresh() {
    client.listMasters(projectId).then(setMasters).catch((e) => setError(String(e)));
  }
  useEffect(() => {
    refresh();
    client.getHarnesses().then(setHarnesses).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client, projectId]);

  async function startEdit(slug: string) {
    try {
      setDraft(await client.getMaster(projectId, slug));
    } catch (e) {
      setError(String(e));
    }
  }

  async function save(master: MasterDto) {
    try {
      await client.createMaster(projectId, master);
      setDraft(blankMaster());
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  if (error) return <div className="p-4 text-sm text-danger">{error}</div>;
  if (!masters) return <div className="p-4 text-sm text-muted">Loading masters…</div>;

  const editingSlug = draft.slug;

  return (
    <div className="space-y-4 overflow-y-auto p-4 text-sm">
      <MasterForm
        key={editingSlug || "new"}
        initial={draft}
        harnesses={harnesses}
        onSubmit={save}
        onCancel={editingSlug ? () => setDraft(blankMaster()) : undefined}
      />
      {masters.length === 0 ? (
        <div className="text-sm text-muted">No masters yet.</div>
      ) : (
        masters.map((e) => (
          <MasterRow
            key={e.slug}
            client={client}
            projectId={projectId}
            master={e}
            onEdit={() => startEdit(e.slug)}
            onChanged={refresh}
            onError={setError}
          />
        ))
      )}
    </div>
  );
}

export function blankMaster(): MasterDto {
  return {
    slug: "",
    name: "",
    summary: "",
    persona: "",
    default_model: "anthropic:claude-opus-4-8",
    allowed_skills: [],
    allowed_tools: [],
    output_contract: "",
    origin: "builtin",
    body: "",
    backend: "internal",
    acp_command: "",
    acp_args: [],
    acp_env: [],
  };
}

export function MasterForm({
  initial,
  harnesses,
  onSubmit,
  onCancel,
}: {
  initial: MasterDto;
  harnesses: AvailableHarnessDto[];
  onSubmit: (master: MasterDto) => void;
  onCancel?: () => void;
}) {
  const editing = !!initial.slug;
  const [name, setName] = useState(initial.name);
  const [persona, setPersona] = useState(initial.persona);
  const [model, setModel] = useState(initial.default_model || "anthropic:claude-opus-4-8");
  const [tools, setTools] = useState((initial.allowed_tools ?? []).join(", "));
  const [backend, setBackend] = useState(initial.backend || "internal");
  const [acpCommand, setAcpCommand] = useState(initial.acp_command ?? "");
  const [acpArgs, setAcpArgs] = useState((initial.acp_args ?? []).join(" "));
  const [acpEnv, setAcpEnv] = useState(
    (initial.acp_env ?? []).map(([k, v]) => `${k}=${v}`).join("\n"),
  );

  const isAcp = backend === "acp";
  const canSubmit = !!name && (isAcp ? !!acpCommand : !!persona);

  function applyHarness(h: AvailableHarnessDto) {
    setBackend("acp");
    setAcpCommand(h.suggested_command);
    setAcpArgs(h.suggested_args.join(" "));
  }

  function submit() {
    if (!canSubmit) return;
    const env: [string, string][] = acpEnv
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean)
      .map((line) => {
        const i = line.indexOf("=");
        return [line.slice(0, i), line.slice(i + 1)] as [string, string];
      })
      .filter((kv) => kv[0]);
    onSubmit({
      ...initial,
      name,
      persona,
      default_model: model,
      allowed_tools: tools
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean),
      backend,
      acp_command: isAcp ? acpCommand : "",
      acp_args: isAcp ? acpArgs.split(/\s+/).filter(Boolean) : [],
      acp_env: isAcp ? env : [],
    });
    if (!editing) {
      setName("");
      setPersona("");
      setTools("");
      setAcpCommand("");
      setAcpArgs("");
      setAcpEnv("");
    }
  }

  return (
    <Card className="space-y-2 p-3">
      <div className="flex items-center justify-between">
        <span className="font-medium text-text">{editing ? `Edit ${initial.name}` : "New master"}</span>
        <Select className="w-auto" value={backend} onChange={(e) => setBackend(e.target.value)}>
          <option value="internal">Internal agent</option>
          <option value="acp">External coding agent (ACP)</option>
        </Select>
      </div>

      <div className="flex gap-2">
        <Input
          className="w-48"
          placeholder="Master name"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        {!isAcp && (
          <Input
            className="flex-1 font-mono"
            placeholder="anthropic:claude-opus-4-8"
            value={model}
            onChange={(e) => setModel(e.target.value)}
          />
        )}
      </div>

      <Textarea
        placeholder={
          isAcp
            ? "Persona (optional for ACP): extra instructions prepended to the harness prompt…"
            : "Persona: voice, masterise, stance…"
        }
        rows={2}
        value={persona}
        onChange={(e) => setPersona(e.target.value)}
      />

      {isAcp ? (
        <div className="space-y-2 rounded-sm border border-border bg-surface p-2">
          {harnesses.length > 0 && (
            <div className="flex flex-wrap items-center gap-1.5">
              <span className="text-xs text-muted">Detected:</span>
              {harnesses.map((h) => (
                <Button
                  key={h.id}
                  variant="secondary"
                  size="sm"
                  disabled={!h.available}
                  title={h.available ? `Use ${h.command}` : `${h.command} not on PATH — ${h.homepage}`}
                  onClick={() => applyHarness(h)}
                >
                  <Cpu className="size-3.5" /> {h.display_name}
                  {!h.available && <span className="text-faint">(not installed)</span>}
                </Button>
              ))}
            </div>
          )}
          <Input
            className="font-mono"
            placeholder="acp command (e.g. claude-code-acp)"
            value={acpCommand}
            onChange={(e) => setAcpCommand(e.target.value)}
          />
          <Input
            className="font-mono"
            placeholder="args (space-separated)"
            value={acpArgs}
            onChange={(e) => setAcpArgs(e.target.value)}
          />
          <Textarea
            className="font-mono"
            placeholder="env, one KEY=value per line (inherited daemon env + these; trusted local agent)"
            rows={2}
            value={acpEnv}
            onChange={(e) => setAcpEnv(e.target.value)}
          />
        </div>
      ) : (
        <Input
          className="font-mono"
          placeholder="Allowed tools (comma-separated, e.g. files.read, knowledge.search)"
          value={tools}
          onChange={(e) => setTools(e.target.value)}
        />
      )}

      <div className="flex gap-2">
        <Button variant="primary" size="sm" disabled={!canSubmit} onClick={submit}>
          {editing ? (
            <>
              <Pencil className="size-3.5" /> Save changes
            </>
          ) : (
            <>
              <Plus className="size-3.5" /> Add master
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

function MasterRow({
  client,
  projectId,
  master,
  onEdit,
  onChanged,
  onError,
}: {
  client: MastersClient;
  projectId: string;
  master: MasterSummaryDto;
  onEdit: () => void;
  onChanged: () => void;
  onError: (e: string) => void;
}) {
  const [brief, setBrief] = useState("");
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const isAcp = master.backend === "acp";

  async function run() {
    if (!brief) return;
    setRunning(true);
    setResult(null);
    try {
      const res = await client.runMaster(projectId, master.slug, brief);
      setResult(res.message.content);
    } catch (e) {
      onError(String(e));
    } finally {
      setRunning(false);
    }
  }
  async function remove() {
    try {
      await client.deleteMaster(projectId, master.slug);
      onChanged();
    } catch (e) {
      onError(String(e));
    }
  }

  return (
    <Card className="p-3">
      <div className="flex items-center justify-between">
        <div>
          <div className="flex items-center gap-2 font-medium text-text">
            {master.name}
            {isAcp && <Badge variant="accent">ACP</Badge>}
          </div>
          <div className="text-xs text-muted">
            <span className="font-mono">
              {isAcp ? "external coding agent" : master.default_model || "default model"}
            </span>
            {master.summary ? ` · ${master.summary}` : ""}
          </div>
        </div>
        <div className="flex gap-1">
          <Button variant="ghost" size="sm" onClick={onEdit}>
            <Pencil className="size-3.5" /> Edit
          </Button>
          <Button variant="ghost" size="sm" className="text-danger" onClick={remove}>
            <Trash2 className="size-3.5" /> Delete
          </Button>
        </div>
      </div>
      <div className="mt-2 flex gap-2">
        <Input
          className="flex-1"
          placeholder="Brief for this master…"
          value={brief}
          onChange={(e) => setBrief(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && run()}
        />
        <Button variant="primary" size="sm" disabled={!brief || running} onClick={run}>
          <Play className="size-3.5" /> {running ? "Running…" : "Run"}
        </Button>
      </div>
      {result && (
        <pre className="mt-2 whitespace-pre-wrap rounded bg-surface p-2 font-sans text-text">
          {result}
        </pre>
      )}
    </Card>
  );
}
