import type { ButtonHTMLAttributes } from "react";
import { cn } from "./cn";

export type IconButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  /** Used as the accessible label (aria-label) and tooltip. */
  label: string;
  variant?: "ghost" | "secondary";
};

const base =
  "inline-flex items-center justify-center rounded-sm p-1.5 transition-colors " +
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent " +
  "disabled:opacity-50 disabled:pointer-events-none";

const variants = {
  ghost: "text-muted hover:bg-surface-2 hover:text-text",
  secondary: "border border-border-strong bg-bg text-text hover:bg-surface-2",
};

export function IconButton({ label, variant = "ghost", className, ...rest }: IconButtonProps) {
  return (
    <button
      aria-label={label}
      title={rest.title ?? label}
      className={cn(base, variants[variant], className)}
      {...rest}
    />
  );
}
