import { memo, useState, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Check, Copy } from "lucide-react";
import { copyText } from "../../lib/clipboard";
import { cn } from "./cn";

/** A fenced code block with a copy button. Streaming-safe — re-renders per delta. */
function CodeBlock({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  async function copy() {
    if (await copyText(text)) {
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    }
  }
  return (
    <div className="group relative my-2">
      <button
        onClick={copy}
        aria-label="Copy code"
        className="absolute right-1.5 top-1.5 rounded-sm border border-border bg-bg p-1 text-faint opacity-0 transition-opacity hover:text-text focus-visible:opacity-100 group-hover:opacity-100"
      >
        {copied ? <Check className="size-3.5 text-success" /> : <Copy className="size-3.5" />}
      </button>
      <pre className="overflow-x-auto rounded border border-border bg-surface-2 p-3 text-xs">
        <code className="font-mono">{text}</code>
      </pre>
    </div>
  );
}

/** Flatten react-markdown's `children` into a raw string for copy/inspection. */
function childText(children: ReactNode): string {
  if (typeof children === "string") return children;
  if (Array.isArray(children)) return children.map(childText).join("");
  if (children && typeof children === "object" && "props" in children) {
    return childText((children as { props: { children: ReactNode } }).props.children);
  }
  return "";
}

/**
 * Renders assistant/master message text as Markdown (GFM: tables, lists, task lists,
 * strikethrough). No raw HTML is allowed. Prose is styled inline via component
 * overrides — the app is dependency-light, so there's no typography plugin. Full
 * syntax highlighting is intentionally omitted (a monospace block + copy is enough
 * and keeps the bundle small); the code-renderer hook is the seam if it's wanted later.
 */
export const Markdown = memo(function Markdown({ text, className }: { text: string; className?: string }) {
  return (
    <div className={cn("space-y-2 text-sm leading-relaxed", className)}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          p: ({ children }) => <p className="whitespace-pre-wrap break-words">{children}</p>,
          a: ({ children, href }) => (
            <a href={href} target="_blank" rel="noreferrer" className="text-accent underline underline-offset-2">
              {children}
            </a>
          ),
          ul: ({ children }) => <ul className="ml-4 list-disc space-y-1">{children}</ul>,
          ol: ({ children }) => <ol className="ml-4 list-decimal space-y-1">{children}</ol>,
          h1: ({ children }) => <h1 className="text-base font-semibold">{children}</h1>,
          h2: ({ children }) => <h2 className="text-base font-semibold">{children}</h2>,
          h3: ({ children }) => <h3 className="text-sm font-semibold">{children}</h3>,
          blockquote: ({ children }) => (
            <blockquote className="border-l-2 border-border-strong pl-3 text-muted">{children}</blockquote>
          ),
          table: ({ children }) => (
            <div className="overflow-x-auto">
              <table className="w-full border-collapse text-xs">{children}</table>
            </div>
          ),
          th: ({ children }) => (
            <th className="border border-border px-2 py-1 text-left font-medium">{children}</th>
          ),
          td: ({ children }) => <td className="border border-border px-2 py-1">{children}</td>,
          code: ({ className: cls, children }) => {
            // Fenced blocks carry a `language-*` class; inline code does not.
            const isBlock = /language-/.test(cls ?? "");
            if (isBlock) return <CodeBlock text={childText(children).replace(/\n$/, "")} />;
            return (
              <code className="rounded bg-surface-2 px-1 py-0.5 font-mono text-[0.85em]">{children}</code>
            );
          },
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
});
