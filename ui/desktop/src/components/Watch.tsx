// The Watch page (docs/11 §9.3) — the investing vertical's core asset surface: every
// instrument the user has shown interest in, with its first-interest snapshot and an honest
// latest quote (provenance + data-as-of; stale/missing states rendered, never hidden).
// D10: no "since watching ±%" here — hypothetical returns belong to coached reviews only.
import { useCallback, useEffect, useMemo, useState } from "react";
import { MessageCircleQuestion, Star, Trash2 } from "lucide-react";
import type {
  AssetDto,
  InvestingWorkspaceDto,
  MastersClient,
  PortfolioDto,
  QuoteDto,
} from "../api/client";
import { GroupChat } from "./GroupChat";
import { Badge, Button, Card, IconButton } from "./ui";
import { t } from "../lib/i18n";

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
      <span className="text-xs text-muted">
        {t("watch.dataAsOf")} {quote.trade_date} · {quote.source}
        {quote.stale ? ` · ⚠ ${t("watch.stale")}` : ""}
      </span>
    </span>
  );
}

function AssetCard({
  asset,
  quote,
  onUntrack,
}: {
  asset: AssetDto;
  quote: QuoteDto | undefined;
  onUntrack: (symbol: string) => void;
}) {
  return (
    <Card className="flex flex-col gap-2 p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="truncate font-medium">{asset.name}</span>
            <span className="text-xs text-muted">{asset.symbol}</span>
            <Badge>{stateLabel(asset.state)}</Badge>
          </div>
          {asset.watch_reason && (
            <p className="mt-1 truncate text-sm text-muted">{asset.watch_reason}</p>
          )}
        </div>
        {asset.state === "watching" && (
          <IconButton
            label={`${t("watch.untrack")} ${asset.name}`}
            onClick={() => onUntrack(asset.symbol)}
          >
            <Trash2 className="h-4 w-4" />
          </IconButton>
        )}
      </div>
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <span className="text-xs text-muted tabular-nums">
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

/** B5 portfolio unlock: a summary strip that appears once any holding is recorded.
 * All numbers come from FinCalc verbatim; unvalued positions are counted, never estimated. */
function PortfolioStrip({ portfolio }: { portfolio: PortfolioDto }) {
  const stat = (label: string, value: string) => (
    <div className="flex flex-col">
      <span className="text-xs text-muted">{label}</span>
      <span className="font-medium tabular-nums">{value}</span>
    </div>
  );
  return (
    <Card className="flex flex-wrap items-center gap-6 p-4">
      <span className="text-sm font-medium">{t("watch.portfolio.title")}</span>
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

export function Watch({ client }: { client: MastersClient }) {
  const [workspace, setWorkspace] = useState<InvestingWorkspaceDto | null>(null);
  const [assets, setAssets] = useState<AssetDto[] | null>(null);
  const [quotes, setQuotes] = useState<Map<string, QuoteDto>>(new Map());
  const [portfolio, setPortfolio] = useState<PortfolioDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [chatting, setChatting] = useState(false);

  const refresh = useCallback(
    async (ws: InvestingWorkspaceDto) => {
      const list = await client.listAssets(ws.project_id);
      setAssets(list);
      // The portfolio strip unlocks progressively — only once a holding is recorded.
      if (list.some((a) => a.state === "holding")) {
        try {
          setPortfolio(await client.getPortfolio(ws.project_id));
        } catch {
          setPortfolio(null);
        }
      } else {
        setPortfolio(null);
      }
      const watching = list.map((a) => a.symbol);
      if (watching.length > 0) {
        // Quotes degrade independently: a failure leaves cards in the explicit no-data state.
        try {
          const qs = await client.listQuotes(ws.project_id, watching);
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

  if (chatting) {
    return (
      <div className="flex h-full flex-col">
        <div className="min-h-0 flex-1">
          <GroupChat
            client={client}
            projectId={workspace.project_id}
            teamSlug={workspace.team_slug}
            title={t("watch.teamTitle")}
            backLabel={t("watch.backToList")}
            onClose={() => {
              setChatting(false);
              void refresh(workspace);
            }}
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
      <div className="flex items-center justify-between gap-3 border-b border-border px-6 py-4">
        <div>
          <h1 className="flex items-center gap-2 text-lg font-semibold">
            <Star className="h-5 w-5" />
            {t("watch.title")}
          </h1>
          <p className="text-sm text-muted">{t("watch.subtitle")}</p>
        </div>
        <Button onClick={() => setChatting(true)}>
          <MessageCircleQuestion className="mr-1 h-4 w-4" />
          {t("watch.askTeam")}
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-6">
        {assets.length === 0 ? (
          <div className="mx-auto mt-16 max-w-md text-center">
            <p className="text-base font-medium">{t("watch.empty.title")}</p>
            <p className="mt-2 text-sm text-muted">{t("watch.empty.hint")}</p>
            <div className="mt-4 flex flex-col gap-2">
              {exampleQuestions.map((q) => (
                <Button key={q} variant="secondary" onClick={() => setChatting(true)}>
                  {q}
                </Button>
              ))}
            </div>
          </div>
        ) : (
          <div className="mx-auto flex max-w-3xl flex-col gap-3">
            {portfolio && <PortfolioStrip portfolio={portfolio} />}
            {assets.map((a) => (
              <AssetCard
                key={a.symbol}
                asset={a}
                quote={quotes.get(a.symbol)}
                onUntrack={onUntrack}
              />
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
