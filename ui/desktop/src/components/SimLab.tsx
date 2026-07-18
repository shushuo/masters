// 模拟盘 — the Simulation Investment Lab (inspired by Alpha Arena + the RETuning paper).
// User-configured masters compete under fixed conditions: each round they reason (framework →
// evidence → decision) and emit a target allocation; the deterministic engine settles at live
// close prices. Everything here is explicitly 模拟/假设 — never real trades, never advice.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { FlaskConical, MessageCircleQuestion, Plus, Play, Download, Trash2, X } from "lucide-react";
import type {
  CreateSimulationRequest,
  MasterSummaryDto,
  MastersClient,
  SimulationDto,
  SimLeaderboardRowDto,
  SimRoundDto,
  SimDecisionDto,
} from "../api/client";
import { Badge, Button, Card, IconButton, Input, Markdown } from "./ui";
import { masterColor, masterGlyph, masterName, BENCHMARK_SLUG } from "../lib/masters";
import { getLocale } from "../lib/i18n";

const zh = getLocale() === "zh";
const L = (cn: string, en: string) => (zh ? cn : en);

const FOOTER = L(
  "ⓘ 模拟结果为假设推演，非真实交易，不构成投资建议，不荐股",
  "ⓘ Simulated results are hypothetical — not real trades, not investment advice",
);

/** Schedule presets (label → cron). Post-close = 07:30 UTC ≈ 15:30 Beijing. */
const SCHEDULE_PRESETS: { key: string; label: string; cron: string | null }[] = [
  { key: "off", label: L("不定时（手动运行）", "Manual only"), cron: null },
  { key: "daily", label: L("每个交易日收盘后", "Each trading day, post-close"), cron: "30 7 * * MON-FRI" },
  { key: "weekly", label: L("每周一开盘前", "Weekly, Monday pre-open"), cron: "0 1 * * MON" },
];

function pct(v: number | null | undefined): string {
  if (v == null) return "—";
  return `${v >= 0 ? "▲" : "▼"} ${(v * 100).toFixed(2)}%`;
}

function money(v: number | null | undefined): string {
  if (v == null) return "—";
  return v.toLocaleString(undefined, { maximumFractionDigits: 0 });
}

/** A master face chip (roster identity; benchmark gets its own glyph/label). */
function Face({ slug, size = 28 }: { slug: string; size?: number }) {
  return (
    <span
      className="inline-flex shrink-0 items-center justify-center rounded-full font-medium text-white"
      style={{ width: size, height: size, background: masterColor(slug), fontSize: size * 0.42 }}
      title={masterName(slug)}
    >
      {masterGlyph(slug)}
    </span>
  );
}

/** Cumulative return in CN gain/loss color with ▲/▼ redundant coding (color-blind rule). */
function ReturnText({ v, className = "" }: { v: number | null | undefined; className?: string }) {
  const up = (v ?? 0) >= 0;
  return (
    <span className={`${v == null ? "text-muted" : up ? "text-gain" : "text-loss"} ${className}`}>
      {pct(v)}
    </span>
  );
}

