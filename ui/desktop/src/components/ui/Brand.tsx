import { cn } from "./cn";

/**
 * The 《大师》 seal mark (docs/12 §5.5): a square seal-style block carrying the
 * single character 大 in the serif display face — accent ground, paper glyph.
 * Pure CSS/text (no asset), so it recolors with the theme tokens automatically.
 * Replaces the retired panda mark (D12: a mascot may return later; the wordmark
 * leads for now). Kept under the `PandaMark` export name so existing call sites
 * (Sidebar/Chat/Onboarding) need no churn.
 */
export function PandaMark({ className }: { className?: string }) {
  return (
    <span
      aria-hidden
      className={cn(
        // Callers size the glyph with a text-* class alongside the box size
        // (e.g. `size-6 text-2xl`); the default suits the sidebar mark.
        "inline-flex select-none items-center justify-center rounded-[6px]",
        "bg-accent font-display text-sm font-semibold leading-none text-accent-fg",
        className,
      )}
    >
      大
    </span>
  );
}

/** Brand lockup: the seal mark beside the 「大师」 wordmark (serif) + latin sublabel. */
export function Wordmark({
  size = "md",
  className,
}: {
  size?: "sm" | "md" | "lg";
  className?: string;
}) {
  const mark = size === "lg" ? "size-8 text-lg" : size === "sm" ? "size-5 text-xs" : "size-6 text-[15px]";
  const text = size === "lg" ? "text-2xl" : size === "sm" ? "text-sm" : "text-base";
  const sub = size === "lg" ? "text-xs" : "text-[10px]";
  return (
    <span className={cn("inline-flex items-baseline gap-2", className)}>
      <PandaMark className={cn("self-center", mark)} />
      <span className={cn("font-display font-semibold tracking-wide text-text", text)}>
        大师
      </span>
      <span className={cn("font-medium uppercase tracking-widest text-faint", sub)}>
        Masters
      </span>
    </span>
  );
}
