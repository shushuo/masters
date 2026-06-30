import type { ReactNode } from "react";
import { cn } from "./cn";

type Variant = "neutral" | "accent" | "tool" | "success" | "warning" | "danger";

export type BadgeProps = {
  variant?: Variant;
  children: ReactNode;
  className?: string;
};

const variants: Record<Variant, string> = {
  neutral: "bg-surface-2 text-muted",
  accent: "bg-accent-subtle text-accent",
  tool: "bg-tool-bg text-tool-fg",
  success: "bg-success/10 text-success",
  warning: "bg-tool-bg text-tool-fg",
  danger: "bg-danger-bg text-danger-fg",
};

export function Badge({ variant = "neutral", children, className }: BadgeProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-sm px-1.5 py-0.5 text-xs font-medium",
        variants[variant],
        className,
      )}
    >
      {children}
    </span>
  );
}
