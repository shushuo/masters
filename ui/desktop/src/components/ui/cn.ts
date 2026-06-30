/** Minimal className joiner. Falsy parts are dropped; later parts win by source order. */
export function cn(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}
