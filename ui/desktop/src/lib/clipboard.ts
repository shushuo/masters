// Minimal clipboard helper. Uses the async Clipboard API (available in the Tauri
// webview and modern browsers); resolves to whether the copy succeeded so callers
// can flash a "Copied" affordance without try/catch noise.
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
