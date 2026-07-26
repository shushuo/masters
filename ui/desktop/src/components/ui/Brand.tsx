import { cn } from "./cn";

/**
 * The 沉见 / DeepAnchor brand mark: the cairn (stacked-stones) logo — a coral
 * capstone over espresso stones (public/logo.svg, transparent), rendered as a
 * rounded app-icon tile. Kept under the `PandaMark` export name (an internal
 * codename) so existing call sites (Sidebar/Chat/Onboarding) need no churn.
 */
export function PandaMark({ className }: { className?: string }) {
  // The SVG asset is transparent (no background). We render it on a faint cream
  // chip: in light mode the chip blends into the paper canvas (reads as "no
  // background"), while in dark mode it keeps the espresso stones legible instead
  // of dissolving into the dark ground.
  return (
    <span
      aria-hidden
      className={cn(
        "inline-flex select-none items-center justify-center overflow-hidden rounded-[5px] bg-[#f4eee2]",
        className,
      )}
    >
      <img src="/logo.svg" alt="" draggable={false} className="h-full w-full object-contain p-[7%]" />
    </span>
  );
}

/** Brand lockup: the cairn mark beside the 「沉见」 wordmark + latin sublabel. */
export function Wordmark({
  size = "md",
  className,
}: {
  size?: "sm" | "md" | "lg";
  className?: string;
}) {
  const mark = size === "lg" ? "size-8" : size === "sm" ? "size-5" : "size-6";
  const text = size === "lg" ? "text-2xl" : size === "sm" ? "text-sm" : "text-base";
  const sub = size === "lg" ? "text-xs" : "text-[10px]";
  return (
    <span className={cn("inline-flex items-center gap-2", className)}>
      <PandaMark className={mark} />
      <span className={cn("font-display font-semibold text-text", text)}>
        沉见
      </span>
      <span className={cn("font-medium uppercase tracking-widest text-faint", sub)}>
        DeepAnchor
      </span>
    </span>
  );
}
