import { useEffect, useState } from "react";
import { MastersClient, type MemoryDto } from "../api/client";
import { Card } from "./ui";

/**
 * Read-only view of a project's durable memory (ADR-0007): the `USER.md` profile and the
 * `MEMORY.md` facts the agent has captured. The files on disk are the source of truth; this
 * surfaces the indexed sections so the user can see (and, in their editor, edit) what's remembered.
 */
export function Memory({ client, projectId }: { client: MastersClient; projectId: string }) {
  const [memories, setMemories] = useState<MemoryDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    client
      .listMemories(projectId)
      .then(setMemories)
      .catch((e) => setError(String(e)));
  }, [client, projectId]);

  if (error) return <div className="p-4 text-sm text-danger">{error}</div>;
  if (!memories) return <div className="p-4 text-sm text-muted">Loading memory…</div>;
  if (memories.length === 0)
    return <div className="p-4 text-sm text-muted">Nothing remembered yet.</div>;

  const profile = memories.filter((m) => m.scope === "user");
  const facts = memories.filter((m) => m.scope !== "user");

  return (
    <div className="space-y-4 overflow-y-auto p-4 text-sm">
      {profile.length > 0 && (
        <section>
          <h3 className="mb-2 text-xs font-semibold uppercase text-muted">User profile</h3>
          {profile.map((m) => (
            <MemoryCard key={`u-${m.title}`} memory={m} />
          ))}
        </section>
      )}
      <section>
        <h3 className="mb-2 text-xs font-semibold uppercase text-muted">Memory</h3>
        {facts.map((m) => (
          <MemoryCard key={`f-${m.title}`} memory={m} />
        ))}
      </section>
    </div>
  );
}

function MemoryCard({ memory }: { memory: MemoryDto }) {
  return (
    <Card className="mb-2 p-3">
      <div className="font-medium text-text">{memory.title}</div>
      <p className="whitespace-pre-wrap text-text">{memory.body}</p>
      <div className="mt-1 text-xs text-faint">{memory.source}</div>
    </Card>
  );
}
