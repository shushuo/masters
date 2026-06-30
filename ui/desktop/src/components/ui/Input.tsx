import type { InputHTMLAttributes } from "react";
import { cn } from "./cn";

export const inputBase =
  "w-full rounded-sm border border-border-strong bg-bg px-3 py-1.5 text-sm text-text " +
  "placeholder:text-faint focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent";

export type InputProps = InputHTMLAttributes<HTMLInputElement>;

export function Input({ className, ...rest }: InputProps) {
  return <input className={cn(inputBase, className)} {...rest} />;
}
