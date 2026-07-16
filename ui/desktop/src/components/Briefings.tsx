// 动静 — the briefings timeline (docs/12 §3.3). Day-grouped, type-badged; silent NO_ALERT
// runs never appear (the server hides them), so a quiet week shows the calm positive empty
// state — quiet is the product keeping its promise. 「就此提问」 hands the briefing to 问大师.
import { useCallback, useEffect, useState } from "react";
import { MessageCircleQuestion, Newspaper } from "lucide-react";
import type { BriefingDto, MastersClient } from "../api/client";
import { Badge, Button, Card, Markdown } from "./ui";
import { t } from "../lib/i18n";

function timeOf(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function dayKey(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

function dayLabel(key: string): string {
  const today = dayKey(Date.now());
  const yesterday = dayKey(Date.now() - 86_400_000);
  if (key === today) return t("briefings.today");
  if (key === yesterday) return t("briefings.yesterday");
  return key;
}

/** Recipe slug → briefing type (badge label + tone). Unknown recipes read as plain 动静. */
function typeOf(recipe: string): { label: string; variant: "accent" | "warning" | "tool" | "neutral" } {
  if (recipe.includes("weekly")) return { label: t("briefings.type.weekly"), variant: "accent" };
  if (recipe.includes("mover")) return { label: t("briefings.type.mover"), variant: "warning" };
  if (recipe.includes("earnings")) return { label: t("briefings.type.earnings"), variant: "tool" };
  return { label: t("nav.briefings"), variant: "neutral" };
}

export function Briefings({
  client,
  onAsk,
  onSeen,
}: {
  client: MastersClient;
  /** Open 问大师 with a pre-filled question about a briefing. */
  onAsk: (draft: string) => void;
  /** Report the newest briefing timestamp so the nav dot clears. */
  onSeen: (latest: number) => void;
}) {
  const [briefings, setBriefings] = useState<BriefingDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const ws = await client.ensureInvestingWorkspace();
    const list = await client.listBriefings(ws.project_id);
    setBriefings(list);
    onSeen(list[0]?.started_at ?? Date.now());
  }, [client, onSeen]);

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
  if (briefings === null) {
    return <div className="p-4 text-sm text-muted">{t("briefings.loading")}</div>;
  }

  // Day-grouped timeline (newest first — the list already arrives newest-first).
  const groups: { day: string; items: BriefingDto[] }[] = [];
  for (const b of briefings) {
    const day = dayKey(b.started_at);
    const last = groups[groups.length - 1];
    if (last && last.day === day) last.items.push(b);
    else groups.push({ day, items: [b] });
  }

  return (
    <div className="flex h-full flex-col">
      <div className="border-b border-border px-6 py-4">
        <h1 className="flex items-center gap-2 font-display text-lg font-semibold">
          <Newspaper className="h-5 w-5" />
          {t("briefings.title")}
        </h1>
        <p className="text-sm text-muted">{t("briefings.subtitle")}</p>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-6">
        {briefings.length === 0 ? (
          <div className="mx-auto mt-16 max-w-md text-center">
            <p className="text-base font-medium">{t("briefings.quiet.title")} ✓</p>
            <p className="mt-2 text-sm text-muted">{t("briefings.quiet.hint")}</p>
          </div>
        ) : (
          <div className="mx-auto flex max-w-3xl flex-col gap-6">
            {groups.map(({ day, items }) => (
              <section key={day}>
                <h2 className="mb-2 text-xs font-medium uppercase tracking-wide text-faint">
                  {dayLabel(day)}
                </h2>
                <div className="flex flex-col gap-3">
                  {items.map((b) => {
                    const type = typeOf(b.recipe_name);
                    return (
                      <Card key={`${b.recipe_name}:${b.started_at}`} className="p-4">
                        <div className="mb-2 flex items-baseline gap-2">
                          <Badge variant={type.variant}>{type.label}</Badge>
                          <span className="font-medium">{b.title}</span>
                          <span className="ml-auto text-xs text-faint tabular-nums">
                            {timeOf(b.started_at)}
                          </span>
                        </div>
                        <div className="text-sm leading-relaxed">
                          <Markdown text={b.body} />
                        </div>
                        <div className="mt-3 flex justify-end">
                          <Button
                            variant="secondary"
                            size="sm"
                            onClick={() =>
                              onAsk(`关于这份「${b.title}」（${type.label}）：`)
                            }
                          >
                            <MessageCircleQuestion className="mr-1 h-4 w-4" />
                            {t("briefings.ask")}
                          </Button>
                        </div>
                      </Card>
                    );
                  })}
                </div>
              </section>
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
