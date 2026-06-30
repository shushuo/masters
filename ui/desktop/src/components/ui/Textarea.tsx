import type { TextareaHTMLAttributes } from "react";
import { cn } from "./cn";
import { inputBase } from "./Input";

export type TextareaProps = TextareaHTMLAttributes<HTMLTextAreaElement>;

export function Textarea({ className, ...rest }: TextareaProps) {
  return <textarea className={cn(inputBase, "resize-y", className)} {...rest} />;
}
