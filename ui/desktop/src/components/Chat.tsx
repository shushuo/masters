import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  ArrowDown,
  Check,
  PanelRight,
  SendHorizontal,
  ShieldCheck,
  Square,
  Undo2,
  X,
} from "lucide-react";
import {
  MastersClient,
  type AuditEntryDto,
  type FilePreview,
  type PendingApproval,
} from "../api/client";
import {
  Badge,
  Button,
  Composer,
  IconButton,
  Markdown,
  PandaMark,
  ToolStep,
  type ToolStepData,
} from "./ui";
import { useStickToBottom } from "../lib/useStickToBottom";

const DECISION_VARIANT: Record<string, "neutral" | "success" | "danger"> = {
  auto: "neutral",
  approved: "success",
  denied: "danger",
};

type Turn =
  | { kind: "user"; content: string }
  | { kind: "assistant"; content: string }
  | { kind: "tool"; step: ToolStepData };

const MAX_DIFF_LINES = 40;

/** A compact before/after diff for a proposed file write, shown in the approval bar. */
function DiffPreview({ preview }: { preview: FilePreview }) {
  if (preview.omitted) {
    return (
      <div className="mb-2 rounded border border-border bg-bg px-2 py-1 text-xs text-muted">
        Binary or large file — preview unavailable.
      </div>
    );
  }
  const before = preview.before ? preview.before.split("\n") : [];
  const after = preview.after ? preview.after.split("\n") : [];
  const beforeSet = new Set(before);
  const afterSet = new Set(after);
  const removed = before.filter((l) => !afterSet.has(l));
  const added = after.filter((l) => !beforeSet.has(l));
  const rows = [
    ...removed.map((text) => ({ sign: "-" as const, text })),
    ...added.map((text) => ({ sign: "+" as const, text })),
  ];
  const shown = rows.slice(0, MAX_DIFF_LINES);

  return (
    <div className="mb-2 overflow-hidden rounded border border-border bg-bg text-xs">
      <div className="flex items-center gap-2 border-b border-border px-2 py-1">
        <span className="truncate font-mono text-muted">{preview.path}</span>
        <span className="ml-auto shrink-0 font-mono">
          <span className="text-success">+{preview.added}</span>{" "}
          <span className="text-danger">−{preview.removed}</span>
        </span>
      </div>
      <pre className="max-h-48 overflow-auto p-2 font-mono text-[11px] leading-snug">
        {shown.map((r, i) => (
          <div
            key={i}
            className={
              r.sign === "+"
                ? "bg-success/10 text-success"
                : "bg-danger/10 text-danger"
            }
          >
            {r.sign} {r.text}
          </div>
        ))}
        {rows.length > MAX_DIFF_LINES && (
          <div className="text-faint">… {rows.length - MAX_DIFF_LINES} more changed lines</div>
        )}
      </pre>
    </div>
  );
}

/**
 * The single-agent chat pane. Controlled by App: the active `sessionId` and `streaming`
 * state live upstream (so the Sidebar's session list stays in sync and can block
 * switching mid-run). Chat owns only the in-flight transcript + approval/audit UI.
 */
