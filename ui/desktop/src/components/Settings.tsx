import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import {
  ExternalLink,
  Info,
  KeyRound,
  Mail,
  Search,
  Server,
  Sparkles,
  X,
  type LucideIcon,
} from "lucide-react";
import {
  MastersClient,
  type ConfigCheckDto,
  type EmailSettingsDto,
  type EnvironmentDto,
  type ProvidersDto,
  type SettingsDto,
} from "../api/client";
import { Badge, type BadgeProps, Button, IconButton, Input, Select } from "./ui";
import { cn } from "./ui/cn";
import { checkForUpdate, installUpdate } from "../lib/updater";

type SectionKey = "model" | "keys" | "environment" | "email" | "about";

/** Left-rail categories, grouped exactly like the desktop settings rail (dividers between groups). */
const NAV_GROUPS: { key: SectionKey; label: string; icon: LucideIcon }[][] = [
  [
    { key: "model", label: "Model", icon: Sparkles },
    { key: "keys", label: "Providers", icon: KeyRound },
  ],
  [
    { key: "environment", label: "Environment", icon: Server },
    { key: "email", label: "Email", icon: Mail },
  ],
  [{ key: "about", label: "About", icon: Info }],
];

/** A single setting: label + description on the left, its control on the right. */
function SettingRow({
  title,
  description,
  badge,
  children,
  full,
}: {
  title: string;
  description?: string;
  badge?: ReactNode;
  children?: ReactNode;
  full?: boolean;
}) {
  return (
    <div
      className={cn(
        "border-b border-border py-5",
        full ? "" : "grid grid-cols-[1fr_minmax(0,16rem)] items-start gap-8",
      )}
    >
      <div className="max-w-md">
        <div className="flex items-center gap-2 font-semibold text-text">
          {title}
          {badge}
        </div>
        {description && <p className="mt-1 text-[13px] leading-snug text-muted">{description}</p>}
      </div>
      {children && <div className={full ? "mt-4" : ""}>{children}</div>}
    </div>
  );
}

