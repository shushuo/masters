import { useEffect, useState } from "react";
import type { InvestingWorkspaceDto, MastersClient } from "../api/client";
import { GroupChat } from "./GroupChat";
import { t } from "../lib/i18n";
import { dailyQuote } from "../lib/quotes";

/**
 * 问大师 — the home view (docs/12 §3.1): the standing investing expert-team group chat.
 * Zero-friction ask (D4): the empty state IS the new-topic screen — a daily master quote,
 * a greeting, three bait questions, and the composer. The topic session is created on the
 * first send (titled with the question) and the URL syncs to `#/ask/:sessionId`.
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

  const quote = dailyQuote();
  const baitQuestions = [t("watch.empty.q1"), t("watch.empty.q2"), t("watch.empty.q3")];

  const emptyState = (
    <div className="mx-auto flex h-full max-w-xl flex-col items-center justify-center gap-6 text-center">
      <blockquote className="font-display text-lg leading-relaxed text-muted">
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
