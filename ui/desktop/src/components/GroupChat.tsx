import { useEffect, useRef, useState, type ReactNode } from "react";
import { ArrowLeft, SendHorizontal } from "lucide-react";
import { MastersClient } from "../api/client";
import { Button, Composer, Markdown, Select, ToolStep, type ToolStepData } from "./ui";
import { cn } from "./ui/cn";
import { getLocale, t } from "../lib/i18n";
import { masterColor, masterGlyph, masterName } from "../lib/masters";

interface Bubble {
  key: string;
  round: number;
  author: string;
  content: string;
  steps: ToolStepData[];
}

/** The roster identity chip: colored first-glyph avatar + display name (docs/12 §5.4). */
function MasterFace({ slug, size = "sm" }: { slug: string; size?: "sm" | "md" }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span
        aria-hidden
        className={cn(
          "inline-flex items-center justify-center rounded-full font-medium text-white",
          size === "md" ? "size-5 text-[11px]" : "size-4 text-[10px]",
        )}
        style={{ backgroundColor: masterColor(slug) }}
      >
        {masterGlyph(slug)}
      </span>
      <span className="text-xs font-medium text-muted">{masterName(slug, getLocale())}</span>
    </span>
  );
}

/**
 * Multi-master group chat (Phase 4c/4e/4f, FR-43; ADR-0012) — the conversation surface behind
 * 问大师 (docs/12 §3.1), the Teams tab, and quick chat. `@master` addresses one, `@all` everyone,
 * no mention → the coordinator. Replies stream live and attributed; mention-driven follow-up
 * rounds are bounded.
 *
 * Session handling: pass `sessionId` to RESUME an existing topic (history replays via
 * `listMessages`, attributed by `author`); otherwise a session is created lazily on first send —
 * via the `openSession` thunk (given the trimmed first question as a suggested topic title) or
 * `startTeamSession`. `onSessionCreated` lets the host sync the URL.
 */
