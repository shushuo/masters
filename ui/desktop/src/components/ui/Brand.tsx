import { cn } from "./cn";

/**
 * The 《大师》 brand mark: the cairn (stacked-stones) logo — a coral capstone over
 * dark stones on cream (public/logo.svg), rendered as a rounded app-icon tile.
 * Replaces the retired panda mark (D12). Kept under the `PandaMark` export name so
 * existing call sites (Sidebar/Chat/Onboarding) need no churn.
 */
export function PandaMark({ className }: { className?: string }) {
  return (
    <img
      src="/logo.svg"
      alt=""
      aria-hidden
      draggable={false}
      className={cn("select-none rounded-[5px] object-contain", className)}
    />
  );
}

/** Brand lockup: the cairn mark beside the 「大师」 wordmark + latin sublabel. */
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
        大师
      </span>
      <span className={cn("font-medium uppercase tracking-widest text-faint", sub)}>
        Masters
      </span>
    </span>
  );
}
