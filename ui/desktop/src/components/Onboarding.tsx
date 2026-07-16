import { useEffect, useMemo, useState } from "react";
import { MastersClient, type ProviderStateDto } from "../api/client";
import { Button, Input, PandaMark, Select } from "./ui";

/** Sensible default model per provider, used to prefill the field when the provider changes. */
const DEFAULT_MODELS: Record<string, string> = {
  anthropic: "claude-opus-4-8",
  openai: "gpt-4o",
  deepseek: "deepseek-chat",
  gemini: "gemini-2.0-flash",
  ollama: "llama3",
  dashscope: "qwen-plus",
  glm: "glm-4-plus",
  moonshot: "moonshot-v1-8k",
  minimax: "MiniMax-Text-01",
};

/**
 * Setup wizard (the getmasters analogue of `hermes setup`), reached from Settings ("Re-run setup").
 * Step 1 configures a provider + model + API key (stored in the OS keychain); Step 2 optionally
 * creates a first project and grants it a working folder so the agent has somewhere to act. Both
 * steps reuse the same endpoints as Settings/Projects. The user can cancel to keep the current
 * configuration. (The daemon requires a usable provider to run — there is no offline mode.)
 */
export function Onboarding({ client, onDone }: { client: MastersClient; onDone: () => void }) {
  const [step, setStep] = useState<1 | 2>(1);
  const [catalog, setCatalog] = useState<ProviderStateDto[]>([]);
  const [provider, setProvider] = useState("anthropic");
  const [model, setModel] = useState("claude-opus-4-8");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [projectName, setProjectName] = useState("");
  const [folder, setFolder] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    client.getProviders().then((d) => setCatalog(d.providers)).catch(() => {});
  }, [client]);

  const entry = useMemo(() => catalog.find((p) => p.id === provider), [catalog, provider]);
  const isLocal = entry?.is_local ?? false;
  const isOpenAiCompatible = entry?.transport === "openai_compatible";

  function selectProvider(id: string) {
    setProvider(id);
    setModel(DEFAULT_MODELS[id] ?? "");
    setBaseUrl("");
  }

  async function saveProvider() {
    setBusy(true);
    setStatus("Saving…");
    try {
      await client.updateSettings({
        provider,
        model,
        provider_bases: baseUrl.trim() ? { [provider]: baseUrl.trim() } : undefined,
      });
      if (apiKey) await client.setSecret(`${provider}_api_key`, apiKey);
      setApiKey("");
      setStatus(null);
      setStep(2);
    } catch (e) {
      setStatus(`Error: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  }

  async function createWorkspace() {
    setBusy(true);
    setStatus("Creating workspace…");
    try {
      const project = await client.createProject(projectName || "My Workspace");
      if (folder.trim()) {
        await client.addGrant(project.id, folder.trim(), "read_write");
      }
      finish();
    } catch (e) {
      setStatus(`Error: ${String(e)}`);
      setBusy(false);
    }
  }

  function finish() {
    // Provider/model changes apply on the next daemon launch; for this session we just proceed.
    onDone();
  }

  return (
    <div className="flex h-full items-center justify-center p-4">
      <div className="w-full max-w-md rounded-lg border border-border bg-surface p-6 text-sm shadow">
        <div className="mb-4 flex flex-col items-center text-center">
          <PandaMark className="size-14 text-3xl" />
          <h2 className="mt-3 font-display text-xl font-semibold">欢迎来到「大师」</h2>
        </div>
        {step === 1 && (
          /* The three promises (docs/12 §3.4) — the product's posture, stated up front. */
          <ul className="mx-auto mb-4 max-w-sm space-y-1.5 text-[13px] text-muted">
            <li>① 数字必有来源，并注明「数据截至」——绝不心算、绝不编造。</li>
            <li>② 你的持仓与画像只存在你自己的电脑上。</li>
            <li>③ 不荐股、不预测、不代操作——结论永远由你自己得出。</li>
          </ul>
        )}
        <p className="text-center text-muted">
          {step === 1
            ? "先连接一个模型服务商。密钥保存在系统钥匙串中，不落盘。"
            : "可选：创建工作区并授予「大师」一个可读写的文件夹。"}
        </p>

        {step === 1 ? (
          <div className="mt-4 space-y-3">
            <label className="block space-y-1">
              <span className="text-muted">Provider</span>
              <Select value={provider} onChange={(e) => selectProvider(e.target.value)}>
                {catalog.length === 0 ? (
                  <option value="anthropic">Anthropic (Claude)</option>
                ) : (
                  catalog.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.label}
                    </option>
                  ))
                )}
              </Select>
            </label>

            <label className="block space-y-1">
              <span className="text-muted">Model</span>
              <Input value={model} onChange={(e) => setModel(e.target.value)} />
            </label>

            {isOpenAiCompatible && (
              <label className="block space-y-1">
                <span className="text-muted">
                  Base URL{entry?.custom ? "" : " (optional override)"}
                </span>
                <Input
                  placeholder={entry?.default_base ?? "https://… (base URL)"}
                  value={baseUrl}
                  onChange={(e) => setBaseUrl(e.target.value)}
                />
              </label>
            )}

            {isLocal ? (
              <p className="text-xs text-faint">
                Local endpoint — no API key needed. Make sure it is running.
              </p>
            ) : (
              <label className="block space-y-1">
                <span className="text-muted">API key</span>
                <Input
                  type="password"
                  placeholder={provider === "anthropic" ? "sk-ant-…" : "sk-…"}
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
                />
              </label>
            )}

            <div className="flex items-center justify-between pt-2">
              <Button variant="ghost" onClick={onDone} disabled={busy}>
                Cancel
              </Button>
              <Button
                variant="primary"
                onClick={saveProvider}
                disabled={busy || (!apiKey && !isLocal)}
              >
                Continue
              </Button>
            </div>
          </div>
        ) : (
          <div className="mt-4 space-y-3">
            <label className="block space-y-1">
              <span className="text-muted">Workspace name</span>
              <Input
                placeholder="My Workspace"
                value={projectName}
                onChange={(e) => setProjectName(e.target.value)}
              />
            </label>

            <label className="block space-y-1">
              <span className="text-muted">Working folder (read + write)</span>
              <Input
                placeholder="/Users/you/Documents/getmasters"
                value={folder}
                onChange={(e) => setFolder(e.target.value)}
              />
              <span className="mt-1 block text-xs text-faint">
                Masters only ever touches folders you grant. You can change this later in Projects.
              </span>
            </label>

            <div className="flex items-center justify-between pt-2">
              <Button variant="ghost" onClick={finish} disabled={busy}>
                Skip
              </Button>
              <Button variant="primary" onClick={createWorkspace} disabled={busy}>
                Finish
              </Button>
            </div>
          </div>
        )}

        {status && <p className="mt-3 text-muted">{status}</p>}
      </div>
    </div>
  );
}
