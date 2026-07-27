import { cn } from "./cn";

/**
 * The 沉见 / DeepAnchor brand mark: the cairn (stacked-stones) logo — a coral
 * capstone over espresso stones (public/logo.svg, transparent), rendered as a
 * rounded app-icon tile. Kept under the `PandaMark` export name (an internal
 * codename) so existing call sites (Sidebar/Chat/Onboarding) need no churn.
 */
export function PandaMark({ className }: { className?: string }) {
  // logo.svg is a self-contained warm-paper tile (its own cream ground), so the
  // mark stays legible on any surface in both themes. We only round the corners.
  return (
    <img
      src="/logo.svg"
      alt=""
      aria-hidden
      draggable={false}
      className={cn("select-none rounded-[22%] object-contain", className)}
    />
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
