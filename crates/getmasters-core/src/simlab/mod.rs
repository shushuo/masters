//! **Simulation Investment Lab** (模拟投资实验室) — pure engine math + decision parsing.
//!
//! Lean-core constraints (ADR-0015): this module owns only the deterministic, HTTP/LLM-free pieces —
//! parsing a master's target-weight decision, enforcing the simulation's constraints, valuing a
//! virtual portfolio, and rebalancing to targets at given close prices. The orchestration (running
//! masters, fetching quotes, persistence) lives in `getmasters-server::simlab`.
//!
//! Design (inspired by Alpha Arena + the RETuning paper): masters express each round as a **target
//! allocation** (percent of NAV per symbol, remainder = cash); the engine — never the LLM — does
//! the money-math (NFR-INV-1). It is a **forward-in-time paper simulation**: positions mark to the
//! latest close, so a symbol with no available quote is carried unchanged and reported unvalued —
//! never estimated.

use std::collections::BTreeMap;

use crate::market::normalize_symbol;

/// The sentinel participant slug for the fixed buy-and-hold benchmark line (no LLM run).
pub const BENCHMARK_SLUG: &str = "__benchmark__";

/// Constraints on a simulation (decoded from the `constraints` JSON column by the server).
#[derive(Clone, Debug)]
pub struct SimConstraints {
    /// Reject short (negative-weight) positions. Default: true.
    pub long_only: bool,
    /// Per-symbol cap as a fraction 0..1 (e.g. 0.4 = 40%). `None` = uncapped.
    pub max_weight: Option<f64>,
    /// Minimum cash weight as a fraction 0..1. `None`/0 = no floor.
    pub cash_floor: Option<f64>,
    /// Benchmark symbol for the fixed buy-and-hold comparison line.
    pub benchmark: Option<String>,
    /// Round-trip turnover fee in basis points (0 = frictionless).
    pub fee_bps: f64,
}

impl Default for SimConstraints {
    fn default() -> Self {
        Self {
            long_only: true,
            max_weight: None,
            cash_floor: None,
            benchmark: None,
            fee_bps: 0.0,
        }
    }
}

/// Is a key a "cash" line (dropped from targets — cash is the implied remainder)?
fn is_cash_key(key: &str) -> bool {
    let k = key.trim().to_ascii_lowercase();
    matches!(
        k.as_str(),
        "现金" | "現金" | "现金比例" | "cash" | "cash%" | "现金仓位" | "留存现金"
    )
}

/// Parse one `symbol: weight` line tolerantly. Returns `(symbol_or_cash, value)` where the symbol is
/// canonicalized; cash lines return `("", value)`. `None` if the line isn't a weight assignment.
fn parse_line(line: &str) -> Option<(String, f64)> {
    // Accept both half- and full-width colons; also `=`.
    let sep = line.find(['：', ':', '=', '\t'])?;
    let (raw_key, raw_val) = line.split_at(sep);
    let key = raw_key
        .trim()
        .trim_start_matches(['-', '*', '•', ' '])
        .trim();
    if key.is_empty() {
        return None;
    }
    // Strip the separator char (1..3 bytes for full-width) then normalize the value.
    let val_str: String = raw_val
        .trim_start_matches(['：', ':', '=', '\t', ' '])
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')
        .collect();
    let value: f64 = val_str.parse().ok()?;
    if is_cash_key(key) {
        return Some((String::new(), value));
    }
    let sym = normalize_symbol(key)?;
    Some((sym, value))
}

/// Parse all weight lines in a text block into a symbol→percent map (cash lines dropped). Returns
/// `(map, saw_any_line)` — `saw_any_line` distinguishes "explicit all-cash" from "no decision".
fn parse_block(block: &str) -> (BTreeMap<String, f64>, bool) {
    let mut map = BTreeMap::new();
    let mut saw = false;
    for line in block.lines() {
        if let Some((sym, val)) = parse_line(line) {
            saw = true;
            if !sym.is_empty() && val > 0.0 {
                *map.entry(sym).or_insert(0.0) += val;
            }
        }
    }
    (map, saw)
}