/** Provider/model/key settings. Keys are write-only — the API reports only their presence. */
export function Settings({
  client,
  onClose,
  onRerunSetup,
}: {
  client: MastersClient;
  onClose: () => void;
  onRerunSetup: () => void;
}) {
  const [settings, setSettings] = useState<SettingsDto | null>(null);
  const [model, setModel] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [section, setSection] = useState<SectionKey>("model");
  const [search, setSearch] = useState("");

  useEffect(() => {
    client.getSettings().then((s) => {
      setSettings(s);
      setModel(s.model);
    });
  }, [client]);

  async function save() {
    setStatus("Saving…");
    try {
      await client.updateSettings({ model });
      setSettings(await client.getSettings());
      setStatus("Saved. Model changes apply on the next daemon launch.");
    } catch (e) {
      setStatus(`Error: ${String(e)}`);
    }
  }

  // Every setting, tagged with its section + search keywords, rendered as a row. Search filters
  // across all sections; otherwise the selected section's rows show.
  const rows = useMemo(() => {
    if (!settings) return [];
    return [
      {
        section: "model" as const,
        title: "Default Model",
        keywords: "model default",
        node: (
          <SettingRow
            title="Default Model"
            description="Used for new chats unless a master pins its own model. Prefix with a provider to route it, e.g. deepseek:deepseek-chat."
          >
            <Input value={model} onChange={(e) => setModel(e.target.value)} />
          </SettingRow>
        ),
      },
      {
        section: "keys" as const,
        title: "Providers",
        keywords:
          "provider anthropic openai deepseek gemini openrouter ollama dashscope qwen glm zhipu moonshot kimi minimax custom api key secret base url endpoint default",
        node: (
          <SettingRow
            title="Providers"
            description="Configure each LLM provider with its own API key + optional base URL, and pick the active default. Keys live in the OS keychain — only their presence is reported back."
            full
          >
            <ProvidersPanel client={client} />
          </SettingRow>
        ),
      },
      {
        section: "environment" as const,
        title: "Environment",
        keywords: "data home database provider source config check setup wizard env override",
        node: (
          <SettingRow
            title="Environment"
            description="Resolved data home, effective configuration, and a live provider check."
            full
          >
            <EnvironmentPanel client={client} onRerunSetup={onRerunSetup} />
          </SettingRow>
        ),
      },
      {
        section: "email" as const,
        title: "Email delivery",
        keywords: "smtp email delivery routine output notification",
        node: (
          <SettingRow
            title="Email delivery"
            description="SMTP for routine output. Off by default; each routine still opts in."
            full
          >
            <EmailDelivery client={client} />
          </SettingRow>
        ),
      },
      {
        section: "about" as const,
        title: "About Masters",
        keywords: "about version local first agentic",
        node: (
          <SettingRow title="About Masters" full>
            <p className="max-w-prose text-[13px] leading-relaxed text-muted">
              Masters is a local-first, single-user agentic desktop app for personal study and
              work — an agent that acts on your local files with human-in-the-loop approval. All
              state lives on this device under your data home.
            </p>
          </SettingRow>
        ),
      },
    ];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings, model, client, onRerunSetup]);

  if (!settings) return <div className="p-6 text-sm text-muted">Loading settings…</div>;

  const q = search.trim().toLowerCase();
  const visible = q
    ? rows.filter((r) => `${r.title} ${r.keywords} ${r.section}`.toLowerCase().includes(q))
    : rows.filter((r) => r.section === section);
  const showSave = visible.some((r) => r.section === "model");

  return (
    <div className="flex h-full">
      {/* Category rail */}
      <nav className="flex w-52 shrink-0 flex-col gap-0.5 overflow-y-auto border-r border-border px-2 py-3">
        {NAV_GROUPS.map((group, gi) => (
          <div key={gi} className="flex flex-col gap-0.5">
            {gi > 0 && <div className="my-2 border-t border-border" />}
            {group.map(({ key, label, icon: Icon }) => {
              const active = !q && section === key;
              return (
                <button
                  key={key}
                  onClick={() => {
                    setSection(key);
                    setSearch("");
                  }}
                  aria-current={active ? "page" : undefined}
                  className={cn(
                    "flex items-center gap-2.5 rounded-sm px-2.5 py-1.5 text-sm transition-colors",
                    active
                      ? "bg-accent-subtle font-medium text-accent"
                      : "text-muted hover:bg-surface-2 hover:text-text",
                  )}
                >
                  <Icon className="size-4 shrink-0" aria-hidden />
                  <span>{label}</span>
                </button>
              );
            })}
          </div>
        ))}
      </nav>

      {/* Content */}
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex items-center gap-3 px-6 py-4">
          <div className="relative flex-1">
            <Search
              className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-faint"
              aria-hidden
            />
            <Input
              className="pl-9"
              placeholder="Search settings…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
          <IconButton label="Close settings" onClick={onClose}>
            <X className="size-4" />
          </IconButton>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 pb-8 text-sm">
          {visible.length === 0 ? (
            <p className="py-8 text-muted">No settings match “{search}”.</p>
          ) : (
            visible.map((r) => <div key={`${r.section}-${r.title}`}>{r.node}</div>)
          )}

          {showSave && (
            <div className="flex items-center gap-3 pt-5">
              <Button variant="primary" onClick={save}>
                Save changes
              </Button>
              {status && <p className="text-[13px] text-muted">{status}</p>}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * The configurable provider catalog (ADR-0013). Lists every provider with its own API-key field +
 * optional base-URL override, and the active-default selector. Keys are write-only (OS keychain);
 * only their presence (`key_set`) is reported back. Changes apply on the next daemon launch.
 */
function ProvidersPanel({ client }: { client: MastersClient }) {
  const [data, setData] = useState<ProvidersDto | null>(null);
  const [active, setActive] = useState("");
  const [keys, setKeys] = useState<Record<string, string>>({});
  const [bases, setBases] = useState<Record<string, string>>({});
  const [status, setStatus] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    setError(null);
    client
      .getProviders()
      .then((d) => {
        setData(d);
        setActive(d.active);
        const b: Record<string, string> = {};
        for (const p of d.providers) b[p.id] = p.base_url ?? "";
        setBases(b);
        setKeys({});
      })
      .catch((e) => setError(String(e)));
  }, [client]);
  useEffect(() => load(), [load]);

  async function save() {
    if (!data) return;
    setSaving(true);
    setStatus("Saving…");
    try {
      for (const [id, val] of Object.entries(keys)) {
        if (val.trim()) await client.setSecret(`${id}_api_key`, val.trim());
      }
      // Send only base URLs that changed ("" clears an override on the backend).
      const provider_bases: Record<string, string> = {};
      for (const p of data.providers) {
        const next = (bases[p.id] ?? "").trim();
        if (next !== (p.base_url ?? "")) provider_bases[p.id] = next;
      }
      await client.updateSettings({
        provider: active,
        provider_bases: Object.keys(provider_bases).length ? provider_bases : undefined,
      });
      setStatus("Saved. Changes apply on the next daemon launch.");
      load();
    } catch (e) {
      setStatus(`Error: ${String(e)}`);
    } finally {
      setSaving(false);
    }
  }

  if (error) {
    return (
      <div className="rounded-lg border border-border p-4 text-sm">
        <p className="text-danger-fg">Couldn’t load providers: {error}</p>
        <p className="mt-1 text-xs text-muted">
          The daemon may be an older build without the provider catalog — restart it (re-run{" "}
          <code className="font-mono">make dev</code>), then retry.
        </p>
        <Button variant="secondary" size="sm" className="mt-3" onClick={load}>
          Retry
        </Button>
      </div>
    );
  }

  if (!data) return <p className="text-sm text-muted">Loading providers…</p>;

  return (
    <div className="space-y-4">
      <fieldset className="rounded-lg border border-border p-4">
        <legend className="px-1 text-muted">Active provider</legend>
        <p className="mb-2 text-xs text-muted">
          The default backend for new chats (experts may pin their own).
        </p>
        <Select value={active} onChange={(e) => setActive(e.target.value)}>
          {data.providers.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
              {p.key_set || p.is_local ? "" : " — no key"}
            </option>
          ))}
        </Select>
      </fieldset>

      <div className="divide-y divide-border rounded-lg border border-border">
        {data.providers.map((p) => (
          <div key={p.id} className="p-4">
            <div className="flex items-center gap-2">
              <span className="font-semibold text-text">{p.label}</span>
              {p.id === active && <Badge variant="accent">active</Badge>}
              {p.is_local ? (
                <Badge variant="neutral">local</Badge>
              ) : p.key_set ? (
                <Badge variant="success">key set</Badge>
              ) : (
                <Badge variant="neutral">no key</Badge>
              )}
              {p.docs_url && (
                <a
                  href={p.docs_url}
                  target="_blank"
                  rel="noreferrer"
                  className="ml-auto inline-flex items-center gap-1 text-xs text-muted hover:text-accent"
                >
                  API keys <ExternalLink className="size-3" aria-hidden />
                </a>
              )}
            </div>

            <div className="mt-2 grid gap-2 sm:grid-cols-2">
              {p.is_local ? (
                <p className="text-xs text-muted sm:col-span-1">
                  Local endpoint — no API key needed.
                </p>
              ) : (
                <Input
                  type="password"
                  placeholder={p.key_set ? "Key ✓ set — enter to replace" : "API key"}
                  value={keys[p.id] ?? ""}
                  onChange={(e) => setKeys({ ...keys, [p.id]: e.target.value })}
                />
              )}
              {p.transport === "openai_compatible" && (
                <Input
                  placeholder={p.default_base ?? "https://… (base URL)"}
                  value={bases[p.id] ?? ""}
                  onChange={(e) => setBases({ ...bases, [p.id]: e.target.value })}
                />
              )}
            </div>
          </div>
        ))}
      </div>

      <div className="flex items-center gap-3">
        <Button variant="primary" onClick={save} disabled={saving}>
          {saving ? "Saving…" : "Save providers"}
        </Button>
        {status && <p className="text-[13px] text-muted">{status}</p>}
      </div>
    </div>
  );
}