export function GroupChat({
  client,
  projectId,
  teamSlug,
  title,
  backLabel = "Back",
  members: membersProp,
  coordinator: coordinatorProp,
  sessionId: sessionIdProp,
  initialDraft,
  emptyState,
  footer,
  hideHeader,
  openSession,
  onSessionCreated,
  onActivity,
  onClose,
}: {
  client: MastersClient;
  projectId?: string;
  teamSlug?: string;
  title?: string;
  backLabel?: string;
  members?: string[];
  coordinator?: string;
  /** Resume this existing group session (history is replayed on mount). */
  sessionId?: string;
  /** Pre-filled composer text (e.g. Watch/Briefings 「就此提问」). */
  initialDraft?: string;
  /** Rendered when the transcript is empty (the 问大师 welcome screen). */
  emptyState?: ReactNode;
  /** A fixed line above the composer (the compliance footer on investing surfaces). */
  footer?: ReactNode;
  /** Hide the back-button header (full-page hosts render their own chrome). */
  hideHeader?: boolean;
  /** Create (or return) the bound group session; defaults to starting a session for the saved team. */
  openSession?: (topicTitle?: string) => Promise<{ id: string }>;
  onSessionCreated?: (id: string) => void;
  /** Fired when a turn completes (hosts refresh topic lists). */
  onActivity?: () => void;
  onClose?: () => void;
}) {
  const [sessionId, setSessionId] = useState<string | null>(sessionIdProp ?? null);
  const [bubbles, setBubbles] = useState<Bubble[]>([]);
  const [draft, setDraft] = useState(initialDraft ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [members, setMembers] = useState<string[]>(membersProp ?? []);
  const [coordinator, setCoordinator] = useState<string>(coordinatorProp ?? "");
  // Optional per-call cap on mention-driven follow-up rounds (Phase 4f); undefined → server default.
  const [maxRounds, setMaxRounds] = useState<number | undefined>(undefined);
  // The active turn's id (so its rounds' bubble keys are unique across sends).
  const turnRef = useRef<string>("");
  const scrollRef = useRef<HTMLDivElement | null>(null);

  // Load the team's roster so the user can address members by clicking a chip. Skipped when the
  // caller supplied the roster directly (quick chat over an ad-hoc set of masters).
  useEffect(() => {
    if (membersProp || !projectId || !teamSlug) return;
    client
      .getTeam(projectId, teamSlug)
      .then((t) => {
        setMembers(t.members ?? []);
        setCoordinator(t.coordinator_slug ?? "");
      })
      .catch(() => {});
  }, [client, projectId, teamSlug, membersProp]);

  // Resume: replay the topic's persisted transcript, attributed by author (docs/12 §3.1).
  useEffect(() => {
    if (!sessionIdProp) return;
    let cancelled = false;
    client
      .listMessages(sessionIdProp)
      .then((msgs) => {
        if (cancelled) return;
        setBubbles(
          msgs
            .filter((m) => m.role !== "tool" && m.content.trim() !== "")
            .map((m) => ({
              key: `hist:${m.id}`,
              round: m.author === "user" ? -1 : 0,
              author: m.author || (m.role === "user" ? "user" : "assistant"),
              content: m.content,
              steps: [],
            })),
        );
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [client, sessionIdProp]);

  // Keep the log pinned to the latest turn.
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [bubbles]);

  function mention(slug: string) {
    setDraft((d) => (d.endsWith(" ") || d === "" ? d : d + " ") + `@${slug} `);
  }

  async function ensureSession(topicTitle?: string): Promise<string> {
    if (sessionId) return sessionId;
    const s = openSession
      ? await openSession(topicTitle)
      : await client.startTeamSession(projectId!, teamSlug!, topicTitle);
    setSessionId(s.id);
    onSessionCreated?.(s.id);
    return s.id;
  }

  function bubbleKey(round: number, author: string): string {
    return `${turnRef.current}:${round}:${author}`;
  }

  function appendDelta(round: number, author: string, text: string) {
    const key = bubbleKey(round, author);
    setBubbles((b) => b.map((x) => (x.key === key ? { ...x, content: x.content + text } : x)));
  }

  function addToolCall(round: number, author: string, id: string, tool: string, summary: string) {
    const key = bubbleKey(round, author);
    setBubbles((b) =>
      b.map((x) =>
        x.key === key ? { ...x, steps: [...x.steps, { id, tool, callSummary: summary }] } : x,
      ),
    );
  }

  function addToolResult(
    round: number,
    author: string,
    id: string,
    summary: string,
    isError: boolean,
  ) {
    const key = bubbleKey(round, author);
    setBubbles((b) =>
      b.map((x) =>
        x.key === key
          ? {
              ...x,
              steps: x.steps.map((s) => (s.id === id ? { ...s, result: { summary, isError } } : s)),
            }
          : x,
      ),
    );
  }

  async function send() {
    const content = draft.trim();
    if (!content || busy) return;
    setBusy(true);
    setError(null);
    try {
      // First question doubles as the topic title (truncated), so the list is recognizable.
      const topicTitle = content.replace(/\s+/g, " ").slice(0, 40);
      const sid = await ensureSession(topicTitle);
      const turn = `${Date.now()}`;
      turnRef.current = turn;
      setBubbles((b) => [
        ...b,
        { key: `${turn}:user`, round: -1, author: "user", content, steps: [] },
      ]);
      setDraft("");

      client.openStream(
        sid,
        content,
        {
          onDelta: () => {}, // group sessions don't emit plain assistant deltas
          onComplete: () => {},
          // Seed an empty bubble per addressed master for this round; deltas fill them in.
          onGroupStart: (round, addressed) => {
            const placeholders = addressed.map((author) => ({
              key: bubbleKey(round, author),
              round,
              author,
              content: "",
              steps: [] as ToolStepData[],
            }));
            setBubbles((b) => [...b, ...placeholders]);
          },
          onMasterDelta: appendDelta,
          onMasterToolCall: addToolCall,
          onMasterToolResult: addToolResult,
          onMasterError: (round, author, message) => appendDelta(round, author, `\n⚠️ ${message}`),
          onGroupComplete: () => {
            setBusy(false);
            onActivity?.();
          },
          onError: (message) => {
            setError(message);
            setBusy(false);
          },
        },
        maxRounds,
      );
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  return (
    <div className="flex h-full flex-col">
      {!hideHeader && (
        <div className="flex items-center gap-2 border-b border-border px-3 py-2 text-sm">
          {onClose && (
            <Button variant="ghost" size="sm" onClick={onClose}>
              <ArrowLeft className="size-4" /> {backLabel}
            </Button>
          )}
          <span className="font-medium text-text">{title ?? `Group chat · ${teamSlug}`}</span>
        </div>
      )}
      {error && (
        <div className="px-4 py-1 text-xs text-danger">
          {t("ask.error")}
          {error}
        </div>
      )}
      <div
        ref={scrollRef}
        className="flex-1 space-y-3 overflow-y-auto px-4 py-4 text-sm"
        role="log"
        aria-live="polite"
      >
        {bubbles.length === 0 &&
          (emptyState ?? (
            <div className="text-muted">
              Start with <code>@master</code>, <code>@all</code>, or no mention (the coordinator
              answers).
            </div>
          ))}
        {bubbles.map((m, i) => {
          // A round divider when a new follow-up round (round ≥ 1) begins.
          const prev = bubbles[i - 1];
          const showDivider = m.round >= 1 && (!prev || prev.round !== m.round);
          const isUser = m.author === "user";
          return (
            <div key={m.key} className="mx-auto w-full max-w-3xl">
              {showDivider && (
                <div className="my-1 text-center text-[10px] uppercase tracking-wide text-faint">
                  round {m.round + 1}
                </div>
              )}
              <div className={isUser ? "text-right" : ""}>
                {!isUser && (
                  <div className="mb-1">
                    <MasterFace slug={m.author} />
                  </div>
                )}
                {m.steps.length > 0 && (
                  <div className="my-1 space-y-1">
                    {m.steps.map((s) => (
                      <ToolStep key={s.id} step={s} />
                    ))}
                  </div>
                )}
                <div
                  className={cn(
                    "inline-block max-w-full rounded-lg px-3 py-2 text-left leading-relaxed",
                    isUser
                      ? "whitespace-pre-wrap bg-accent text-accent-fg"
                      : "bg-surface-2 text-text",
                  )}
                >
                  {isUser ? m.content || "…" : m.content ? <Markdown text={m.content} /> : "…"}
                </div>
              </div>
            </div>
          );
        })}
      </div>
      {footer && (
        <div className="mx-auto w-full max-w-3xl px-4 pb-1 text-center text-[11px] text-faint">
          {footer}
        </div>
      )}
      <div className="mx-auto w-full max-w-3xl">
        <Composer
          value={draft}
          onChange={setDraft}
          onSubmit={send}
          disabled={busy}
          placeholder={t("ask.placeholder")}
          leading={
            members.length > 0 ? (
              <div className="mb-2 flex flex-wrap items-center gap-1.5 text-xs">
                <button
                  className="rounded-full bg-surface-2 px-2 py-0.5 text-muted hover:text-text"
                  onClick={() => mention("all")}
                >
                  @all
                </button>
                {members.map((slug) => (
                  <button
                    key={slug}
                    className="inline-flex items-center gap-1 rounded-full bg-surface-2 px-2 py-0.5 hover:bg-surface"
                    title={slug === coordinator ? t("ask.coordinatorHint") : `@${slug}`}
                    onClick={() => mention(slug)}
                  >
                    <span
                      aria-hidden
                      className="inline-flex size-3.5 items-center justify-center rounded-full text-[9px] font-medium text-white"
                      style={{ backgroundColor: masterColor(slug) }}
                    >
                      {masterGlyph(slug)}
                    </span>
                    <span className="text-muted">{masterName(slug, getLocale())}</span>
                    {slug === coordinator && <span className="text-accent">★</span>}
                  </button>
                ))}
              </div>
            ) : undefined
          }
          trailing={
            <>
              <Select
                className="w-auto"
                title="Max follow-up rounds"
                value={maxRounds?.toString() ?? ""}
                disabled={busy}
                onChange={(e) => setMaxRounds(e.target.value ? Number(e.target.value) : undefined)}
              >
                <option value="">{t("ask.rounds.auto")}</option>
                {[1, 2, 3, 4, 5].map((n) => (
                  <option key={n} value={n}>
                    {n} {t("ask.rounds.n")}
                  </option>
                ))}
              </Select>
              <Button
                variant="primary"
                className="whitespace-nowrap"
                disabled={busy || !draft.trim()}
                onClick={send}
              >
                <SendHorizontal className="size-4" /> {busy ? "…" : t("ask.send")}
              </Button>
            </>
          }
        />
      </div>
    </div>
  );
}
