/**
 * The expert roster's identity table (docs/12 §5.4): slug → Chinese display name +
 * a fixed identity color. The roster is a trust asset — the same face appears in
 * bubbles, chips, and empty states. Colors are hand-picked from the 《大师》 palette
 * family (low saturation, readable as white-on-color at avatar size in both themes).
 */

export interface MasterIdentity {
  /** Chinese display name (the slug stays the address, e.g. `@chief`). */
  name: string;
  /** English display name (en locale). */
  nameEn: string;
  /** Identity color (avatar ground). */
  color: string;
}

export const MASTER_IDENTITIES: Record<string, MasterIdentity> = {
  chief: { name: "首席顾问", nameEn: "Chief Advisor", color: "#c2593f" },
  analyst: { name: "研究员", nameEn: "Analyst", color: "#8c6239" },
  risk: { name: "风控官", nameEn: "Risk Officer", color: "#7a4a58" },
  allocation: { name: "配置规划师", nameEn: "Allocation Planner", color: "#9a7b2f" },
  coach: { name: "投资教练", nameEn: "Investing Coach", color: "#4a5d7a" },
};

const FALLBACK_COLOR = "#6b7075";

/** The simulation-lab benchmark line (a fixed buy-and-hold, not a master). */
export const BENCHMARK_SLUG = "__benchmark__";

/** Display name for a slug (falls back to the slug itself for unknown masters). */
export function masterName(slug: string, locale: "zh" | "en" = "zh"): string {
  if (slug === BENCHMARK_SLUG) return locale === "en" ? "Benchmark" : "基准";
  const m = MASTER_IDENTITIES[slug];
  if (!m) return slug;
  return locale === "en" ? m.nameEn : m.name;
}

/** Identity color for a slug. */
export function masterColor(slug: string): string {
  return MASTER_IDENTITIES[slug]?.color ?? FALLBACK_COLOR;
}

/** Avatar glyph: the first character of the Chinese name (or the slug's first letter). */
export function masterGlyph(slug: string): string {
  if (slug === BENCHMARK_SLUG) return "基";
  const m = MASTER_IDENTITIES[slug];
  return m ? m.name.slice(0, 1) : (slug.slice(0, 1) || "?").toUpperCase();
}
