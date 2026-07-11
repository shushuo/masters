import { useEffect, useRef, type KeyboardEvent, type ReactNode } from "react";
import { cn } from "./cn";
import { inputBase } from "./Input";

/**
 * A shared message composer: an auto-growing textarea (Enter sends, Shift+Enter inserts
 * a newline) with a trailing action slot for the Send/Stop button (and, in group chat,
 * the chips + rounds controls via `leading`/`trailing`). Replaces the single-line
 * `Input` in Chat and GroupChat so multi-line prompts and pasted code are first-class.
 */
export function Composer({
  value,
  onChange,
  onSubmit,
  placeholder,
  disabled,
  maxRows = 8,
  leading,
  trailing,
  className,
}: {
  value: string;
  onChange: (v: string) => void;
  onSubmit: () => void;
  placeholder?: string;
  disabled?: boolean;
  maxRows?: number;
  /** Rendered above the textarea row (e.g. group @-mention chips). */
  leading?: ReactNode;
  /** Rendered to the right of the textarea (Send/Stop, rounds select). */
  trailing?: ReactNode;
  className?: string;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);

  // Grow with content up to maxRows, then scroll.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    const line = parseFloat(getComputedStyle(el).lineHeight) || 20;
    const max = line * maxRows;
    el.style.height = `${Math.min(el.scrollHeight, max)}px`;
    el.style.overflowY = el.scrollHeight > max ? "auto" : "hidden";
  }, [value, maxRows]);

  function onKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      onSubmit();
    }
  }

  return (
    <div className={cn("border-t border-border bg-bg p-3", className)}>
      {leading}
      <div className="flex items-end gap-2">
        <textarea
          ref={ref}
          rows={1}
          className={cn(inputBase, "max-h-48 resize-none py-2 leading-relaxed")}
          placeholder={placeholder}
          value={value}
          disabled={disabled}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={onKeyDown}
        />
        {trailing}
      </div>
    </div>
  );
}
