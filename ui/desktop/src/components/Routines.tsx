import { useCallback, useEffect, useState } from "react";
import {
  MastersClient,
  type RecipeSummaryDto,
  type ScheduleDto,
  type ScheduledRunDto,
} from "../api/client";
import { Button, Card, Input, Select } from "./ui";

/**
 * Routines (Phase 3d, FR-17): the project's recipe schedules. Each fires a recipe once at a time or
 * on a cron expression while the daemon is running. Lists schedules with enable/disable + delete, a
 * minimal create form, and recent run history.
 */
export function Routines({ client, projectId }: { client: MastersClient; projectId: string }) {
  const [schedules, setSchedules] = useState<ScheduleDto[] | null>(null);
  const [recipes, setRecipes] = useState<RecipeSummaryDto[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    client.listSchedules(projectId).then(setSchedules).catch((e) => setError(String(e)));
  }, [client, projectId]);

  useEffect(() => {
    refresh();
    client.listRecipes(projectId).then(setRecipes).catch(() => {});
  }, [client, projectId, refresh]);

  if (error) return <div className="p-4 text-sm text-danger">{error}</div>;
  if (!schedules) return <div className="p-4 text-sm text-muted">Loading routines…</div>;

  return (
    <div className="space-y-4 overflow-y-auto p-4 text-sm">
      <CreateForm
        client={client}
        projectId={projectId}
        recipes={recipes}
        onCreated={refresh}
        onError={setError}
      />
      {schedules.length === 0 ? (
        <div className="text-sm text-muted">No routines scheduled.</div>
      ) : (
        schedules.map((s) => (
          <ScheduleRow
            key={s.id}
            client={client}
            projectId={projectId}
            schedule={s}
            onChanged={refresh}
            onError={setError}
          />
        ))
      )}
    </div>
  );
}

function CreateForm({
  client,
  projectId,
  recipes,
  onCreated,
  onError,
}: {
  client: MastersClient;
  projectId: string;
  recipes: RecipeSummaryDto[];
  onCreated: () => void;
  onError: (e: string) => void;
}) {
  const [recipe, setRecipe] = useState("");
  const [cron, setCron] = useState("0 9 * * 1");
  const [notify, setNotify] = useState(false);
  const [email, setEmail] = useState(false);

  async function create() {
    if (!recipe) return;
    try {
      await client.createSchedule(projectId, {
        recipe_name: recipe,
        kind: "cron",
        cron_expr: cron,
        deliver_notify: notify,
        deliver_email: email,
      });
      onCreated();
    } catch (e) {
      onError(String(e));
    }
  }

  return (
    <Card className="flex flex-wrap items-end gap-2 p-2">
      <Select
        className="w-auto"
        value={recipe}
        onChange={(e) => setRecipe(e.target.value)}
      >
        <option value="">Pick a recipe…</option>
        {recipes.map((r) => (
          <option key={r.name} value={r.name}>
            {r.title}
          </option>
        ))}
      </Select>
      <Input
        className="w-40 font-mono"
        value={cron}
        onChange={(e) => setCron(e.target.value)}
        placeholder="0 9 * * 1"
      />
      <label className="flex items-center gap-1 text-xs text-muted">
        <input
          type="checkbox"
          className="size-4 accent-accent"
          checked={notify}
          onChange={(e) => setNotify(e.target.checked)}
        />
        Notify
      </label>
      <label className="flex items-center gap-1 text-xs text-muted">
        <input
          type="checkbox"
          className="size-4 accent-accent"
          checked={email}
          onChange={(e) => setEmail(e.target.checked)}
        />
        Email
      </label>
      <Button variant="primary" size="sm" disabled={!recipe} onClick={create}>
        Schedule
      </Button>
    </Card>
  );
}

function ScheduleRow({
  client,
  projectId,
  schedule,
  onChanged,
  onError,
}: {
  client: MastersClient;
  projectId: string;
  schedule: ScheduleDto;
  onChanged: () => void;
  onError: (e: string) => void;
}) {
  const [runs, setRuns] = useState<ScheduledRunDto[] | null>(null);

  const cadence =
    schedule.kind === "cron" ? schedule.cron_expr ?? "cron" : "once";
  const next = schedule.next_run_at
    ? new Date(schedule.next_run_at).toLocaleString()
    : "—";

  async function toggle() {
    try {
      await client.setSchedule(projectId, schedule.id, { enabled: !schedule.enabled });
      onChanged();
    } catch (e) {
      onError(String(e));
    }
  }
  async function setDelivery(patch: { deliver_notify?: boolean; deliver_email?: boolean }) {
    try {
      await client.setSchedule(projectId, schedule.id, patch);
      onChanged();
    } catch (e) {
      onError(String(e));
    }
  }
  async function remove() {
    try {
      await client.deleteSchedule(projectId, schedule.id);
      onChanged();
    } catch (e) {
      onError(String(e));
    }
  }
  async function loadRuns() {
    try {
      setRuns(await client.scheduleRuns(projectId, schedule.id));
    } catch (e) {
      onError(String(e));
    }
  }

  return (
    <Card className="p-2">
      <div className="flex items-center justify-between">
        <div>
          <div className="font-medium text-text">{schedule.recipe_name}</div>
          <div className="text-xs text-muted">
            <span className="font-mono">{cadence}</span> · next {next}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <label className="flex items-center gap-1 text-xs text-muted">
            <input
              type="checkbox"
              className="size-4 accent-accent"
              checked={schedule.deliver_notify}
              onChange={(e) => setDelivery({ deliver_notify: e.target.checked })}
            />
            Notify
          </label>
          <label className="flex items-center gap-1 text-xs text-muted">
            <input
              type="checkbox"
              className="size-4 accent-accent"
              checked={schedule.deliver_email}
              onChange={(e) => setDelivery({ deliver_email: e.target.checked })}
            />
            Email
          </label>
          <Button variant="ghost" size="sm" onClick={loadRuns}>
            History
          </Button>
          <Button variant="ghost" size="sm" onClick={toggle}>
            {schedule.enabled ? "Disable" : "Enable"}
          </Button>
          <Button variant="ghost" size="sm" className="text-danger" onClick={remove}>
            Delete
          </Button>
        </div>
      </div>
      {runs && (
        <div className="mt-2 space-y-1 text-xs">
          {runs.length === 0 ? (
            <div className="text-faint">No runs yet.</div>
          ) : (
            runs.map((r, i) => (
              <div key={i} className="flex justify-between">
                <span className={r.status === "ok" ? "text-muted" : "text-danger"}>
                  {r.status}
                </span>
                <span className="text-faint">
                  {new Date(r.started_at).toLocaleString()}
                </span>
              </div>
            ))
          )}
        </div>
      )}
    </Card>
  );
}