/** Badge variant for a config-source label (`settings` | `env` | `default`). */
function sourceVariant(source: string): BadgeProps["variant"] {
  return source === "settings" ? "accent" : source === "env" ? "warning" : "neutral";
}

/** Badge variant for a config-check status (`ok` | `warn` | `error`). */
function statusVariant(status: string): BadgeProps["variant"] {
  return status === "ok" ? "success" : status === "warn" ? "warning" : "danger";
}

/**
 * Environment & config-init panel — the `hermes config` / `config check` analogue. Surfaces the
 * resolved data home, effective-vs-configured provider, where each value comes from, and active
 * env-var overrides; runs a live provider test call; and re-launches the first-run setup wizard.
 */
function EnvironmentPanel({
  client,
  onRerunSetup,
}: {
  client: MastersClient;
  onRerunSetup: () => void;
}) {
  const [env, setEnv] = useState<EnvironmentDto | null>(null);
  const [check, setCheck] = useState<ConfigCheckDto | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [telemetry, setTelemetry] = useState<boolean | null>(null);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);
  const [updating, setUpdating] = useState(false);

  useEffect(() => {
    client.getEnvironment().then(setEnv).catch((e) => setError(String(e)));
    client
      .getSettings()
      .then((s) => setTelemetry(s.telemetry_enabled ?? true))
      .catch(() => setTelemetry(null));
  }, [client]);

  async function toggleTelemetry(enabled: boolean) {
    setTelemetry(enabled);
    try {
      await client.updateSettings({ telemetry_enabled: enabled });
    } catch (e) {
      setError(`Error: ${String(e)}`);
    }
  }

  async function checkUpdates() {
    setUpdating(true);
    setUpdateStatus("Checking…");
    try {
      const update = await checkForUpdate();
      if (!update) {
        setUpdateStatus("You're on the latest version.");
        return;
      }
      setUpdateStatus(`Updating to v${update.version}…`);
      await installUpdate(update); // downloads, installs, and relaunches
    } catch (e) {
      setUpdateStatus(`Update failed: ${String(e)}`);
    } finally {
      setUpdating(false);
    }
  }

  async function runCheck() {
    setChecking(true);
    setError(null);
    try {
      setCheck(await client.checkConfig());
    } catch (e) {
      setError(`Error: ${String(e)}`);
    } finally {
      setChecking(false);
    }
  }

  if (!env) return null;

  const mismatch = env.configured_provider !== env.effective_provider;

  return (
    <fieldset className="rounded-lg border border-border p-4">
      <legend className="px-1 text-muted">Environment</legend>
      <dl className="grid grid-cols-[7rem_1fr] gap-x-3 gap-y-1.5 text-xs">
        <dt className="text-muted">Data home</dt>
        <dd className="break-all font-mono text-text">{env.data_home}</dd>
        <dt className="text-muted">Database</dt>
        <dd className="break-all font-mono text-text">{env.db_path}</dd>
        <dt className="text-muted">Provider</dt>
        <dd className="flex flex-wrap items-center gap-2">
          <span className="text-text">{env.effective_provider}</span>
          {mismatch && <Badge variant="warning">configured: {env.configured_provider}</Badge>}
          <Badge variant={sourceVariant(env.provider_source)}>{env.provider_source}</Badge>
        </dd>
        <dt className="text-muted">Model</dt>
        <dd className="flex flex-wrap items-center gap-2">
          <span className="text-text">{env.model}</span>
          <Badge variant={sourceVariant(env.model_source)}>{env.model_source}</Badge>
        </dd>
        <dt className="text-muted">Base URL</dt>
        <dd className="flex flex-wrap items-center gap-2">
          <span className="text-text">{env.openai_base_url ?? "default"}</span>
          <Badge variant={sourceVariant(env.base_url_source)}>{env.base_url_source}</Badge>
        </dd>
        <dt className="text-muted">Keys</dt>
        <dd className="flex flex-wrap items-center gap-2">
          <Badge variant={env.anthropic_key_set ? "success" : "neutral"}>
            anthropic {env.anthropic_key_set ? "set" : "not set"}
          </Badge>
          <Badge variant={env.openai_key_set ? "success" : "neutral"}>
            openai {env.openai_key_set ? "set" : "not set"}
          </Badge>
        </dd>
      </dl>

      {env.env_overrides.length > 0 && (
        <div className="mt-3">
          <span className="text-xs text-muted">Env overrides</span>
          <div className="mt-1 flex flex-wrap gap-1">
            {env.env_overrides.map((name) => (
              <Badge key={name} variant="tool">
                {name}
              </Badge>
            ))}
          </div>
        </div>
      )}

      {telemetry !== null && (
        <label className="mt-3 flex items-start gap-2 text-xs text-muted">
          <input
            type="checkbox"
            className="mt-0.5"
            checked={telemetry}
            onChange={(e) => toggleTelemetry(e.target.checked)}
          />
          <span>
            <span className="text-text">Anonymous install telemetry</span> — reports a single random
            install id + platform/version once, so installs can be counted. No file or personal data
            is sent. Uncheck to opt out.
          </span>
        </label>
      )}

      <div className="mt-3 flex flex-wrap items-center gap-2">
        <Button variant="secondary" size="sm" onClick={runCheck} disabled={checking}>
          {checking ? "Checking…" : "Check configuration"}
        </Button>
        <Button variant="ghost" size="sm" onClick={onRerunSetup}>
          Re-run setup wizard
        </Button>
        <Button variant="ghost" size="sm" onClick={checkUpdates} disabled={updating}>
          {updating ? "Checking…" : "Check for updates"}
        </Button>
      </div>
      {updateStatus && <p className="mt-2 text-xs text-muted">{updateStatus}</p>}

      {check && (
        <ul className="mt-3 space-y-1">
          {check.checks.map((c, i) => (
            <li key={`${c.name}-${i}`} className="flex items-start gap-2 text-xs">
              <Badge variant={statusVariant(c.status)}>{c.status}</Badge>
              <span className="text-muted">
                <span className="text-text">{c.name}</span> — {c.detail}
              </span>
            </li>
          ))}
        </ul>
      )}
      {error && <p className="mt-2 text-xs text-danger-fg">{error}</p>}
    </fieldset>
  );
}