/// Extract fenced code blocks as `(info_string, body)` pairs.
fn fenced_blocks(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let t = line.trim_start();
        if let Some(info) = t.strip_prefix("```").or_else(|| t.strip_prefix("~~~")) {
            let mut body = String::new();
            for inner in lines.by_ref() {
                let it = inner.trim_start();
                if it.starts_with("```") || it.starts_with("~~~") {
                    break;
                }
                body.push_str(inner);
                body.push('\n');
            }
            out.push((info.trim().to_string(), body));
        }
    }
    out
}

/// Does a fenced-block info string mark it as a decision block?
fn is_decision_label(info: &str) -> bool {
    let i = info.to_ascii_lowercase();
    info.contains("目标配置")
        || info.contains("配置")
        || info.contains("决策")
        || i.contains("allocation")
        || i.contains("targets")
        || i.contains("target")
        || i.contains("decision")
        || i.contains("portfolio")
}

/// Parse a master's reply into raw target weights (percent of NAV). Tolerant of full/half-width
/// punctuation, `%`/`％` suffixes, and list bullets. Prefers a labelled `目标配置`/`allocation`
/// fenced block; falls back to any fenced block that yields weights, then to scanning the raw text.
/// Returns `None` only when no weight assignment could be found at all (→ the master holds).
pub fn parse_targets(text: &str) -> Option<BTreeMap<String, f64>> {
    let blocks = fenced_blocks(text);
    // 1. A labelled decision block wins (even if it parses to all-cash → sell everything).
    for (info, body) in &blocks {
        if is_decision_label(info) {
            let (map, saw) = parse_block(body);
            if saw {
                return Some(map);
            }
        }
    }
    // 2. Any fenced block that yields at least one symbol.
    for (_, body) in &blocks {
        let (map, saw) = parse_block(body);
        if saw && !map.is_empty() {
            return Some(map);
        }
    }
    // 3. Fallback: scan the whole reply for `symbol: pct` lines.
    let (map, _) = parse_block(text);
    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

/// Apply the simulation's constraints to raw target percentages. Drops out-of-universe symbols
/// (and, when `long_only`, negative weights), clamps each to `max_weight`, and scales the invested
/// total down so it never exceeds `100 - cash_floor%`. Returns `(clean_targets_pct, dropped)`.
pub fn enforce_constraints(
    raw: &BTreeMap<String, f64>,
    universe: &[String],
    c: &SimConstraints,
) -> (BTreeMap<String, f64>, Vec<String>) {
    let mut clean: BTreeMap<String, f64> = BTreeMap::new();
    let mut dropped: Vec<String> = Vec::new();
    let cap_pct = c.max_weight.map(|w| w * 100.0);
    for (sym, &w) in raw {
        if !universe.iter().any(|u| u == sym) {
            dropped.push(sym.clone());
            continue;
        }
        if c.long_only && w < 0.0 {
            dropped.push(sym.clone());
            continue;
        }
        let w = cap_pct.map(|cap| w.min(cap)).unwrap_or(w);
        if w != 0.0 {
            clean.insert(sym.clone(), w);
        }
    }
    let invest_cap = 100.0 - c.cash_floor.unwrap_or(0.0) * 100.0;
    let sum: f64 = clean.values().sum();
    if sum > invest_cap && sum > 0.0 {
        let scale = invest_cap / sum;
        for w in clean.values_mut() {
            *w *= scale;
        }
    }
    (clean, dropped)
}

/// Value a virtual portfolio at the given close prices. Returns `(nav, unvalued_count)` where NAV =
/// cash + Σ(qty × close) over priced holdings; a held symbol with no price is counted unvalued and
/// contributes nothing (honest, never estimated).
pub fn portfolio_nav(
    cash: f64,
    positions: &BTreeMap<String, f64>,
    prices: &BTreeMap<String, f64>,
) -> (f64, usize) {
    let mut nav = cash;
    let mut unvalued = 0usize;
    for (sym, qty) in positions {
        match prices.get(sym) {
            Some(p) => nav += qty * p,
            None => {
                if *qty > 0.0 {
                    unvalued += 1;
                }
            }
        }
    }
    (nav, unvalued)
}

/// The result of a rebalance: the new position book (symbol, quantity, execution price) and cash.
#[derive(Clone, Debug)]
pub struct Rebalanced {
    /// `(symbol, quantity, avg_cost)` for the non-zero holdings after rebalancing.
    pub positions: Vec<(String, f64, Option<f64>)>,
    pub cash: f64,
    /// Held symbols carried unchanged because no price was available to trade them.
    pub unvalued_count: usize,
}

/// Rebalance a virtual portfolio to `targets` (percent of NAV) at `prices`. Symbols with a price are
/// moved to their target value (fractional shares); held symbols without a price are carried
/// unchanged (can't trade). A turnover fee (`fee_bps`) is charged on the traded notional. Pure.
pub fn rebalance(
    current: &BTreeMap<String, f64>,
    nav: f64,
    targets: &BTreeMap<String, f64>,
    prices: &BTreeMap<String, f64>,
    fee_bps: f64,
) -> Rebalanced {
    let mut priced: Vec<(String, f64, f64)> = Vec::new(); // (symbol, new_qty, price)
    let mut carried: Vec<(String, f64, Option<f64>)> = Vec::new();
    let mut spent = 0.0;
    let mut turnover = 0.0;
    let mut unvalued = 0usize;

    // Every symbol that is either a (priced) target or a currently-held symbol.
    let mut symbols: BTreeMap<String, ()> = BTreeMap::new();
    for s in targets.keys().chain(current.keys()) {
        symbols.insert(s.clone(), ());
    }

    for sym in symbols.keys() {
        let old_qty = current.get(sym).copied().unwrap_or(0.0);
        match prices.get(sym) {
            Some(&price) if price > 0.0 => {
                let target_pct = targets.get(sym).copied().unwrap_or(0.0).max(0.0);
                let new_qty = (target_pct / 100.0 * nav) / price;
                turnover += (new_qty - old_qty).abs() * price;
                spent += new_qty * price;
                priced.push((sym.clone(), new_qty, price));
            }
            _ => {
                // No price: can't trade. Carry the existing holding untouched.
                if old_qty > 0.0 {
                    unvalued += 1;
                    carried.push((sym.clone(), old_qty, None));
                }
            }
        }
    }

    // The fee is a real leak: if the targets are near-fully invested there's no cash to pay it
    // from, so scale the whole priced book down to fit `nav - fee`, conserving post-NAV = nav - fee.
    let fee = turnover * fee_bps / 10_000.0;
    if spent + fee > nav && spent > 0.0 {
        let scale = ((nav - fee).max(0.0)) / spent;
        spent = 0.0;
        for (_, qty, price) in priced.iter_mut() {
            *qty *= scale;
            spent += *qty * *price;
        }
    }
    let cash = (nav - spent - fee).max(0.0);

    let mut positions: Vec<(String, f64, Option<f64>)> = carried;
    for (sym, qty, price) in priced {
        if qty > 0.0 {
            positions.push((sym, qty, Some(price)));
        }
    }
    Rebalanced {
        positions,
        cash,
        unvalued_count: unvalued,
    }
}

/// Cumulative return of a NAV against the starting cash, as a fraction (0.1 = +10%).
pub fn return_pct(nav: f64, starting_cash: f64) -> Option<f64> {
    if starting_cash > 0.0 {
        Some((nav - starting_cash) / starting_cash)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs.iter().map(|(s, v)| (s.to_string(), *v)).collect()
    }

    #[test]
    fn parses_labelled_block() {
        let text = "分析框架……\n\n```目标配置\nsh600519: 30\nsz000001：20%\n现金: 50\n```\n";
        let t = parse_targets(text).unwrap();
        assert_eq!(t.get("sh600519"), Some(&30.0));
        assert_eq!(t.get("sz000001"), Some(&20.0));
        assert!(!t.contains_key("现金"), "cash line dropped");
    }

    #[test]
    fn tolerates_bare_codes_and_percent_signs() {
        // No fence, bullets, full-width colon, trailing percent — still parses.
        let text = "决策：\n- 600519：40%\n- 000001: 10%\n";
        let t = parse_targets(text).unwrap();
        assert_eq!(t.get("sh600519"), Some(&40.0));
        assert_eq!(t.get("sz000001"), Some(&10.0));
    }

    #[test]
    fn no_decision_returns_none() {
        assert!(parse_targets("我认为市场充满不确定性，本轮无明确操作建议。").is_none());
    }

    #[test]
    fn enforce_drops_out_of_universe_and_caps() {
        let raw = map(&[("sh600519", 60.0), ("sz000001", 30.0), ("sh600000", 10.0)]);
        let universe = vec!["sh600519".to_string(), "sz000001".to_string()];
        let c = SimConstraints {
            max_weight: Some(0.4),
            ..Default::default()
        };
        let (clean, dropped) = enforce_constraints(&raw, &universe, &c);
        assert_eq!(dropped, vec!["sh600000".to_string()]);
        assert_eq!(clean.get("sh600519"), Some(&40.0), "capped to 40%");
        assert_eq!(clean.get("sz000001"), Some(&30.0));
    }

    #[test]
    fn enforce_scales_to_cash_floor() {
        let raw = map(&[("sh600519", 70.0), ("sz000001", 50.0)]); // 120% invested
        let universe = vec!["sh600519".to_string(), "sz000001".to_string()];
        let c = SimConstraints {
            cash_floor: Some(0.1), // invest ≤ 90%
            ..Default::default()
        };
        let (clean, _) = enforce_constraints(&raw, &universe, &c);
        let sum: f64 = clean.values().sum();
        assert!(
            (sum - 90.0).abs() < 1e-9,
            "scaled to 90% invested, got {sum}"
        );
    }

    #[test]
    fn rebalance_hits_targets_and_conserves_value() {
        // NAV 100k, all cash. Target 50% of one 10-yuan stock → 5000 shares, 50k cash left.
        let current = BTreeMap::new();
        let targets = map(&[("sh600519", 50.0)]);
        let prices = map(&[("sh600519", 10.0)]);
        let r = rebalance(&current, 100_000.0, &targets, &prices, 0.0);
        assert_eq!(r.positions.len(), 1);
        assert_eq!(r.positions[0], ("sh600519".to_string(), 5000.0, Some(10.0)));
        assert!((r.cash - 50_000.0).abs() < 1e-6);
        // Post-NAV conserved (no fee): cash + shares*price = 100k.
        let post: BTreeMap<String, f64> = r
            .positions
            .iter()
            .map(|(s, q, _)| (s.clone(), *q))
            .collect();
        let (nav, _) = portfolio_nav(r.cash, &post, &prices);
        assert!((nav - 100_000.0).abs() < 1e-6);
    }

    #[test]
    fn rebalance_carries_unpriced_holdings() {
        // Hold 100 shares of an unpriced symbol; target moves cash into a priced one.
        let current = map(&[("sh600519", 100.0), ("sz000001", 200.0)]);
        let targets = map(&[("sh600519", 100.0)]);
        let prices = map(&[("sh600519", 10.0)]); // sz000001 has no price
                                                 // NAV = cash(0) + 100*10 (sz000001 unpriced → 0) = 1000.
        let (nav, unvalued) = portfolio_nav(0.0, &current, &prices);
        assert_eq!(unvalued, 1);
        let r = rebalance(&current, nav, &targets, &prices, 0.0);
        // The unpriced holding is carried untouched; the priced one is rebalanced to 100% of NAV.
        assert!(r
            .positions
            .iter()
            .any(|(s, q, _)| s == "sz000001" && *q == 200.0));
        assert_eq!(r.unvalued_count, 1);
    }

    #[test]
    fn fee_leaks_only_turnover() {
        let current = BTreeMap::new();
        let targets = map(&[("sh600519", 100.0)]);
        let prices = map(&[("sh600519", 10.0)]);
        // 10 bps on 100k turnover = 100 fee.
        let r = rebalance(&current, 100_000.0, &targets, &prices, 10.0);
        let post: BTreeMap<String, f64> = r
            .positions
            .iter()
            .map(|(s, q, _)| (s.clone(), *q))
            .collect();
        let (nav, _) = portfolio_nav(r.cash, &post, &prices);
        assert!(
            (nav - (100_000.0 - 100.0)).abs() < 1.0,
            "post-nav ~ 99,900, got {nav}"
        );
    }
}
