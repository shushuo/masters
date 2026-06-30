import { useEffect, useRef, useState } from "react";
import { ArrowLeft, SendHorizontal } from "lucide-react";
import { MastersClient } from "../api/client";
import { Button, Input, Select } from "./ui";

interface Bubble {
  key: string;
  round: number;
  author: string;
  content: string;
  tools: string[];
}

/**
 * Multi-master group chat (Phase 4c/4e/4f, FR-43; ADR-0012). Start a chat for a team, then post:
 * `@master` addresses one, `@all` everyone, no mention → the coordinator. Each addressed master answers
 * from the shared transcript on its own persona + model; replies **stream live**, attributed, over the
 * session WebSocket (Phase 4e). A master reply that `@mentions` another master drives a **bounded
 * follow-up round** (Phase 4f); each round's bubbles are grouped under a round divider.
 *
 * Driven either by a saved team (`projectId` + `teamSlug`, the Teams tab) or, for the Masters sidebar's
 * **quick chat**, by an explicit `openSession` thunk + a `members`/`coordinator` roster — the same
 * streaming UI serves both an ad-hoc master set and a persisted team.
 */
export function GroupChat({
  client,
  projectId,
  teamSlug,
  title,
  backLabel = "Back",
  members: membersProp,
  coordinator: coordinatorProp,
  openSession,
  onClose,
}: {
  client: MastersClient;
  projectId?: string;
  teamSlug?: string;
  title?: string;
  backLabel?: string;
  members?: string[];
  coordinator?: string;
  /** Create (or return) the bound group session; defaults to starting a session for the saved team. */
  openSession?: () => Promise<{ id: string }>;
  onClose: () => void;
}) {
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [bubbles, setBubbles] = useState<Bubble[]>([]);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [members, setMembers] = useState<string[]>(membersProp ?? []);
  const [coordinator, setCoordinator] = useState<string>(coordinatorProp ?? "");
  // Optional per-call cap on mention-driven follow-up rounds (Phase 4f); undefined → server default.
  const [maxRounds, setMaxRounds] = useState<number | undefined>(undefined);
  // The active turn's id (so its rounds' bubble keys are unique across sends).
  const turnRef = useRef<string>("");

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

  function mention(slug: string) {
    setDraft((d) => (d.endsWith(" ") || d === "" ? d : d + " ") + `@${slug} `);
  }

  async function ensureSession(): Promise<string> {
    if (sessionId) return sessionId;
    const s = openSession
      ? await openSession()
      : await client.startTeamSession(projectId!, teamSlug!);
    setSessionId(s.id);
    return s.id;
  }

  function bubbleKey(round: number, author: string): string {
    return `${turnRef.current}:${round}:${author}`;
  }

  function appendDelta(round: number, author: string, text: string) {
    const key = bubbleKey(round, author);
    setBubbles((b) => b.map((x) => (x.key === key ? { ...x, content: x.content + text } : x)));
  }

  function appendTool(round: number, author: string, line: string) {
    const key = bubbleKey(round, author);
    setBubbles((b) => b.map((x) => (x.key === key ? { ...x, tools: [...x.tools, line] } : x)));
  }

  async function send() {
    const content = draft.trim();
    if (!content || busy) return;
    setBusy(true);
    setError(null);
    try {
      const sid = await ensureSession();
      const turn = `${Date.now()}`;
      turnRef.current = turn;
      setBubbles((b) => [
        ...b,
        { key: `${turn}:user`, round: -1, author: "user", content, tools: [] },
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
            tools: [],
          }));
          setBubbles((b) => [...b, ...placeholders]);
        },
        onMasterDelta: appendDelta,
        onMasterToolCall: (round, author, tool) => appendTool(round, author, `→ ${tool}`),
        onMasterToolResult: (round, author, summary, isError) =>
          appendTool(round, author, `${isError ? "⚠️" : "←"} ${summary}`),
        onMasterError: (round, author, message) => appendDelta(round, author, `\n⚠️ ${message}`),
        onGroupComplete: () => setBusy(false),
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
      <div className="flex items-center gap-2 border-b border-border px-3 py-2 text-sm">
        <Button variant="ghost" size="sm" onClick={onClose}>
          <ArrowLeft className="size-4" /> {backLabel}
        </Button>
        <span className="font-medium text-text">{title ?? `Group chat · ${teamSlug}`}</span>
      </div>
      {error && <div className="px-3 py-1 text-xs text-danger">{error}</div>}
      <div className="flex-1 space-y-2 overflow-y-auto p-3 text-sm">
        {bubbles.length === 0 && (
          <div className="text-muted">
            Start with <code>@master</code>, <code>@all</code>, or no mention (the coordinator answers).
          </div>
        )}
        {bubbles.map((m, i) => {
          // A round divider when a new follow-up round (round ≥ 1) begins.
          const prev = bubbles[i - 1];
          const showDivider = m.round >= 1 && (!prev || prev.round !== m.round);
          return (
            <div key={m.key}>
              {showDivider && (
                <div className="my-1 text-center text-[10px] uppercase tracking-wide text-faint">
                  round {m.round + 1}
                </div>
              )}
              <div className={m.author === "user" ? "text-right" : ""}>
                <div className="text-xs text-faint">{m.author}</div>
                {m.tools.map((line, j) => (
                  <div key={j} className="font-mono text-[11px] text-faint">
                    {line}
                  </div>
                ))}
                <div
                  className={`inline-block whitespace-pre-wrap rounded px-2 py-1 ${
                    m.author === "user" ? "bg-accent text-accent-fg" : "bg-surface-2 text-text"
                  }`}
                >
                  {m.content || "…"}
                </div>
              </div>
            </div>
          );
        })}
      </div>
      {members.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5 border-t border-border px-2 pt-2 text-xs">
          <button
            className="rounded-sm bg-surface-2 px-1.5 py-0.5 text-muted hover:text-text"
            onClick={() => mention("all")}
          >
            @all
          </button>
          {members.map((slug) => (
            <button
              key={slug}
              className="rounded-sm bg-surface-2 px-1.5 py-0.5 text-muted hover:text-text"
              title={slug === coordinator ? "coordinator (answers unaddressed messages)" : undefined}
              onClick={() => mention(slug)}
            >
              @{slug}
              {slug === coordinator && <span className="ml-1 text-accent">★</span>}
            </button>
          ))}
        </div>
      )}
      <div className="flex gap-2 border-t border-border p-2">
        <Input
          className="flex-1"
          placeholder="Message the team… (@master / @all)"
          value={draft}
          disabled={busy}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && send()}
        />
        <Select
          className="w-auto"
          title="Max follow-up rounds"
          value={maxRounds?.toString() ?? ""}
          disabled={busy}
          onChange={(e) => setMaxRounds(e.target.value ? Number(e.target.value) : undefined)}
        >
          <option value="">Rounds: auto</option>
          {[1, 2, 3, 4, 5].map((n) => (
            <option key={n} value={n}>
              {n} round{n === 1 ? "" : "s"}
            </option>
          ))}
        </Select>
        <Button variant="primary" disabled={busy || !draft.trim()} onClick={send}>
          <SendHorizontal className="size-4" /> {busy ? "…" : "Send"}
        </Button>
      </div>
    </div>
  );
}
