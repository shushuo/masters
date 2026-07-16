/**
 * Minimal, dependency-free i18n for the investing-vertical surfaces (docs/11 decision g).
 *
 * A typed key dictionary with zh-CN + en variants. Locale resolves from a localStorage
 * override (`getmasters-locale`) falling back to the browser language; missing keys fall
 * back zh → en → the key itself. Only the *new* vertical surfaces use this — retrofitting
 * the existing English screens is out of scope for slice 1.
 */

const dict = {
  "nav.watch": { zh: "关注", en: "Watch" },
  "watch.title": { zh: "关注", en: "Watching" },
  "watch.subtitle": {
    zh: "你问过的标的会出现在这里，我会替你盯着。",
    en: "Instruments you ask about land here; Masters keeps an eye on them.",
  },
  "watch.loading": { zh: "加载中…", en: "Loading…" },
  "watch.empty.title": { zh: "还没有关注任何标的", en: "Not watching anything yet" },
  "watch.empty.hint": {
    zh: "去和专家团聊聊，问过的标的会自动加入关注。试试：",
    en: "Ask the expert team about an instrument and it will be tracked automatically. Try:",
  },
  "watch.empty.q1": { zh: "沪深300指数基金怎么样？", en: "How is the CSI 300 index fund?" },
  "watch.empty.q2": { zh: "帮我看看贵州茅台的基本情况", en: "Give me the basics on Kweichow Moutai" },
  "watch.empty.q3": { zh: "最近有什么值得注意的市场变化？", en: "Anything notable in the market lately?" },
  "watch.watchedAt": { zh: "关注于", en: "Watching since" },
  "watch.dataAsOf": { zh: "数据截至", en: "Data as of" },
  "watch.noQuote": { zh: "暂无行情数据", en: "No quote data" },
  "watch.stale": { zh: "数据可能过期", en: "Quote may be outdated" },
  "watch.untrack": { zh: "移除关注", en: "Stop watching" },
  "watch.askTeam": { zh: "问专家团", en: "Ask the experts" },
  "watch.backToList": { zh: "返回关注列表", en: "Back to watch list" },
  "watch.teamTitle": { zh: "投资专家团", en: "Investing expert team" },
  "watch.error": { zh: "加载失败：", en: "Failed to load: " },
  "watch.portfolio.title": { zh: "组合概览", en: "Portfolio overview" },
  "watch.portfolio.total": { zh: "总市值", en: "Total value" },
  "watch.portfolio.hhi": { zh: "集中度 HHI", en: "Concentration (HHI)" },
  "watch.portfolio.top3": { zh: "前三大占比", en: "Top-3 share" },
  "watch.portfolio.unvalued": {
    zh: "笔持有暂无法估值（缺数量或行情）",
    en: "holding(s) unvalued (missing quantity or quote)",
  },
  "watch.stateWatching": { zh: "关注中", en: "watching" },
  "watch.stateHolding": { zh: "持有", en: "holding" },
  "watch.stateSold": { zh: "已卖出", en: "sold" },
  "disclaimer.footer": {
    zh: "ⓘ 以上为事实与风险梳理，不构成投资建议",
    en: "ⓘ Facts and risk notes only — not investment advice",
  },
  "nav.briefings": { zh: "简报", en: "Briefings" },
  "briefings.title": { zh: "简报", en: "Briefings" },
  "briefings.subtitle": {
    zh: "例行的周报与异动提醒会出现在这里；没有值得说的就不打扰你。",
    en: "Weekly digests and mover alerts land here; quiet weeks stay quiet.",
  },
  "briefings.loading": { zh: "加载中…", en: "Loading…" },
  "briefings.empty.title": { zh: "还没有简报", en: "No briefings yet" },
  "briefings.empty.hint": {
    zh: "关注一些标的后，每周日晚会有关注周报；工作日收盘后若有明显异动也会提醒。",
    en: "Watch some instruments — a weekly digest arrives Sunday evening, and post-close mover alerts fire only when something moved.",
  },
  "briefings.ask": { zh: "就此提问", en: "Ask about this" },
  "briefings.error": { zh: "加载失败：", en: "Failed to load: " },
} as const;

export type I18nKey = keyof typeof dict;
export type Locale = "zh" | "en";

const STORAGE_KEY = "getmasters-locale";

export function getLocale(): Locale {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "zh" || stored === "en") return stored;
  } catch {
    // storage unavailable (e.g. tests) — fall through to navigator
  }
  return typeof navigator !== "undefined" && navigator.language?.toLowerCase().startsWith("zh")
    ? "zh"
    : "en";
}

export function setLocale(locale: Locale): void {
  try {
    localStorage.setItem(STORAGE_KEY, locale);
  } catch {
    // best-effort
  }
}

/** Translate a key in the resolved locale (zh → en → key fallback). */
export function t(key: I18nKey): string {
  const entry = dict[key];
  if (!entry) return key;
  const locale = getLocale();
  return entry[locale] ?? entry.zh ?? key;
}

/** Hook-shaped accessor (stable identity today; reactive locale switching can come later). */
export function useT(): (key: I18nKey) => string {
  return t;
}
