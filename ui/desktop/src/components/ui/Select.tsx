import type { SelectHTMLAttributes } from "react";
import { ChevronDown } from "lucide-react";
import { cn } from "./cn";
import { inputBase } from "./Input";

export type SelectProps = SelectHTMLAttributes<HTMLSelectElement>;

export function Select({ className, children, ...rest }: SelectProps) {
  return (
    <div className="relative">
      <select className={cn(inputBase, "cursor-pointer appearance-none pr-9", className)} {...rest}>
        {children}
      </select>
      <ChevronDown
        className="pointer-events-none absolute right-2.5 top-1/2 size-4 -translate-y-1/2 text-faint"
        aria-hidden
      />
    </div>
  );
}
