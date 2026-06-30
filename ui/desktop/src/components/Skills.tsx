import { useEffect, useState } from "react";
import { MastersClient, type SkillDto } from "../api/client";
import { Card } from "./ui";

/**
 * Read-only list of a project's saved skills (ADR-0006): agent-authored, recall-able procedures
 * stored as `skills/<slug>.md`. Shows name + summary; the full steps are recalled on demand during
 * a run via the `skills.recall_skill` tool.
 */
export function Skills({ client, projectId }: { client: MastersClient; projectId: string }) {
  const [skills, setSkills] = useState<SkillDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    client
      .listSkills(projectId)
      .then(setSkills)
      .catch((e) => setError(String(e)));
  }, [client, projectId]);

  if (error) return <div className="p-4 text-sm text-danger">{error}</div>;
  if (!skills) return <div className="p-4 text-sm text-muted">Loading skills…</div>;
  if (skills.length === 0)
    return <div className="p-4 text-sm text-muted">No skills saved yet.</div>;

  return (
    <div className="space-y-2 overflow-y-auto p-4 text-sm">
      {skills.map((s) => (
        <Card key={s.slug} className="p-3">
          <div className="font-medium text-text">{s.name}</div>
          <p className="text-text">{s.summary}</p>
          <div className="mt-1 text-xs text-faint">{s.slug}</div>
        </Card>
      ))}
    </div>
  );
}
