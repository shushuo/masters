import { useCallback, useEffect, useRef, useState } from "react";

/**
 * Keeps a scroll container pinned to the bottom as content streams in, but yields
 * to the user the moment they scroll up (so reading back through a long transcript
 * isn't yanked away). Exposes `atBottom` so callers can show a "jump to latest"
 * affordance, and `scrollToBottom` to act on it.
 *
 * Usage:
 *   const { ref, atBottom, scrollToBottom } = useStickToBottom([turns]);
 *   <div ref={ref} className="overflow-y-auto">…</div>
 */
export function useStickToBottom(deps: unknown[]) {
  const ref = useRef<HTMLDivElement>(null);
  const [atBottom, setAtBottom] = useState(true);
  // Ref mirror so the content effect reads the latest value without re-subscribing.
  const atBottomRef = useRef(true);

  const scrollToBottom = useCallback((behavior: ScrollBehavior = "auto") => {
    const el = ref.current;
    if (el) el.scrollTo({ top: el.scrollHeight, behavior });
  }, []);

  // Track whether the user is near the bottom (24px slack absorbs sub-pixel rounding).
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onScroll = () => {
      const near = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
      atBottomRef.current = near;
      setAtBottom(near);
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, []);

  // Follow new content only while the user hasn't scrolled away.
  useEffect(() => {
    if (atBottomRef.current) scrollToBottom("auto");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  return { ref, atBottom, scrollToBottom };
}
