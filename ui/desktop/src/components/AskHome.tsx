import { useEffect, useState } from "react";
import type { DailySnapshotDto, InvestingWorkspaceDto, MastersClient } from "../api/client";
import { GroupChat } from "./GroupChat";
import { Markdown } from "./ui";
import { t } from "../lib/i18n";
import { dailyQuote } from "../lib/quotes";

/** Deterministic daily pick (same all day, rotates tomorrow) — for the cloud quote pack. */
function pickDaily<T>(items: T[], now: Date = new Date()): T {
  const day = Math.floor((now.getTime() - now.getTimezoneOffset() * 60_000) / 86_400_000);
  return items[((day % items.length) + items.length) % items.length];
}

/**
 * 问大师 — the home view (docs/12 §3.1): the standing investing expert-team group chat.
 * Zero-friction ask (D4): the empty state IS the new-topic screen — a daily master quote (from
 * the cloud pack when available, else the local pack), the D13 「本周市场三件事」 heartbeat card
 * when the cloud has a published bulletin, a greeting, three bait questions, and the composer.
 * The topic session is created on first send (titled with the question).
 */
export function AskHome({
  client,
  sessionId,
  draft,
  onSessionCreated,
  onActivity,
}: {
  client: MastersClient;
  /** Resume this topic; absent → the new-topic welcome screen. */
  sessionId: string | null;
  /** Pre-filled question (Watch/Briefings 「就此提问」). */
  draft?: string;
  onSessionCreated: (id: string) => void;
  onActivity: () => void;
}) {
  const [workspace, setWorkspace] = useState<InvestingWorkspaceDto | null>(null);
  const [snapshot, setSnapshot] = useState<DailySnapshotDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [bait, setBait] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    client
      .ensureInvestingWorkspace()
      .then((ws) => !cancelled && setWorkspace(ws))
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [client]);

  // The cloud daily heartbeat is best-effort — a failure just leaves the local quote pack.
  useEffect(() => {
    let cancelled = false;
    client
      .getDailySnapshot()
      .then((s) => !cancelled && setSnapshot(s))
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [client]);

  if (error) {
    return (
      <div className="m-4 rounded border border-danger bg-danger-bg p-3 text-sm text-danger-fg">
        {t("ask.error")}
        {error}
      </div>
    );
  }
  if (!workspace) {
    return <div className="p-6 text-sm text-muted">{t("ask.loading")}</div>;
  }

  // Prefer the cloud quote pack (D13); fall back to the local pack offline.
  const cloudQuotes = snapshot?.quotes ?? [];
  const quote = cloudQuotes.length ? pickDaily(cloudQuotes) : dailyQuote();
  const bulletin = snapshot?.bulletin ?? null;
  const indices = snapshot?.indices ?? [];
  const baitQuestions = [t("watch.empty.q1"), t("watch.empty.q2"), t("watch.empty.q3")];

  const emptyState = (
    <div className="mx-auto flex h-full max-w-xl flex-col items-center justify-center gap-6 py-8 text-center">
      {/* 本周市场三件事 — the D13 weekly heartbeat card (only when the cloud has a bulletin). */}
      {bulletin && (
        <div className="w-full rounded-lg border border-border bg-surface p-4 text-left">
          <div className="mb-2 flex items-baseline justify-between gap-2">
            <span className="text-sm font-medium text-text">{t("ask.weekly")}</span>
            {snapshot?.snapshot_date && (
              <span className="text-xs text-faint">
                {t("watch.dataAsOf")} {snapshot.snapshot_date}
              </span>
            )}
          </div>
          {indices.length > 0 && (
            <div className="mb-3 flex flex-wrap gap-x-4 gap-y-1 text-xs">
              {indices.map((ix) => {
                const up = (ix.change_pct ?? 0) >= 0;
                return (
                  <span key={ix.symbol} className="tabular-nums text-muted">
                    {ix.name} {ix.close != null ? ix.close.toFixed(2) : "—"}{" "}
                    {ix.change_pct != null && (
                      <span className={up ? "text-gain" : "text-loss"}>
                        {up ? "▲" : "▼"}
                        {ix.change_pct.toFixed(2)}%
                      </span>
                    )}
                  </span>
                );
              })}
            </div>
          )}
          <div className="text-sm font-medium">{bulletin.title}</div>
          <div className="mt-1 text-sm text-muted">
            <Markdown text={bulletin.body} />
          </div>
        </div>
      )}
      <blockquote className="text-lg leading-relaxed text-muted">
        「{quote.text}」
        <footer className="mt-2 text-xs text-faint">—— {quote.who}</footer>
      </blockquote>
      <p className="text-base font-medium text-text">{t("ask.greeting")}</p>
      <div className="flex flex-col gap-2">
        {baitQuestions.map((q) => (
          <button
            key={q}
            className="rounded-lg border border-border bg-surface px-4 py-2 text-sm text-muted transition-colors hover:bg-surface-2 hover:text-text"
            onClick={() => setBait(q)}
          >
            {q}
          </button>
        ))}
      </div>
      <p className="text-xs text-faint">{t("ask.coordinatorHint")}</p>
    </div>
  );

  return (
    <GroupChat
      // Remount per topic (and when a bait question seeds the draft) so state stays clean.
      key={`${sessionId ?? "new"}:${bait ?? draft ?? ""}`}
      client={client}
      projectId={workspace.project_id}
      teamSlug={workspace.team_slug}
      sessionId={sessionId ?? undefined}
      initialDraft={bait ?? draft}
      hideHeader
      emptyState={emptyState}
      footer={t("disclaimer.footer")}
      onSessionCreated={onSessionCreated}
      onActivity={onActivity}
    />
  );
}
