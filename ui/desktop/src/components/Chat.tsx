import { useEffect, useRef, useState } from "react";
import {
  Check,
  CircleAlert,
  PanelRight,
  Plus,
  SendHorizontal,
  ShieldCheck,
  Square,
  Undo2,
  Wrench,
  X,
} from "lucide-react";
import {
  MastersClient,
  type AuditEntryDto,
  type PendingApproval,
  type SessionDto,
} from "../api/client";
import { Badge, Button, IconButton, Input, PandaMark, Select } from "./ui";

const DECISION_VARIANT: Record<string, "neutral" | "success" | "danger"> = {
  auto: "neutral",
  approved: "success",
  denied: "danger",
};

interface Turn {
  role: "user" | "assistant" | "tool";
  content: string;
  isError?: boolean;
}

export function Chat({ client }: { client: MastersClient }) {
  const [sessions, setSessions] = useState<SessionDto[]>([]);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [approval, setApproval] = useState<PendingApproval | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [showPanel, setShowPanel] = useState(false);
  const [audit, setAudit] = useState<AuditEntryDto[]>([]);
  const wsRef = useRef<WebSocket | null>(null);

  async function refreshAudit(id = sessionId) {
    if (!id) return;
    try {
      setAudit(await client.listAudit(id));
    } catch {
      /* non-fatal: the panel just shows no rows */
    }
  }

  async function refreshSessions() {
    try {
      setSessions(await client.listSessions());
    } catch {
      /* non-fatal: the switcher just stays empty */
    }
  }

  // On mount: start a fresh chat session, and load the list of prior sessions to switch between.
  useEffect(() => {
    client
      .createSession("Desktop chat")
      .then((s) => {
        setSessionId(s.id);
        refreshSessions();
      })
      .catch((e) => console.error(e));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client]);

  // Keep the audit trail in sync with the active session whenever the panel is open.
  useEffect(() => {
    if (showPanel) refreshAudit();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [showPanel, sessionId]);

  async function newChat() {
    try {
      const s = await client.createSession("Desktop chat");
      setSessionId(s.id);
      setTurns([]);
      setNotice(null);
      refreshSessions();
    } catch (e) {
      setNotice(`New chat failed: ${String(e)}`);
    }
  }

  async function switchSession(id: string) {
    if (id === sessionId || streaming) return;
    setSessionId(id);
    setNotice(null);
    try {
      const msgs = await client.listMessages(id);
      setTurns(
        msgs.map((m) => ({
          role: (m.role === "assistant" || m.role === "tool" ? m.role : "user") as Turn["role"],
          content: m.content,
        })),
      );
    } catch (e) {
      setTurns([]);
      setNotice(`Could not load session: ${String(e)}`);
    }
  }

  function appendAssistant(text: string) {
    setTurns((t) => {
      const next = [...t];
      const last = next[next.length - 1];
      if (last && last.role === "assistant") {
        next[next.length - 1] = { role: "assistant", content: last.content + text };
      } else {
        next.push({ role: "assistant", content: text });
      }
      return next;
    });
  }

  function send() {
    if (!sessionId || !input.trim() || streaming) return;
    const content = input.trim();
    setInput("");
    setTurns((t) => [...t, { role: "user", content }, { role: "assistant", content: "" }]);
    setStreaming(true);

    wsRef.current = client.openStream(sessionId, content, {
      onDelta: appendAssistant,
      onToolCall: (_id, tool, summary) =>
        setTurns((t) => [...t, { role: "tool", content: `${tool} · ${summary}` }]),
      onToolResult: (_id, summary, isError) =>
        setTurns((t) => [...t, { role: "tool", content: summary, isError }]),
      onApproval: (a) => setApproval(a),
      onComplete: () => {
        setStreaming(false);
        refreshSessions();
        refreshAudit();
      },
      onError: (message) => {
        setStreaming(false);
        setTurns((t) => [...t, { role: "assistant", content: `⚠️ ${message}` }]);
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
    setStreaming(false);
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
        {/* Session bar: switch between prior chats or start a new one. */}
        <div className="flex items-center gap-2 border-b border-border px-3 py-2">
          <Select
            className="max-w-xs"
            value={sessionId ?? ""}
            onChange={(e) => switchSession(e.target.value)}
            disabled={streaming}
          >
            {sessions.length === 0 && <option value="">Desktop chat</option>}
            {sessions.map((s) => (
              <option key={s.id} value={s.id}>
                {(s.title || "Untitled") + " · " + new Date(s.updated_at).toLocaleString()}
              </option>
            ))}
          </Select>
          <Button variant="ghost" size="sm" onClick={newChat} disabled={streaming}>
            <Plus className="size-4" /> New chat
          </Button>
          <div className="flex-1" />
          <IconButton
            label={showPanel ? "Hide details" : "Show details"}
            onClick={() => setShowPanel((v) => !v)}
          >
            <PanelRight className="size-4" />
          </IconButton>
        </div>

        <div className="flex-1 space-y-3 overflow-y-auto p-6">
          {turns.length === 0 && (
            <div className="mx-auto mt-16 flex max-w-sm flex-col items-center text-center">
              <PandaMark className="size-16 opacity-90" />
              <h2 className="mt-4 text-xl font-semibold text-text">
                What can Masters do for you?
              </h2>
              <p className="mt-2 text-sm text-muted">
                Ask Masters to work on your granted files — every tool call is gated and audited.
              </p>
            </div>
          )}
          {turns.map((turn, i) =>
            turn.role === "tool" ? (
              <div
                key={i}
                className="flex items-start gap-2 rounded border border-tool-border bg-tool-bg px-3 py-2 font-mono text-xs text-tool-fg"
              >
                {turn.isError ? (
                  <CircleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden />
                ) : (
                  <Wrench className="mt-0.5 size-3.5 shrink-0" aria-hidden />
                )}
                <span className="whitespace-pre-wrap break-words">{turn.content}</span>
              </div>
            ) : (
              <div key={i} className={turn.role === "user" ? "text-right" : "text-left"}>
                <span
                  className={
                    "inline-block max-w-[80%] whitespace-pre-wrap rounded-lg px-3 py-2 text-sm " +
                    (turn.role === "user"
                      ? "rounded-br-sm bg-accent text-accent-fg"
                      : "rounded-bl-sm bg-surface-2 text-text")
                  }
                >
                  {turn.content || (turn.role === "assistant" ? "…" : "")}
                </span>
              </div>
            ),
          )}
        </div>

        {approval && (
          <div className="border-t border-tool-border bg-tool-bg p-3 text-sm">
            <p className="mb-2 text-text">
              Approve <Badge variant="tool">{approval.tool}</Badge> — {approval.summary}{" "}
              <span className="text-muted">[{approval.classes.join(", ")}]</span>?
            </p>
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
          <div className="border-t border-border bg-surface px-4 py-2 text-xs text-muted">
            {notice}
          </div>
        )}

        <div className="flex gap-2 border-t border-border bg-bg p-3">
          <Input
            className="flex-1"
            placeholder={sessionId ? "Message Masters…" : "Connecting…"}
            value={input}
            disabled={!sessionId}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && send()}
          />
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
        </div>
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
              <h3 className="text-xs font-semibold uppercase tracking-wide text-muted">
                Audit trail
              </h3>
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
                      <Badge variant={DECISION_VARIANT[e.decision] ?? "neutral"}>
                        {e.decision}
                      </Badge>
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
