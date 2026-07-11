import { useState } from "react";
import { ChevronRight, CircleAlert, Loader2, Wrench } from "lucide-react";
import { cn } from "./cn";

/** A single tool step: the CALL and (once it arrives) its RESULT, correlated by `id`. */
export interface ToolStepData {
  id: string;
  tool: string;
  /** Human summary from the call event (often includes a path / preview). */
  callSummary: string;
  /** Populated when the matching tool_result arrives. */
  result?: { summary: string; isError: boolean };
}

/**
 * One collapsed-by-default card pairing a tool call with its result (replaces the old
 * two-separate-strips rendering). Header = status icon + tool + summary; expanding shows
 * the result detail. Built on the `tool` palette so agent activity reads consistently in
 * both single chat and group chat.
 */
export function ToolStep({ step }: { step: ToolStepData }) {
  const [open, setOpen] = useState(false);
  const pending = !step.result;
  const isError = step.result?.isError ?? false;

  return (
    <div
      className={cn(
        "rounded border text-xs",
        isError ? "border-danger/40 bg-danger-bg" : "border-tool-border bg-tool-bg",
      )}
    >
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left"
        aria-expanded={open}
      >
        <ChevronRight
          className={cn("size-3 shrink-0 text-faint transition-transform", open && "rotate-90")}
          aria-hidden
        />
        {pending ? (
          <Loader2 className="size-3.5 shrink-0 animate-spin text-tool-fg" aria-hidden />
        ) : isError ? (
          <CircleAlert className="size-3.5 shrink-0 text-danger-fg" aria-hidden />
        ) : (
          <Wrench className="size-3.5 shrink-0 text-tool-fg" aria-hidden />
        )}
        <span className={cn("font-mono", isError ? "text-danger-fg" : "text-tool-fg")}>{step.tool}</span>
        <span className="truncate text-muted">{step.callSummary}</span>
      </button>
      {open && (
        <div className="border-t border-tool-border/60 px-2.5 py-1.5 font-mono text-[11px]">
          <div className="whitespace-pre-wrap break-words text-muted">{step.callSummary}</div>
          {step.result && (
            <div
              className={cn(
                "mt-1 whitespace-pre-wrap break-words",
                isError ? "text-danger-fg" : "text-text",
              )}
            >
              {step.result.summary || (isError ? "error" : "done")}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
