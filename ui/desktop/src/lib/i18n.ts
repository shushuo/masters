/**
 * Minimal, dependency-free i18n for the investing-vertical surfaces (docs/11 decision g).
 *
 * A typed key dictionary with zh-CN + en variants. Locale resolves from a localStorage
 * override (`getmasters-locale`) falling back to the browser language; missing keys fall
 * back zh → en → the key itself. Only the *new* vertical surfaces use this — retrofitting
 * the existing English screens is out of scope for slice 1.
 */

const dict = {
  // --- shell / navigation (docs/12 §2: navigation is the user's nouns) ---
  "nav.ask": { zh: "问大师", en: "Ask" },
  "nav.settings": { zh: "设置", en: "Settings" },
  "sidebar.newTopic": { zh: "新话题", en: "New topic" },
  "sidebar.noTopics": { zh: "还没有话题。", en: "No topics yet." },
  "sidebar.deleteTopic": { zh: "删除话题", en: "Delete topic" },
  "sidebar.untitledTopic": { zh: "未命名话题", en: "Untitled topic" },
  "sidebar.guarding": { zh: "守护中 · 本地", en: "Guarding · local" },
  "sidebar.connecting": { zh: "连接中…", en: "connecting…" },
  "sidebar.collapse": { zh: "收起侧栏", en: "Collapse sidebar" },
  "sidebar.expand": { zh: "展开侧栏", en: "Expand sidebar" },
  // --- 问大师 (ask home) ---
  "ask.greeting": { zh: "今天想弄明白什么？", en: "What would you like to understand today?" },
  "ask.placeholder": {
    zh: "问点什么…（@某位大师 指名回答；Shift+Enter 换行）",
    en: "Ask something…  (@a master to address one · Shift+Enter for a new line)",
  },
  "ask.send": { zh: "发送", en: "Send" },
  "ask.rounds.auto": { zh: "轮次：自动", en: "Rounds: auto" },
  "ask.rounds.n": { zh: "轮", en: "round(s)" },
  "ask.loading": { zh: "正在唤醒守护…", en: "Waking the guardian…" },
  "ask.coordinatorHint": { zh: "不@任何人时由首席顾问回答", en: "Unaddressed questions go to the coordinator" },
  "ask.weekly": { zh: "本周市场三件事", en: "This week in the market" },
  "ask.error": { zh: "出错了：", en: "Something went wrong: " },
  // --- lab (advanced workbench) ---
  "lab.title": { zh: "高级工作台", en: "Advanced workbench" },
  "lab.chat": { zh: "通用对话", en: "General chat" },
  "lab.projects": { zh: "项目", en: "Projects" },
  "lab.masters": { zh: "大师管理", en: "Masters hub" },
  "lab.newChat": { zh: "新对话", en: "New chat" },
  "settings.openLab": { zh: "打开高级工作台", en: "Open the advanced workbench" },
  "settings.labHint": {
    zh: "通用对话、项目与大师管理等幕后能力",
    en: "General chat, projects, and master management",
  },
  "settings.language": { zh: "语言", en: "Language" },
  "settings.languageHint": {
    zh: "界面语言（切换后刷新生效）",
    en: "Interface language (reloads to apply)",
  },
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
  "watch.section.holding": { zh: "持有", en: "Holding" },
  "watch.section.watching": { zh: "关注中", en: "Watching" },
  "watch.section.sold": { zh: "已卖出", en: "Sold" },
  "watch.reasonPrefix": { zh: "你当时关心：", en: "You cared because: " },
  "watch.ask": { zh: "就此提问", en: "Ask about" },
  "watch.unvalued": { zh: "未估值", en: "unvalued" },
  "watch.qty": { zh: "数量", en: "Quantity" },
  "watch.cost": { zh: "成本", en: "Cost" },
  "watch.value": { zh: "市值", en: "Value" },
  "watch.weight": { zh: "权重", en: "Weight" },
  "briefings.today": { zh: "今天", en: "Today" },
  "briefings.yesterday": { zh: "昨天", en: "Yesterday" },
  "briefings.type.weekly": { zh: "周报", en: "Weekly" },
  "briefings.type.mover": { zh: "异动", en: "Mover" },
  "briefings.type.earnings": { zh: "财报", en: "Earnings" },
  "briefings.quiet.title": { zh: "最近没什么需要你操心的", en: "Nothing needs your attention" },
  "briefings.quiet.hint": {
    zh: "有值得说的才会出现在这里——安静，是守护在正常工作。",
    en: "Briefings appear only when something is worth saying — quiet means the guardian is doing its job.",
  },
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
  // 中文优先 (docs/12 §1.8): the product speaks Chinese by default; English is the explicit
  // opt-in via Settings (or an English-language browser).
  return typeof navigator !== "undefined" && navigator.language?.toLowerCase().startsWith("en")
    ? "en"
    : "zh";
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
