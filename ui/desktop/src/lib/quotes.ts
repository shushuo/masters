/**
 * 「大师一句」 — a small curated pack of investing-discipline quotes for empty states
 * (docs/12 §3.1; the local forerunner of the D13 daily-heartbeat content pack, which
 * will later arrive via catalog sync). One quote per day, deterministic by date.
 *
 * Curation rules: discipline/temperament over prediction; no directional or
 * return-promising language (the compliance boundary applies to content we ship).
 */

export interface MasterQuote {
  text: string;
  who: string;
}

const QUOTES: MasterQuote[] = [
  { text: "别人贪婪时恐惧，别人恐惧时贪婪。", who: "沃伦·巴菲特" },
  { text: "投资的第一条准则是不要亏损；第二条是记住第一条。", who: "沃伦·巴菲特" },
  { text: "股市是把钱从没有耐心的人转移给有耐心的人的工具。", who: "沃伦·巴菲特" },
  { text: "价格是你付出的，价值是你得到的。", who: "沃伦·巴菲特" },
  { text: "如果你不愿意持有一只股票十年，就不要考虑持有它十分钟。", who: "沃伦·巴菲特" },
  { text: "投资者最大的敌人不是市场，而是自己。", who: "本杰明·格雷厄姆" },
  { text: "市场短期是投票机，长期是称重机。", who: "本杰明·格雷厄姆" },
  { text: "安全边际，是把犯错的余地留给自己。", who: "本杰明·格雷厄姆" },
  { text: "知道自己不知道什么，比聪明更重要。", who: "查理·芒格" },
  { text: "反过来想，总是反过来想。", who: "查理·芒格" },
  { text: "赚大钱靠的不是买卖，而是等待。", who: "查理·芒格" },
  { text: "能力圈的边界在哪里，比能力圈有多大更重要。", who: "查理·芒格" },
  { text: "不了解的东西，热闹也与你无关。", who: "彼得·林奇" },
  { text: "了解你持有的东西，并知道你为什么持有它。", who: "彼得·林奇" },
  { text: "在别人沮丧地抛售时买进，在别人疯狂抢购时卖出，需要最大的勇气。", who: "约翰·邓普顿" },
  { text: "行情总在绝望中诞生，在半信半疑中成长，在憧憬中成熟，在希望中毁灭。", who: "约翰·邓普顿" },
  { text: "风险来自你不知道自己在做什么。", who: "沃伦·巴菲特" },
  { text: "预测很难，尤其是预测未来。", who: "尼尔斯·玻尔" },
  { text: "复利是世界第八大奇迹——前提是别打断它。", who: "常被引为爱因斯坦" },
  { text: "最重要的不是你对了多少次，而是对的时候赚了多少、错的时候亏了多少。", who: "乔治·索罗斯" },
  { text: "投资应该像看着油漆变干或小草生长一样无聊。", who: "保罗·萨缪尔森" },
  { text: "时间是好生意的朋友，是坏生意的敌人。", who: "沃伦·巴菲特" },
];

/** Deterministic daily pick: the same quote all day, a new one tomorrow. */
export function dailyQuote(now: Date = new Date()): MasterQuote {
  const day = Math.floor(
    (now.getTime() - now.getTimezoneOffset() * 60_000) / 86_400_000,
  );
  return QUOTES[((day % QUOTES.length) + QUOTES.length) % QUOTES.length];
}
