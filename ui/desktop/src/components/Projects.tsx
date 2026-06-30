import { useEffect, useState } from "react";
import { ChevronRight, Folder, FolderPlus, Plus } from "lucide-react";
import { MastersClient, type ProjectDto } from "../api/client";
import { Button, Input } from "./ui";

/**
 * Project picker (ADR-0011): list the context-container projects, create new ones, and select one
 * to open its detail view (instructions, knowledge, memory, skills, extensions).
 */
export function Projects({
  client,
  onSelect,
}: {
  client: MastersClient;
  onSelect: (projectId: string) => void;
}) {
  const [projects, setProjects] = useState<ProjectDto[] | null>(null);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);

  function refresh() {
    client
      .listProjects()
      .then(setProjects)
      .catch((e) => setError(String(e)));
  }

  useEffect(refresh, [client]);

  async function create() {
    if (!name.trim()) return;
    try {
      const p = await client.createProject(name.trim());
      setName("");
      refresh();
      onSelect(p.id);
    } catch (e) {
      setError(String(e));
    }
  }

  if (error) return <div className="p-6 text-sm text-danger">{error}</div>;
  if (!projects) return <div className="p-6 text-sm text-muted">Loading projects…</div>;

  return (
    <div className="mx-auto w-full max-w-3xl space-y-6 overflow-y-auto p-6 text-sm">
      <div>
        <div className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-faint">
          Projects
        </div>
        <div className="flex gap-2">
          <Input
            className="flex-1"
            placeholder="New project name…"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && create()}
          />
          <Button variant="primary" onClick={create} disabled={!name.trim()}>
            <Plus className="size-4" /> Create
          </Button>
        </div>
      </div>

      {projects.length === 0 ? (
        <div className="flex flex-col items-center gap-2 py-12 text-center">
          <FolderPlus className="size-8 text-faint" aria-hidden />
          <p className="text-muted">No projects yet — create one above.</p>
        </div>
      ) : (
        <ul className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          {projects.map((p) => (
            <li key={p.id}>
              <button
                className="flex w-full items-center gap-3 rounded border border-border bg-bg p-4 text-left transition-colors hover:border-border-strong hover:bg-surface-2"
                onClick={() => onSelect(p.id)}
              >
                <Folder className="size-5 shrink-0 text-faint" aria-hidden />
                <div className="min-w-0 flex-1">
                  <div className="truncate font-medium text-text">{p.name}</div>
                  {p.instructions && (
                    <div className="truncate text-xs text-faint">{p.instructions}</div>
                  )}
                </div>
                <ChevronRight className="size-4 shrink-0 text-faint" aria-hidden />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
