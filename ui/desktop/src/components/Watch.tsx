// 关注 — the asset library (docs/12 §3.2): one lifecycle spine rendered as sections.
// 「持有」 (portfolio strip + valued position cards) sits above 「关注中」 (watch cards whose
// reason line is the "it remembers me" moment). Honest quotes throughout: provenance +
// data-as-of always visible, stale flagged, missing data stated — never invented.
// D10: no "since watching ±%" anywhere — hypothetical returns belong to coached reviews only.
import { useCallback, useEffect, useMemo, useState } from "react";
import { MessageCircleQuestion, Star, Trash2 } from "lucide-react";
import type {
  AssetDto,
  InvestingWorkspaceDto,
  MastersClient,
  PortfolioDto,
  QuoteDto,
} from "../api/client";
import { Badge, Button, Card, IconButton } from "./ui";
import { t } from "../lib/i18n";
import { dailyQuote } from "../lib/quotes";

function stateLabel(state: string): string {
  if (state === "holding") return t("watch.stateHolding");
  if (state === "sold") return t("watch.stateSold");
  return t("watch.stateWatching");
}

/** `1721026800000` → `2026-07-15` (local date, for the watched-at line). */
function dateOf(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** Latest-quote line: change% in gain/loss color with ▲/▼ redundant coding (color-blind rule). */
function QuoteLine({ quote }: { quote: QuoteDto | undefined }) {
  if (!quote) {
    return <span className="text-sm text-muted">{t("watch.noQuote")}</span>;
  }
  const pct = quote.change_pct;
  const up = (pct ?? 0) >= 0;
  return (
    <span className="flex items-baseline gap-2 whitespace-nowrap">
      {quote.close != null && (
        <span className="font-medium tabular-nums">{quote.close.toFixed(2)}</span>
      )}
      {pct != null && (
        <span className={`tabular-nums text-sm ${up ? "text-gain" : "text-loss"}`}>
          {up ? "▲" : "▼"} {up ? "+" : ""}
          {pct.toFixed(2)}%
        </span>
      )}
      <span className="text-xs text-faint">
        {t("watch.dataAsOf")} {quote.trade_date} · {quote.source}
        {quote.stale
          ? ` · ⚠ ${t("watch.stale")}`
          : quote.validation === "disputed"
            ? ` · ⚠ ${t("watch.disputed")}`
            : ""}
      </span>
    </span>
  );
}

/** A watch-list card: the reason line leads (docs/12 — "it remembers why you cared"). */
function WatchCard({
  asset,
  quote,
  onUntrack,
  onAsk,
}: {
  asset: AssetDto;
  quote: QuoteDto | undefined;
  onUntrack: (symbol: string) => void;
  onAsk: (draft: string) => void;
}) {
  return (
    <Card className="flex flex-col gap-2 p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="truncate font-medium">{asset.name}</span>
            <span className="text-xs text-faint">{asset.symbol}</span>
            <Badge>{stateLabel(asset.state)}</Badge>
          </div>
          {asset.watch_reason && (
            <p className="mt-1 truncate text-sm text-muted">
              {t("watch.reasonPrefix")}
              {asset.watch_reason}
            </p>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <IconButton
            label={`${t("watch.ask")} ${asset.name}`}
            onClick={() => onAsk(`关于 ${asset.name}（${asset.symbol}）：`)}
          >
            <MessageCircleQuestion className="h-4 w-4" />
          </IconButton>
          {asset.state === "watching" && (
            <IconButton
              label={`${t("watch.untrack")} ${asset.name}`}
              onClick={() => onUntrack(asset.symbol)}
            >
              <Trash2 className="h-4 w-4" />
            </IconButton>
          )}
        </div>
      </div>
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <span className="text-xs text-faint tabular-nums">
          {t("watch.watchedAt")} {dateOf(asset.watched_at)}
          {asset.snapshot_price != null &&
            ` @ ${asset.snapshot_price.toFixed(2)}${
              asset.snapshot_date ? ` (${asset.snapshot_date})` : ""
            }`}
        </span>
        <QuoteLine quote={quote} />
      </div>
    </Card>
  );
}

/** A holding card: FinCalc's numbers verbatim; missing pieces stated as 未估值, never guessed. */
function HoldingCard({
  asset,
  position,
  quote,
  onAsk,
}: {
  asset: AssetDto;
  position: PortfolioDto["positions"][number] | undefined;
  quote: QuoteDto | undefined;
  onAsk: (draft: string) => void;
}) {
  const stat = (label: string, value: string) => (
    <div className="flex flex-col">
      <span className="text-[11px] text-faint">{label}</span>
      <span className="text-sm font-medium tabular-nums">{value}</span>
    </div>
  );
  return (
    <Card className="flex flex-col gap-3 p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="truncate font-medium">{asset.name}</span>
            <span className="text-xs text-faint">{asset.symbol}</span>
            <Badge variant="accent">{stateLabel(asset.state)}</Badge>
            {position?.value == null && <Badge variant="warning">{t("watch.unvalued")}</Badge>}
          </div>
          {asset.watch_reason && (
            <p className="mt-1 truncate text-sm text-muted">
              {t("watch.reasonPrefix")}
              {asset.watch_reason}
            </p>
          )}
        </div>
        <IconButton
          label={`${t("watch.ask")} ${asset.name}`}
          onClick={() => onAsk(`关于我持有的 ${asset.name}（${asset.symbol}）：`)}
        >
          <MessageCircleQuestion className="h-4 w-4" />
        </IconButton>
      </div>
      <div className="flex flex-wrap items-end justify-between gap-x-6 gap-y-2">
        <div className="flex flex-wrap gap-6">
          {position?.quantity != null && stat(t("watch.qty"), position.quantity.toLocaleString())}
          {position?.cost != null && stat(t("watch.cost"), position.cost.toFixed(2))}
          {position?.value != null &&
            stat(
              t("watch.value"),
              position.value.toLocaleString(undefined, { maximumFractionDigits: 0 }),
            )}
          {position?.weight != null &&
            stat(t("watch.weight"), `${(position.weight * 100).toFixed(1)}%`)}
        </div>
        <QuoteLine quote={quote} />
      </div>
    </Card>
  );
}

/** The portfolio strip: totals + concentration, all verbatim from FinCalc (docs/12 §3.2). */
function PortfolioStrip({ portfolio }: { portfolio: PortfolioDto }) {
  const stat = (label: string, value: string) => (
    <div className="flex flex-col">
      <span className="text-xs text-faint">{label}</span>
      <span className="font-medium tabular-nums">{value}</span>
    </div>
  );
  return (
    <Card className="flex flex-wrap items-center gap-6 bg-surface p-4">
      <span className="font-display text-sm font-semibold">{t("watch.portfolio.title")}</span>
      {portfolio.total_value != null &&
        stat(
          t("watch.portfolio.total"),
          portfolio.total_value.toLocaleString(undefined, { maximumFractionDigits: 0 }),
        )}
      {portfolio.hhi != null && stat(t("watch.portfolio.hhi"), portfolio.hhi.toFixed(2))}
      {portfolio.top3_share != null &&
        stat(t("watch.portfolio.top3"), `${(portfolio.top3_share * 100).toFixed(0)}%`)}
      {portfolio.unvalued_count > 0 && (
        <span className="text-xs text-muted">
          ⚠ {portfolio.unvalued_count} {t("watch.portfolio.unvalued")}
        </span>
      )}
    </Card>
  );
}

export function Watch({
  client,
  onAsk,
}: {
  client: MastersClient;
  /** Open 问大师 with a pre-filled question (empty → a fresh blank topic). */
  onAsk: (draft?: string) => void;
}) {
  const [workspace, setWorkspace] = useState<InvestingWorkspaceDto | null>(null);
  const [assets, setAssets] = useState<AssetDto[] | null>(null);
  const [quotes, setQuotes] = useState<Map<string, QuoteDto>>(new Map());
  const [portfolio, setPortfolio] = useState<PortfolioDto | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(
    async (ws: InvestingWorkspaceDto) => {
      const list = await client.listAssets(ws.project_id);
      setAssets(list);
      // The portfolio section unlocks progressively — only once a holding is recorded.
      if (list.some((a) => a.state === "holding")) {
        try {
          setPortfolio(await client.getPortfolio(ws.project_id));
        } catch {
          setPortfolio(null);
        }
      } else {
        setPortfolio(null);
      }
      const symbols = list.map((a) => a.symbol);
      if (symbols.length > 0) {
        // Quotes degrade independently: a failure leaves cards in the explicit no-data state.
        try {
          const qs = await client.listQuotes(ws.project_id, symbols);
          setQuotes(new Map(qs.map((q) => [q.symbol, q])));
        } catch {
          setQuotes(new Map());
        }
      } else {
        setQuotes(new Map());
      }
    },
    [client],
  );

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const ws = await client.ensureInvestingWorkspace();
        if (cancelled) return;
        setWorkspace(ws);
        await refresh(ws);
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [client, refresh]);

  const onUntrack = useCallback(
    async (symbol: string) => {
      if (!workspace) return;
      try {
        await client.untrackAsset(workspace.project_id, symbol);
        await refresh(workspace);
      } catch (e) {
        setError(String(e));
      }
    },
    [client, workspace, refresh],
  );

  const exampleQuestions = useMemo(
    () => [t("watch.empty.q1"), t("watch.empty.q2"), t("watch.empty.q3")],
    [],
  );

  if (error) {
    return (
      <div className="p-4 text-sm text-danger">
        {t("watch.error")}
        {error}
      </div>
    );
  }
  if (!workspace || assets === null) {
    return <div className="p-4 text-sm text-muted">{t("watch.loading")}</div>;
  }

  const holdings = assets.filter((a) => a.state === "holding");
  const watching = assets.filter((a) => a.state === "watching");
  const sold = assets.filter((a) => a.state === "sold");
  const positionOf = (symbol: string) => portfolio?.positions.find((p) => p.symbol === symbol);
  const quote = dailyQuote();

  const section = (label: string, children: React.ReactNode) => (
    <section>
      <h2 className="mb-2 text-xs font-medium uppercase tracking-wide text-faint">{label}</h2>
      <div className="flex flex-col gap-3">{children}</div>
    </section>
  );

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between gap-3 border-b border-border px-6 py-4">
        <div>
          <h1 className="flex items-center gap-2 font-display text-lg font-semibold">
            <Star className="h-5 w-5" />
            {t("watch.title")}
          </h1>
          <p className="text-sm text-muted">{t("watch.subtitle")}</p>
        </div>
        <Button onClick={() => onAsk()}>
          <MessageCircleQuestion className="mr-1 h-4 w-4" />
          {t("watch.askTeam")}
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-6">
        {assets.length === 0 ? (
          <div className="mx-auto mt-16 max-w-md text-center">
            <blockquote className="mb-6 font-display text-base leading-relaxed text-muted">
              「{quote.text}」
              <footer className="mt-1 text-xs text-faint">—— {quote.who}</footer>
            </blockquote>
            <p className="text-base font-medium">{t("watch.empty.title")}</p>
            <p className="mt-2 text-sm text-muted">{t("watch.empty.hint")}</p>
            <div className="mt-4 flex flex-col gap-2">
              {exampleQuestions.map((q) => (
                <Button key={q} variant="secondary" onClick={() => onAsk(q)}>
                  {q}
                </Button>
              ))}
            </div>
          </div>
        ) : (
          <div className="mx-auto flex max-w-3xl flex-col gap-6">
            {holdings.length > 0 &&
              section(
                t("watch.section.holding"),
                <>
                  {portfolio && <PortfolioStrip portfolio={portfolio} />}
                  {holdings.map((a) => (
                    <HoldingCard
                      key={a.symbol}
                      asset={a}
                      position={positionOf(a.symbol)}
                      quote={quotes.get(a.symbol)}
                      onAsk={onAsk}
                    />
                  ))}
                </>,
              )}
            {watching.length > 0 &&
              section(
                t("watch.section.watching"),
                watching.map((a) => (
                  <WatchCard
                    key={a.symbol}
                    asset={a}
                    quote={quotes.get(a.symbol)}
                    onUntrack={onUntrack}
                    onAsk={onAsk}
                  />
                )),
              )}
            {sold.length > 0 &&
              section(
                t("watch.section.sold"),
                sold.map((a) => (
                  <WatchCard
                    key={a.symbol}
                    asset={a}
                    quote={quotes.get(a.symbol)}
                    onUntrack={onUntrack}
                    onAsk={onAsk}
                  />
                )),
              )}
          </div>
        )}
      </div>

      <p className="border-t border-border px-4 py-2 text-center text-xs text-muted">
        {t("disclaimer.footer")}
      </p>
    </div>
  );
}
