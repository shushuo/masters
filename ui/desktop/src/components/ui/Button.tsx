import type { ButtonHTMLAttributes } from "react";
import { cn } from "./cn";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "sm" | "md";

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  size?: Size;
};

const base =
  "inline-flex items-center justify-center gap-2 rounded-sm font-medium transition-colors " +
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-1 " +
  "disabled:opacity-50 disabled:pointer-events-none";

const sizes: Record<Size, string> = {
  sm: "px-2.5 py-1 text-xs",
  md: "px-3.5 py-2 text-sm",
};

const variants: Record<Variant, string> = {
  primary: "bg-accent text-accent-fg hover:bg-accent-hover",
  secondary: "border border-border-strong bg-bg text-text hover:bg-surface-2",
  ghost: "text-muted hover:bg-surface-2 hover:text-text",
  danger: "bg-danger text-white hover:bg-danger-hover",
};

export function Button({ variant = "primary", size = "md", className, ...rest }: ButtonProps) {
  return <button className={cn(base, sizes[size], variants[variant], className)} {...rest} />;
}