/**
 * Outbound email delivery (Phase 3e, FR-27): SMTP config for routine output. Off by default; each
 * routine still opts in per-schedule (Routines tab). The password is write-only (OS keychain).
 */
function EmailDelivery({ client }: { client: MastersClient }) {
  const [email, setEmail] = useState<EmailSettingsDto | null>(null);
  const [host, setHost] = useState("");
  const [port, setPort] = useState("587");
  const [username, setUsername] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [enabled, setEnabled] = useState(false);
  const [password, setPassword] = useState("");
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    client.getEmailSettings().then((e) => {
      setEmail(e);
      setHost(e.host ?? "");
      setPort(e.port ? String(e.port) : "587");
      setUsername(e.username ?? "");
      setFrom(e.from ?? "");
      setTo(e.to ?? "");
      setEnabled(e.enabled);
    });
  }, [client]);

  async function save() {
    setStatus("Saving…");
    try {
      await client.updateEmailSettings({
        enabled,
        host: host || null,
        port: port ? Number(port) : null,
        username: username || null,
        from: from || null,
        to: to || null,
      });
      if (password) await client.setSecret("smtp_password", password);
      setPassword("");
      setEmail(await client.getEmailSettings());
      setStatus("Saved.");
    } catch (e) {
      setStatus(`Error: ${String(e)}`);
    }
  }

  if (!email) return null;

  return (
    <fieldset className="rounded-lg border border-border p-4">
      <legend className="px-1 text-muted">Email delivery (routine output)</legend>
      <label className="flex cursor-pointer items-center gap-2">
        <input
          type="checkbox"
          className="size-4 accent-accent"
          checked={enabled}
          onChange={(e) => setEnabled(e.target.checked)}
        />
        <span className="text-muted">Enable email delivery</span>
      </label>
      <div className="mt-2 grid grid-cols-2 gap-2">
        <Input placeholder="SMTP host" value={host} onChange={(e) => setHost(e.target.value)} />
        <Input placeholder="Port (587)" value={port} onChange={(e) => setPort(e.target.value)} />
        <Input
          placeholder="SMTP username"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
        />
        <Input
          type="password"
          placeholder={email.password_set ? "Password ✓ set" : "SMTP password"}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <Input
          placeholder="From address"
          value={from}
          onChange={(e) => setFrom(e.target.value)}
        />
        <Input
          placeholder="Deliver to address"
          value={to}
          onChange={(e) => setTo(e.target.value)}
        />
      </div>
      <Button variant="primary" size="sm" className="mt-2" onClick={save}>
        Save email
      </Button>
      {status && <p className="mt-1 text-xs text-muted">{status}</p>}
    </fieldset>
  );
}
