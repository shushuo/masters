import { useEffect, useState } from "react";
import { Play } from "lucide-react";
import { MastersClient, type RecipeSummaryDto } from "../api/client";
import { Button, Card } from "./ui";

/**
 * Read-only list of a project's recipes (Phase 3c, FR-16): human-authored, parameterized
 * automations stored as `recipes/<name>.yaml`. Each can be run on demand ("run now") — the recipe's
 * prompt seeds the agent loop, auto-approved within the project's grants. Scheduling (3d) and
 * delivery (3e) come later.
 */
export function Recipes({ client, projectId }: { client: MastersClient; projectId: string }) {
  const [recipes, setRecipes] = useState<RecipeSummaryDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState<string | null>(null);
  const [result, setResult] = useState<{ name: string; text: string } | null>(null);

  useEffect(() => {
    client
      .listRecipes(projectId)
      .then(setRecipes)
      .catch((e) => setError(String(e)));
  }, [client, projectId]);

  async function run(name: string) {
    setRunning(name);
    setResult(null);
    setError(null);
    try {
      const res = await client.runRecipe(projectId, name);
      setResult({ name, text: res.message.content });
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(null);
    }
  }

  if (error) return <div className="p-4 text-sm text-danger">{error}</div>;
  if (!recipes) return <div className="p-4 text-sm text-muted">Loading recipes…</div>;
  if (recipes.length === 0)
    return <div className="p-4 text-sm text-muted">No recipes yet.</div>;

  return (
    <div className="space-y-2 overflow-y-auto p-4 text-sm">
      {recipes.map((r) => (
        <Card key={r.name} className="p-3">
          <div className="flex items-center justify-between gap-2">
            <div>
              <div className="font-medium text-text">{r.title}</div>
              <p className="text-text">{r.description}</p>
              <div className="mt-1 text-xs text-faint">{r.name}</div>
            </div>
            <Button
              variant="primary"
              size="sm"
              disabled={running === r.name}
              onClick={() => run(r.name)}
            >
              <Play className="size-3.5" />
              {running === r.name ? "Running…" : "Run now"}
            </Button>
          </div>
          {result?.name === r.name && (
            <pre className="mt-2 whitespace-pre-wrap rounded bg-surface p-2 font-sans text-text">
              {result.text}
            </pre>
          )}
        </Card>
      ))}
    </div>
  );
}