/** A tiny equity sparkline from a cumulative-return series. */
function Sparkline({ series }: { series: number[] }) {
  if (series.length < 2) return <span className="text-xs text-faint">—</span>;
  const min = Math.min(...series, 0);
  const max = Math.max(...series, 0);
  const span = max - min || 1;
  const w = 72;
  const h = 22;
  const pts = series
    .map((v, i) => {
      const x = (i / (series.length - 1)) * w;
      const y = h - ((v - min) / span) * h;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  const last = series[series.length - 1];
  return (
    <svg width={w} height={h} className="overflow-visible">
      <polyline
        points={pts}
        fill="none"
        stroke={last >= 0 ? "var(--color-gain)" : "var(--color-loss)"}
        strokeWidth="1.5"
      />
    </svg>
  );
}

function Leaderboard({ rows }: { rows: SimLeaderboardRowDto[] }) {
  if (rows.length === 0) return null;
  return (
    <div className="space-y-1">
      {rows.map((r, i) => (
        <div
          key={r.master_slug}
          className="flex items-center gap-3 rounded-lg border border-border bg-surface px-3 py-2"
        >
          <span className="w-5 text-center text-sm font-medium text-faint">{i + 1}</span>
          <Face slug={r.master_slug} />
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-medium">{masterName(r.master_slug)}</div>
            <div className="text-xs text-muted">
              {L("净值", "NAV")} {money(r.nav)}
              {(r.unvalued_count ?? 0) > 0 && (
                <span className="text-warning-fg"> · {r.unvalued_count} {L("项未估值", "unvalued")} ⚠</span>
              )}
            </div>
          </div>
          <Sparkline series={r.equity ?? []} />
          <div className="w-28 text-right">
            <div className="text-base font-semibold">
              <ReturnText v={r.return_pct} />
            </div>
            {r.alpha != null && (
              <div className="text-xs text-muted">
                {L("超额", "α")} <ReturnText v={r.alpha} className="text-xs" />
              </div>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

function DecisionCard({
  d,
  simName,
  roundNo,
  onAsk,
}: {
  d: SimDecisionDto;
  simName: string;
  roundNo: number;
  onAsk: (draft: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const targets = Object.entries(d.targets ?? {}).sort((a, b) => b[1] - a[1]);
  return (
    <Card className="p-3">
      <div className="flex items-center gap-2">
        <Face slug={d.master_slug} size={24} />
        <span className="text-sm font-medium">{masterName(d.master_slug)}</span>
        {!d.parsed && d.master_slug !== BENCHMARK_SLUG && (
          <Badge>{L("维持不动", "held")}</Badge>
        )}
        <span className="ml-auto text-sm font-semibold">
          <ReturnText v={d.return_pct} />
        </span>
      </div>
      {targets.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1">
          {targets.map(([sym, w]) => (
            <Badge key={sym}>
              {sym === "现金" ? L("现金", "cash") : sym} {Math.round(w)}%
            </Badge>
          ))}
        </div>
      )}
      {d.summary && <div className="mt-2 text-xs text-muted">{d.summary}</div>}
      {d.reasoning && (
        <div className="mt-2">
          <button
            className="text-xs text-accent hover:underline"
            onClick={() => setOpen((o) => !o)}
          >
            {open ? L("收起推理", "Hide reasoning") : L("查看推理", "Show reasoning")}
          </button>
          {open && (
            <div className="mt-2 max-h-80 overflow-y-auto rounded-lg border border-border bg-bg p-3">
              <Markdown text={d.reasoning} />
            </div>
          )}
        </div>
      )}
      {d.tokens != null && (
        <div className="mt-1 text-[11px] text-faint">{d.tokens} tokens</div>
      )}
      <button
        className="mt-2 inline-flex items-center gap-1 text-xs text-muted hover:text-text"
        onClick={() =>
          onAsk(
            L(
              `关于「${simName}」模拟盘第 ${roundNo} 轮，${masterName(d.master_slug)}的决策，我想请教：`,
              `About "${simName}" round ${roundNo}, ${masterName(d.master_slug, "en")}'s decision, I'd like to ask: `,
            ),
          )
        }
      >
        <MessageCircleQuestion className="h-3.5 w-3.5" />
        {L("就此提问", "Ask about this")}
      </button>
    </Card>
  );
}

function CreateForm({
  masters,
  onCreate,
  onCancel,
  busy,
}: {
  masters: MasterSummaryDto[];
  onCreate: (body: CreateSimulationRequest) => void;
  onCancel: () => void;
  busy: boolean;
}) {
  const [name, setName] = useState("");
  const [scenario, setScenario] = useState("");
  const [universe, setUniverse] = useState("");
  const [cash, setCash] = useState("100000");
  const [benchmark, setBenchmark] = useState("sh000300");
  const [longOnly, setLongOnly] = useState(true);
  const [maxWeight, setMaxWeight] = useState("");
  const [cashFloor, setCashFloor] = useState("");
  const [feeBps, setFeeBps] = useState("");
  const [picked, setPicked] = useState<Set<string>>(new Set());

  const toggle = (slug: string) =>
    setPicked((p) => {
      const n = new Set(p);
      n.has(slug) ? n.delete(slug) : n.add(slug);
      return n;
    });

  const canSubmit =
    name.trim() !== "" &&
    universe.trim() !== "" &&
    picked.size > 0 &&
    Number(cash) > 0 &&
    !busy;

  const submit = () => {
    const body: CreateSimulationRequest = {
      name: name.trim(),
      scenario: scenario.trim() || undefined,
      universe: universe
        .split(/[,，、\s]+/)
        .map((s) => s.trim())
        .filter(Boolean),
      starting_cash: Number(cash),
      participants: [...picked],
      constraints: {
        long_only: longOnly,
        benchmark: benchmark.trim() || undefined,
        max_weight: maxWeight ? Number(maxWeight) / 100 : undefined,
        cash_floor: cashFloor ? Number(cashFloor) / 100 : undefined,
        fee_bps: feeBps ? Number(feeBps) : undefined,
      },
    };
    onCreate(body);
  };

  return (
    <Card className="space-y-3 p-4">
      <div className="flex items-center justify-between">
        <h3 className="font-display text-base">{L("新建模拟盘", "New simulation")}</h3>
        <IconButton onClick={onCancel} label={L("关闭", "Close")}>
          <X className="h-4 w-4" />
        </IconButton>
      </div>
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="text-sm">
          <span className="mb-1 block text-muted">{L("名称", "Name")}</span>
          <Input value={name} onChange={(e) => setName(e.target.value)} placeholder={L("如：熊市防御赛", "e.g. Bear-market defense")} />
        </label>
        <label className="text-sm">
          <span className="mb-1 block text-muted">{L("初始资金", "Starting cash")}</span>
          <Input type="number" value={cash} onChange={(e) => setCash(e.target.value)} />
        </label>
      </div>
      <label className="block text-sm">
        <span className="mb-1 block text-muted">{L("情景说明（可选）", "Scenario (optional)")}</span>
        <Input value={scenario} onChange={(e) => setScenario(e.target.value)} placeholder={L("如：只做沪深300成分股，防御为主", "e.g. CSI 300 only, defensive")} />
      </label>
      <label className="block text-sm">
        <span className="mb-1 block text-muted">{L("股票池（逗号分隔的代码）", "Universe (comma-separated codes)")}</span>
        <Input value={universe} onChange={(e) => setUniverse(e.target.value)} placeholder="600519, 000001, 300750" />
      </label>
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="text-sm">
          <span className="mb-1 block text-muted">{L("基准（可选）", "Benchmark (optional)")}</span>
          <Input value={benchmark} onChange={(e) => setBenchmark(e.target.value)} placeholder="sh000300" />
        </label>
        <label className="text-sm">
          <span className="mb-1 block text-muted">{L("单标的上限 %（可选）", "Max weight % (optional)")}</span>
          <Input type="number" value={maxWeight} onChange={(e) => setMaxWeight(e.target.value)} placeholder="40" />
        </label>
        <label className="text-sm">
          <span className="mb-1 block text-muted">{L("现金下限 %（可选）", "Cash floor % (optional)")}</span>
          <Input type="number" value={cashFloor} onChange={(e) => setCashFloor(e.target.value)} placeholder="0" />
        </label>
        <label className="text-sm">
          <span className="mb-1 block text-muted">{L("交易费 bp（可选）", "Fee bps (optional)")}</span>
          <Input type="number" value={feeBps} onChange={(e) => setFeeBps(e.target.value)} placeholder="0" />
        </label>
      </div>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" checked={longOnly} onChange={(e) => setLongOnly(e.target.checked)} />
        {L("仅做多", "Long-only")}
      </label>
      <div>
        <span className="mb-1 block text-sm text-muted">{L("参赛大师", "Participating masters")}</span>
        {masters.length === 0 ? (
          <p className="text-xs text-faint">{L("暂无可用大师，先在高级工作台创建或从云端同步。", "No masters yet — create or sync some in the Lab.")}</p>
        ) : (
          <div className="flex flex-wrap gap-2">
            {masters.map((m) => {
              const on = picked.has(m.slug);
              return (
                <button
                  key={m.slug}
                  onClick={() => toggle(m.slug)}
                  className={`flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-sm transition-colors ${
                    on ? "border-accent bg-accent-subtle text-text" : "border-border text-muted hover:text-text"
                  }`}
                >
                  <Face slug={m.slug} size={20} />
                  {masterName(m.slug)}
                </button>
              );
            })}
          </div>
        )}
      </div>
      <div className="flex justify-end gap-2 pt-1">
        <Button variant="ghost" onClick={onCancel}>{L("取消", "Cancel")}</Button>
        <Button onClick={submit} disabled={!canSubmit}>
          {busy ? L("创建中…", "Creating…") : L("创建", "Create")}
        </Button>
      </div>
    </Card>
  );
}

export default function SimLab({
  client,
  onAsk,
  simId,
  onOpen,
}: {
  client: MastersClient;
  onAsk: (draft?: string) => void;
  /** Deep-linked simulation id (`#/simlab/:sid`); undefined = the list. */
  simId?: string;
  /** Navigate to a simulation (or back to the list with `null`). */
  onOpen: (sid: string | null) => void;
}) {
  const [projectId, setProjectId] = useState<string | null>(null);
  const [masters, setMasters] = useState<MasterSummaryDto[]>([]);
  const [sims, setSims] = useState<SimulationDto[]>([]);
  const [selected, setSelected] = useState<SimulationDto | null>(null);
  const [rounds, setRounds] = useState<SimRoundDto[]>([]);
  const [creating, setCreating] = useState(false);
  const [busy, setBusy] = useState(false);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  // Live round stream (WS): per-master reasoning accumulating token-by-token.
  const [live, setLive] = useState<{
    round: number;
    byAuthor: Record<string, { text: string; done: boolean }>;
  } | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  useEffect(() => () => wsRef.current?.close(), []);

  const loadList = useCallback(
    async (pid: string) => {
      const list = await client.listSimulations(pid);
      setSims(list);
      return list;
    },
    [client],
  );

  useEffect(() => {
    (async () => {
      try {
        const ws = await client.ensureInvestingWorkspace();
        setProjectId(ws.project_id);
        const [ms] = await Promise.all([client.listGlobalMasters(), loadList(ws.project_id)]);
        setMasters(ms);
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    })();
  }, [client, loadList]);

  const openDetail = useCallback(
    async (sid: string) => {
      if (!projectId) return;
      setError(null);
      const [sim, rs] = await Promise.all([
        client.getSimulation(projectId, sid),
        client.listSimulationRounds(projectId, sid),
      ]);
      setSelected(sim);
      setRounds(rs);
    },
    [client, projectId],
  );

  // Selection is route-driven (`#/simlab/:sid`): open the deep-linked sim, else show the list.
  useEffect(() => {
    if (!projectId) return;
    if (simId) {
      openDetail(simId).catch((e) => {
        setError(String(e));
        onOpen(null);
      });
    } else {
      setSelected(null);
      setRounds([]);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [simId, projectId]);

  const create = async (body: CreateSimulationRequest) => {
    if (!projectId) return;
    setBusy(true);
    setError(null);
    try {
      const sim = await client.createSimulation(projectId, body);
      setCreating(false);
      await loadList(projectId);
      onOpen(sim.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  // Run a round with a live WebSocket stream: each master's reasoning appears token-by-token; the
  // engine settles deterministically on the server, and we refresh the leaderboard + rounds when
  // the round completes.
  const runRound = () => {
    if (!projectId || !selected) return;
    const sid = selected.id;
    setError(null);
    setRunning(true);
    setLive({ round: 1, byAuthor: {} });
    const patch = (author: string, fn: (s: { text: string; done: boolean }) => { text: string; done: boolean }) =>
      setLive((cur) =>
        cur
          ? { ...cur, byAuthor: { ...cur.byAuthor, [author]: fn(cur.byAuthor[author] ?? { text: "", done: false }) } }
          : cur,
      );
    const ws = client.openSimStream(projectId, sid, {
      onDelta: () => {},
      onComplete: () => {},
      onError: (m) => {
        setError(m);
        setRunning(false);
        setLive(null);
        wsRef.current = null;
      },
      onGroupStart: (round, addressed) =>
        setLive({ round, byAuthor: Object.fromEntries(addressed.map((a) => [a, { text: "", done: false }])) }),
      onMasterDelta: (_r, author, text) => patch(author, (s) => ({ text: s.text + text, done: false })),
      onMasterComplete: (_r, author) => patch(author, (s) => ({ ...s, done: true })),
      onMasterError: (_r, author, message) =>
        patch(author, (s) => ({ text: `${s.text}\n\n[错误] ${message}`, done: true })),
      onGroupComplete: () => {
        wsRef.current = null;
        void (async () => {
          await openDetail(sid);
          await loadList(projectId);
          setLive(null);
          setRunning(false);
        })();
      },
    });
    wsRef.current = ws;
  };

  const stopRound = () => {
    const ws = wsRef.current;
    if (ws) {
      try {
        ws.send(JSON.stringify({ type: "stop" }));
      } catch {
        /* socket may already be closing */
      }
      ws.close();
      wsRef.current = null;
    }
    setRunning(false);
    setLive(null);
    if (projectId && selected) void openDetail(selected.id);
  };

  const changeState = async (s: "active" | "paused" | "ended") => {
    if (!projectId || !selected) return;
    try {
      const sim = await client.setSimulationState(projectId, selected.id, s);
      setSelected(sim);
      await loadList(projectId);
    } catch (e) {
      setError(String(e));
    }
  };

  const setSchedule = async (cron: string | null) => {
    if (!projectId || !selected) return;
    try {
      const sim = await client.setSimulationSchedule(projectId, selected.id, {
        cron_expr: cron,
        deliver_notify: cron != null,
        deliver_email: false,
      });
      setSelected(sim);
    } catch (e) {
      setError(String(e));
    }
  };

  const removeSim = async (sid: string) => {
    if (!projectId) return;
    await client.deleteSimulation(projectId, sid);
    onOpen(null);
    await loadList(projectId);
  };

  const exportReport = async () => {
    if (!projectId || !selected) return;
    try {
      const { markdown } = await client.getSimulationReport(projectId, selected.id);
      const blob = new Blob([markdown], { type: "text/markdown;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${selected.name.replace(/[\\/:*?"<>|]/g, "_")}-模拟报告.md`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      setError(String(e));
    }
  };

  const resetSim = async () => {
    if (!projectId || !selected) return;
    if (!confirm(L("重置将清空全部轮次与持仓，回到第 0 轮（保留配置）。确定？", "Reset clears all rounds and holdings back to round 0 (config kept). Continue?"))) {
      return;
    }
    try {
      const sim = await client.resetSimulation(projectId, selected.id);
      setSelected(sim);
      setRounds([]);
      await loadList(projectId);
    } catch (e) {
      setError(String(e));
    }
  };

  const currentPreset = useMemo(() => {
    const cron = selected?.schedule_cron ?? null;
    return SCHEDULE_PRESETS.find((p) => p.cron === cron)?.key ?? "off";
  }, [selected]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex items-center gap-2 border-b border-border px-6 py-4">
        <FlaskConical className="h-5 w-5 text-accent" />
        <div>
          <h1 className="font-display text-lg">{L("模拟投资实验室", "Simulation Investment Lab")}</h1>
          <p className="text-xs text-muted">
            {L("让大师在给定条件下做模拟投资，观察其思考与结果", "Masters paper-trade under fixed conditions — watch their reasoning and results")}
          </p>
        </div>
        {selected ? (
          <Button variant="ghost" className="ml-auto" onClick={() => onOpen(null)}>
            ← {L("全部模拟盘", "All simulations")}
          </Button>
        ) : (
          !creating && (
            <Button className="ml-auto" onClick={() => setCreating(true)}>
              <Plus className="h-4 w-4" /> {L("新建", "New")}
            </Button>
          )
        )}
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto p-6">
        <div className="mx-auto max-w-3xl space-y-4">
          {error && (
            <div className="rounded-lg border border-danger bg-danger-bg p-3 text-sm text-danger-fg">
              {error}
            </div>
          )}

          {loading ? (
            <p className="text-sm text-muted">{L("加载中…", "Loading…")}</p>
          ) : creating ? (
            <CreateForm masters={masters} onCreate={create} onCancel={() => setCreating(false)} busy={busy} />
          ) : selected ? (
            <>
              {/* Detail header */}
              <div className="flex items-start justify-between">
                <div>
                  <div className="flex items-center gap-2">
                    <h2 className="font-display text-xl">{selected.name}</h2>
                    {selected.state === "paused" && <Badge>{L("已暂停", "Paused")}</Badge>}
                    {selected.state === "ended" && <Badge>{L("已结束", "Ended")}</Badge>}
                  </div>
                  {selected.scenario && <p className="text-sm text-muted">{selected.scenario}</p>}
                  <p className="mt-1 text-xs text-faint">
                    {L("已进行", "Rounds")} {selected.round_no} · {L("初始资金", "start")} {money(selected.starting_cash)} ·{" "}
                    {selected.universe.join("、")}
                  </p>
                </div>
                <IconButton onClick={() => removeSim(selected.id)} label={L("删除", "Delete")}>
                  <Trash2 className="h-4 w-4" />
                </IconButton>
              </div>

              {/* Controls */}
              <div className="flex flex-wrap items-center gap-3">
                <Button
                  onClick={runRound}
                  disabled={running || selected.state === "paused" || selected.state === "ended"}
                >
                  <Play className="h-4 w-4" />
                  {running ? L("运行中…", "Running…") : L("运行一轮", "Run a round")}
                </Button>
                {running && (
                  <Button variant="ghost" onClick={stopRound}>
                    {L("停止", "Stop")}
                  </Button>
                )}
                {selected.state !== "ended" && (
                  <Button
                    variant="ghost"
                    disabled={running}
                    onClick={() => changeState(selected.state === "paused" ? "active" : "paused")}
                  >
                    {selected.state === "paused" ? L("继续", "Resume") : L("暂停", "Pause")}
                  </Button>
                )}
                {selected.state !== "ended" && (
                  <Button variant="ghost" disabled={running} onClick={() => changeState("ended")}>
                    {L("结束", "End")}
                  </Button>
                )}
                {selected.round_no > 0 && (
                  <Button variant="ghost" onClick={exportReport}>
                    <Download className="h-4 w-4" /> {L("导出报告", "Export")}
                  </Button>
                )}
                {selected.round_no > 0 && (
                  <Button variant="ghost" disabled={running} onClick={resetSim}>
                    {L("重置", "Reset")}
                  </Button>
                )}
                <label className="flex items-center gap-2 text-sm text-muted">
                  {L("定时", "Schedule")}
                  <select
                    className="rounded-full border border-border bg-surface px-2 py-1 text-sm text-text"
                    value={currentPreset}
                    onChange={(e) =>
                      setSchedule(SCHEDULE_PRESETS.find((p) => p.key === e.target.value)?.cron ?? null)
                    }
                  >
                    {SCHEDULE_PRESETS.map((p) => (
                      <option key={p.key} value={p.key}>
                        {p.label}
                      </option>
                    ))}
                  </select>
                </label>
                {running && (
                  <span className="text-xs text-faint">
                    {L(`本轮将调用 ${(selected.participants ?? []).filter((p) => p.master_slug !== BENCHMARK_SLUG).length} 位大师`, "calling masters…")}
                  </span>
                )}
              </div>

              {/* Live round stream */}
              {live && (
                <section className="space-y-2">
                  <h3 className="text-sm font-medium text-muted">
                    {L("本轮进行中 · 实时推理", "This round · live reasoning")}
                  </h3>
                  <div className="grid gap-2 sm:grid-cols-2">
                    {Object.entries(live.byAuthor).map(([author, s]) => (
                      <Card key={author} className="p-3">
                        <div className="flex items-center gap-2">
                          <Face slug={author} size={24} />
                          <span className="text-sm font-medium">{masterName(author)}</span>
                          {s.done ? (
                            <Badge className="ml-auto">{L("已完成", "done")}</Badge>
                          ) : (
                            <span className="ml-auto animate-pulse text-xs text-accent">▍</span>
                          )}
                        </div>
                        <div className="mt-2 max-h-72 overflow-y-auto text-sm">
                          {s.text ? (
                            <Markdown text={s.text} />
                          ) : (
                            <span className="text-faint">{L("思考中…", "thinking…")}</span>
                          )}
                        </div>
                      </Card>
                    ))}
                  </div>
                </section>
              )}

              {/* Leaderboard */}
              <section>
                <h3 className="mb-2 text-sm font-medium text-muted">{L("排行榜", "Leaderboard")}</h3>
                {(selected.participants ?? []).length === 0 ? (
                  <p className="text-sm text-faint">{L("尚无参赛者", "No participants")}</p>
                ) : (
                  <Leaderboard rows={selected.participants ?? []} />
                )}
              </section>

              {/* Rounds */}
              <section className="space-y-3">
                <h3 className="text-sm font-medium text-muted">{L("轮次记录", "Rounds")}</h3>
                {rounds.length === 0 ? (
                  <p className="text-sm text-faint">
                    {L("还没有轮次，点击「运行一轮」开始。", "No rounds yet — click Run a round.")}
                  </p>
                ) : (
                  rounds.map((r) => (
                    <div key={r.round_no} className="space-y-2">
                      <div className="flex items-center gap-2 text-sm text-muted">
                        <span className="font-medium text-text">
                          {L("第", "Round")} {r.round_no} {L("轮", "")}
                        </span>
                        {r.quote_date && <span className="text-xs text-faint">{L("行情", "as of")} {r.quote_date}</span>}
                      </div>
                      <div className="grid gap-2 sm:grid-cols-2">
                        {(r.decisions ?? []).map((d) => (
                          <DecisionCard
                            key={`${r.round_no}-${d.master_slug}`}
                            d={d}
                            simName={selected.name}
                            roundNo={r.round_no}
                            onAsk={onAsk}
                          />
                        ))}
                      </div>
                    </div>
                  ))
                )}
              </section>
            </>
          ) : sims.length === 0 ? (
            <Card className="p-8 text-center">
              <FlaskConical className="mx-auto h-8 w-8 text-faint" />
              <h2 className="mt-3 font-display text-lg">{L("还没有模拟盘", "No simulations yet")}</h2>
              <p className="mx-auto mt-1 max-w-md text-sm text-muted">
                {L(
                  "创建一个模拟盘，选几位大师、给定股票池与初始资金，让他们在真实行情下做模拟投资，比一比谁的判断更稳。",
                  "Create a simulation, pick a few masters, set a universe and starting capital, and watch them paper-trade against the live market.",
                )}
              </p>
              <Button className="mx-auto mt-4" onClick={() => setCreating(true)}>
                <Plus className="h-4 w-4" /> {L("新建模拟盘", "New simulation")}
              </Button>
            </Card>
          ) : (
            <div className="space-y-2">
              {sims.map((s) => {
                const parts = s.participants ?? [];
                const top = parts[0];
                return (
                  <button
                    key={s.id}
                    onClick={() => onOpen(s.id)}
                    className="flex w-full items-center gap-3 rounded-lg border border-border bg-surface px-4 py-3 text-left transition-colors hover:border-accent"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="truncate font-medium">{s.name}</div>
                      <div className="text-xs text-muted">
                        {L("已进行", "Rounds")} {s.round_no} · {parts.length} {L("位参赛", "players")}
                        {s.schedule_cron && <span> · ⏱ {L("定时中", "scheduled")}</span>}
                      </div>
                    </div>
                    {top && (
                      <div className="text-right">
                        <div className="flex items-center justify-end gap-1.5 text-sm">
                          <Face slug={top.master_slug} size={20} />
                          <span className="text-muted">{L("领先", "leader")}</span>
                        </div>
                        <ReturnText v={top.return_pct} className="text-sm font-semibold" />
                      </div>
                    )}
                  </button>
                );
              })}
            </div>
          )}

          <p className="pt-2 text-center text-xs text-faint">{FOOTER}</p>
        </div>
      </div>
    </div>
  );
}
