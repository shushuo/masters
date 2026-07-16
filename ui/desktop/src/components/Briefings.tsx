// The briefings feed (docs/11 M8, slice 2) — the reading surface for proactive-touch outputs
// (weekly watch digest, mover alerts). Silent NO_ALERT runs never appear here (the server hides
// them); each card can hand its content to the expert-team chat via 就此提问.
import { useCallback, useEffect, useState } from "react";
import { MessageCircleQuestion, Newspaper } from "lucide-react";
import type { BriefingDto, InvestingWorkspaceDto, MastersClient } from "../api/client";
import { GroupChat } from "./GroupChat";
import { Button, Card, Markdown } from "./ui";
import { t } from "../lib/i18n";

function dateTimeOf(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(
    d.getMinutes(),
  )}`;
}

export function Briefings({ client }: { client: MastersClient }) {
  const [workspace, setWorkspace] = useState<InvestingWorkspaceDto | null>(null);
  const [briefings, setBriefings] = useState<BriefingDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** When set, the embedded expert chat is open, pre-seeded with this briefing's context. */
  const [askAbout, setAskAbout] = useState<BriefingDto | null>(null);

  const refresh = useCallback(async () => {
    const ws = await client.ensureInvestingWorkspace();
    setWorkspace(ws);
    setBriefings(await client.listBriefings(ws.project_id));
  }, [client]);

  useEffect(() => {
    refresh().catch((e) => setError(String(e)));
  }, [refresh]);

  if (error) {
    return (
      <div className="p-4 text-sm text-danger">
        {t("briefings.error")}
        {error}
      </div>
    );
  }
  if (!workspace || briefings === null) {
    return <div className="p-4 text-sm text-muted">{t("briefings.loading")}</div>;
  }

  if (askAbout) {
    return (
      <div className="flex h-full flex-col">
        <div className="min-h-0 flex-1">
          <GroupChat
            client={client}
            projectId={workspace.project_id}
            teamSlug={workspace.team_slug}
            title={`${t("watch.teamTitle")} · ${askAbout.title}`}
            backLabel={t("watch.backToList")}
            onClose={() => setAskAbout(null)}
          />
        </div>
        <p className="border-t border-border px-4 py-2 text-center text-xs text-muted">
          {t("disclaimer.footer")}
        </p>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <div className="border-b border-border px-6 py-4">
        <h1 className="flex items-center gap-2 text-lg font-semibold">
          <Newspaper className="h-5 w-5" />
          {t("briefings.title")}
        </h1>
        <p className="text-sm text-muted">{t("briefings.subtitle")}</p>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-6">
        {briefings.length === 0 ? (
          <div className="mx-auto mt-16 max-w-md text-center">
            <p className="text-base font-medium">{t("briefings.empty.title")}</p>
            <p className="mt-2 text-sm text-muted">{t("briefings.empty.hint")}</p>
          </div>
        ) : (
          <div className="mx-auto flex max-w-3xl flex-col gap-4">
            {briefings.map((b) => (
              <Card key={`${b.recipe_name}:${b.started_at}`} className="p-4">
                <div className="mb-2 flex items-baseline justify-between gap-3">
                  <span className="font-medium">{b.title}</span>
                  <span className="text-xs text-muted tabular-nums">
                    {dateTimeOf(b.started_at)}
                  </span>
                </div>
                <div className="text-sm">
                  <Markdown text={b.body} />
                </div>
                <div className="mt-3 flex justify-end">
                  <Button variant="secondary" onClick={() => setAskAbout(b)}>
                    <MessageCircleQuestion className="mr-1 h-4 w-4" />
                    {t("briefings.ask")}
                  </Button>
                </div>
              </Card>
            ))}
          </div>
        )}
      </div>

      <p className="border-t border-border px-4 py-2 text-center text-xs text-muted">
        {t("disclaimer.footer")}
      </p>
    </div>
  );
}
