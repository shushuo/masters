import type { HTMLAttributes } from "react";
import { cn } from "./cn";

export type CardProps = HTMLAttributes<HTMLDivElement> & {
  /** Adds hover affordance for clickable cards. */
  interactive?: boolean;
};

export function Card({ interactive, className, ...rest }: CardProps) {
  return (
    <div
      className={cn(
        "rounded border border-border bg-bg p-4",
        interactive && "cursor-pointer transition-colors hover:border-border-strong hover:bg-surface-2",
        className,
      )}
      {...rest}
    />
  );
}