export function Chat({
  client,
  sessionId,
  streaming,
  onStreamingChange,
  onActivity,
}: {
  client: MastersClient;
  sessionId: string | null;
  streaming: boolean;
  onStreamingChange: (streaming: boolean) => void;
  onActivity: () => void;
}) {
  const [turns, setTurns] = useState<Turn[]>([]);
  const [input, setInput] = useState("");
  const [approval, setApproval] = useState<PendingApproval | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [showPanel, setShowPanel] = useState(false);
  const [audit, setAudit] = useState<AuditEntryDto[]>([]);
  const wsRef = useRef<WebSocket | null>(null);
  const approvalRef = useRef<HTMLDivElement>(null);
  const prevFocusRef = useRef<HTMLElement | null>(null);
  const { ref: scrollRef, atBottom, scrollToBottom } = useStickToBottom([turns]);

  async function refreshAudit(id = sessionId) {
    if (!id) return;
    try {
      setAudit(await client.listAudit(id));
    } catch {
      /* non-fatal: the panel just shows no rows */
    }
  }

  // Load the active session's history whenever it changes (App owns which session is active).
  useEffect(() => {
    if (!sessionId) {
      setTurns([]);
      return;
    }
    let cancelled = false;
    setNotice(null);
    client
      .listMessages(sessionId)
      .then((msgs) => {
        if (cancelled) return;
        setTurns(
          msgs.map((m): Turn =>
            m.role === "tool"
              ? { kind: "tool", step: { id: m.id, tool: "tool", callSummary: m.content } }
              : m.role === "assistant"
                ? { kind: "assistant", content: m.content }
                : { kind: "user", content: m.content },
          ),
        );
      })
      .catch(() => {
        if (!cancelled) setTurns([]);
      });
    return () => {
      cancelled = true;
    };
  }, [client, sessionId]);

  // Keep the audit trail in sync with the active session whenever the panel is open.
  useEffect(() => {
    if (showPanel) refreshAudit();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [showPanel, sessionId]);

  // Esc stops an in-flight stream.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape" && streaming) stop();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [streaming]);

  // The approval prompt is the security-critical control: focus it when it appears,
  // restore focus when it resolves, and keep Tab within it while a decision is pending.
  useEffect(() => {
    if (approval) {
      prevFocusRef.current = document.activeElement as HTMLElement | null;
      approvalRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    } else {
      prevFocusRef.current?.focus?.();
    }
  }, [approval]);

  function trapApprovalTab(e: ReactKeyboardEvent<HTMLDivElement>) {
    if (e.key !== "Tab") return;
    const btns = approvalRef.current?.querySelectorAll<HTMLButtonElement>("button");
    if (!btns || btns.length === 0) return;
    const first = btns[0];
    const last = btns[btns.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  function appendAssistant(text: string) {
    setTurns((t) => {
      const next = [...t];
      const last = next[next.length - 1];
      if (last && last.kind === "assistant") {
        next[next.length - 1] = { kind: "assistant", content: last.content + text };
      } else {
        next.push({ kind: "assistant", content: text });
      }
      return next;
    });
  }

  function addToolCall(id: string, tool: string, summary: string) {
    setTurns((t) => [...t, { kind: "tool", step: { id, tool, callSummary: summary } }]);
  }

  function addToolResult(id: string, summary: string, isError: boolean) {
    setTurns((t) =>
      t.map((turn) =>
        turn.kind === "tool" && turn.step.id === id
          ? { kind: "tool", step: { ...turn.step, result: { summary, isError } } }
          : turn,
      ),
    );
  }

  function send() {
    if (!sessionId || !input.trim() || streaming) return;
    const content = input.trim();
    setInput("");
    setTurns((t) => [...t, { kind: "user", content }, { kind: "assistant", content: "" }]);
    onStreamingChange(true);

    wsRef.current = client.openStream(sessionId, content, {
      onDelta: appendAssistant,
      onToolCall: addToolCall,
      onToolResult: addToolResult,
      onApproval: (a) => setApproval(a),
      onComplete: () => {
        onStreamingChange(false);
        onActivity();
        refreshAudit();
      },
      onError: (message) => {
        onStreamingChange(false);
        appendAssistant(`\n\n⚠️ ${message}`);
      },
    });
  }

  function decide(decision: "allow" | "always_tool" | "deny") {
    if (approval && wsRef.current) {
      MastersClient.sendApproval(wsRef.current, approval.requestId, decision);
    }
    setApproval(null);
  }

  function stop() {
    wsRef.current?.send(JSON.stringify({ type: "stop" }));
    wsRef.current?.close();
    onStreamingChange(false);
  }

  async function revert() {
    if (!sessionId) return;
    try {
      const r = await client.revert(sessionId);
      setNotice(r.summary);
    } catch (e) {
      setNotice(`Revert failed: ${String(e)}`);
    }
  }

  return (
    <div className="flex h-full min-h-0">
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex items-center gap-2 border-b border-border px-3 py-2">
          <div className="flex-1" />
          <IconButton
            label={showPanel ? "Hide details" : "Show details"}
            onClick={() => setShowPanel((v) => !v)}
          >
            <PanelRight className="size-4" />
          </IconButton>
        </div>

        <div className="relative min-h-0 flex-1">
          <div
            ref={scrollRef}
            className="h-full space-y-3 overflow-y-auto p-6"
            role="log"
            aria-live="polite"
          >
            {turns.length === 0 && (
              <div className="mx-auto mt-16 flex max-w-sm flex-col items-center text-center">
                <PandaMark className="size-16 opacity-90" />
                <h2 className="mt-4 text-xl font-semibold text-text">What can Masters do for you?</h2>
                <p className="mt-2 text-sm text-muted">
                  Ask Masters to work on your granted files — every tool call is gated and audited.
                </p>
              </div>
            )}
            {turns.map((turn, i) =>
              turn.kind === "tool" ? (
                <ToolStep key={turn.step.id || i} step={turn.step} />
              ) : turn.kind === "user" ? (
                <div key={i} className="text-right">
                  <span className="inline-block max-w-[80%] whitespace-pre-wrap rounded-lg rounded-br-sm bg-accent px-3 py-2 text-sm text-accent-fg">
                    {turn.content}
                  </span>
                </div>
              ) : (
                <div key={i} className="text-left">
                  <div className="inline-block max-w-[80%] rounded-lg rounded-bl-sm bg-surface-2 px-3 py-2 text-text">
                    {turn.content ? (
                      <Markdown text={turn.content} />
                    ) : (
                      <span className="text-sm text-muted">…</span>
                    )}
                  </div>
                </div>
              ),
            )}
          </div>
          {!atBottom && (
            <button
              onClick={() => scrollToBottom("smooth")}
              className="absolute bottom-3 left-1/2 flex -translate-x-1/2 items-center gap-1 rounded-full border border-border bg-bg px-3 py-1 text-xs text-muted shadow hover:text-text"
            >
              <ArrowDown className="size-3.5" /> Jump to latest
            </button>
          )}
        </div>

        {approval && (
          <div
            ref={approvalRef}
            onKeyDown={trapApprovalTab}
            role="alertdialog"
            aria-label="Tool approval required"
            className="border-t border-tool-border bg-tool-bg p-3 text-sm"
          >
            <p className="mb-2 text-text">
              Approve <Badge variant="tool">{approval.tool}</Badge> — {approval.summary}{" "}
              <span className="text-muted">[{approval.classes.join(", ")}]</span>?
            </p>
            {approval.preview && <DiffPreview preview={approval.preview} />}
            <div className="flex gap-2">
              <Button variant="primary" size="sm" onClick={() => decide("allow")}>
                <Check className="size-3.5" /> Allow once
              </Button>
              <Button variant="secondary" size="sm" onClick={() => decide("always_tool")}>
                <ShieldCheck className="size-3.5" /> Always allow this tool
              </Button>
              <Button variant="danger" size="sm" onClick={() => decide("deny")}>
                <X className="size-3.5" /> Deny
              </Button>
            </div>
          </div>
        )}

        {notice && (
          <div className="border-t border-border bg-surface px-4 py-2 text-xs text-muted">{notice}</div>
        )}

        <Composer
          value={input}
          onChange={setInput}
          onSubmit={send}
          disabled={!sessionId}
          placeholder={sessionId ? "Message Masters…  (Shift+Enter for a new line)" : "Connecting…"}
          trailing={
            <>
              <Button variant="ghost" onClick={revert} title="Undo the last file change">
                <Undo2 className="size-4" /> Revert
              </Button>
              {streaming ? (
                <Button variant="secondary" onClick={stop}>
                  <Square className="size-4" /> Stop
                </Button>
              ) : (
                <Button variant="primary" onClick={send} disabled={!sessionId || !input.trim()}>
                  <SendHorizontal className="size-4" /> Send
                </Button>
              )}
            </>
          }
        />
      </div>

      {/* Right-hand context panel: session detail + this turn's gated tool activity. */}
      {showPanel && (
        <aside className="flex w-72 shrink-0 flex-col gap-3 overflow-y-auto border-l border-border bg-surface p-4 text-sm">
          <div>
            <h3 className="text-xs font-semibold uppercase tracking-wide text-muted">Session</h3>
            <div className="mt-1 break-all font-mono text-xs text-faint">{sessionId ?? "—"}</div>
          </div>
          <div>
            <div className="flex items-center justify-between">
              <h3 className="text-xs font-semibold uppercase tracking-wide text-muted">Audit trail</h3>
              <span className="text-xs text-faint">{audit.length}</span>
            </div>
            {audit.length === 0 ? (
              <p className="mt-1 text-muted">No gated tool calls in this session yet.</p>
            ) : (
              <ul className="mt-2 space-y-1.5">
                {audit.map((e) => (
                  <li key={e.id} className="rounded border border-border bg-bg p-2">
                    <div className="flex items-center justify-between gap-2">
                      <span className="truncate font-mono text-xs text-text">{e.tool}</span>
                      <Badge variant={DECISION_VARIANT[e.decision] ?? "neutral"}>{e.decision}</Badge>
                    </div>
                    {e.result_summary && (
                      <div className="mt-1 break-words text-xs text-muted">{e.result_summary}</div>
                    )}
                    {e.args && (
                      <pre className="mt-1 max-h-20 overflow-y-auto whitespace-pre-wrap break-words rounded bg-surface-2 p-1 font-mono text-[11px] text-faint">
                        {e.args}
                      </pre>
                    )}
                    <div className="mt-1 text-[11px] text-faint">
                      {new Date(e.created_at).toLocaleTimeString()}
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
          <div>
            <h3 className="text-xs font-semibold uppercase tracking-wide text-muted">Undo</h3>
            <p className="mt-1 text-muted">Roll back the most recent file change in this session.</p>
            <Button variant="secondary" size="sm" className="mt-2" onClick={revert}>
              <Undo2 className="size-3.5" /> Revert last change
            </Button>
          </div>
        </aside>
      )}
    </div>
  );
}
