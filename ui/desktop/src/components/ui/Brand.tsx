import { cn } from "./cn";

/**
 * The Masters panda mark (assets/logo.svg → public/logo.svg). A cream-and-slate
 * panda head: the cream face keeps it legible on the dark canvas and the slate
 * outline keeps it legible on cream, so a single asset works in both themes.
 */
export function PandaMark({ className }: { className?: string }) {
  return (
    <img
      src="/logo.svg"
      alt=""
      aria-hidden
      draggable={false}
      className={cn("select-none object-contain", className)}
    />
  );
}

/** Brand lockup: the panda mark beside the wordmark in the display face. */
export function Wordmark({
  size = "md",
  className,
}: {
  size?: "sm" | "md" | "lg";
  className?: string;
}) {
  const mark = size === "lg" ? "size-8" : size === "sm" ? "size-5" : "size-6";
  const text = size === "lg" ? "text-2xl" : size === "sm" ? "text-sm" : "text-base";
  return (
    <span className={cn("inline-flex items-center gap-2", className)}>
      <PandaMark className={mark} />
      <span className={cn("font-semibold tracking-tight text-text", text)}>
        Masters
      </span>
    </span>
  );
}
