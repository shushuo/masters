import { useEffect, useState } from "react";
import { MastersClient, type ConnectorDto } from "../api/client";
import { Button, Card, Input, Textarea } from "./ui";

/**
 * External MCP connectors (Phase 4d, FR-20; ADR-0005): add a third-party stdio MCP server to this
 * project. Its tools join the agent, gated + audited like the built-ins. The child runs with a
 * cleared environment plus only the env you configure here (credential stripping). Remote (SSE/HTTP)
 * transports are deferred.
 */
export function Connectors({ client, projectId }: { client: MastersClient; projectId: string }) {
  const [connectors, setConnectors] = useState<ConnectorDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  function refresh() {
    client.listConnectors(projectId).then(setConnectors).catch((e) => setError(String(e)));
  }
  useEffect(refresh, [client, projectId]);

  async function toggle(name: string, enabled: boolean) {
    try {
      await client.setConnectorEnabled(projectId, name, enabled);
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }
  async function remove(name: string) {
    try {
      await client.deleteConnector(projectId, name);
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="space-y-2 border-t pt-3">
      <p className="font-medium">External MCP servers</p>
      {error && <div className="text-xs text-danger">{error}</div>}
      {(connectors ?? []).map((c) => (
        <Card key={c.name} className="p-2">
          <div className="flex items-center justify-between">
            <div>
              <span className="font-medium text-text">{c.name}</span>
              <div className="font-mono text-xs text-muted">
                {c.command} {(c.args ?? []).join(" ")}
              </div>
            </div>
            <div className="flex items-center gap-2">
              <input
                type="checkbox"
                className="size-4 accent-accent"
                checked={c.enabled !== false}
                onChange={(e) => toggle(c.name, e.target.checked)}
              />
              <Button variant="ghost" size="sm" className="text-danger" onClick={() => remove(c.name)}>
                Delete
              </Button>
            </div>
          </div>
        </Card>
      ))}
      <AddForm client={client} projectId={projectId} onAdded={refresh} onError={setError} />
    </div>
  );
}

function AddForm({
  client,
  projectId,
  onAdded,
  onError,
}: {
  client: MastersClient;
  projectId: string;
  onAdded: () => void;
  onError: (e: string) => void;
}) {
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [argsText, setArgsText] = useState("");
  const [envText, setEnvText] = useState("");

  async function add() {
    if (!name || !command) return;
    try {
      const args = argsText.split(/\s+/).filter(Boolean);
      const env: [string, string][] = envText
        .split("\n")
        .map((line) => line.trim())
        .filter(Boolean)
        .map((line) => {
          const i = line.indexOf("=");
          return [line.slice(0, i), line.slice(i + 1)] as [string, string];
        })
        .filter((kv) => kv[0]);
      await client.createConnector(projectId, { name, command, args, env, enabled: true });
      setName("");
      setCommand("");
      setArgsText("");
      setEnvText("");
      onAdded();
    } catch (e) {
      onError(String(e));
    }
  }

  return (
    <Card className="space-y-2 p-2">
      <div className="flex gap-2">
        <Input
          className="w-32"
          placeholder="name"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <Input
          className="flex-1 font-mono"
          placeholder="command (e.g. npx)"
          value={command}
          onChange={(e) => setCommand(e.target.value)}
        />
      </div>
      <Input
        className="font-mono"
        placeholder="args (space-separated)"
        value={argsText}
        onChange={(e) => setArgsText(e.target.value)}
      />
      <Textarea
        className="font-mono"
        placeholder="env, one KEY=value per line (the only env the server sees)"
        rows={2}
        value={envText}
        onChange={(e) => setEnvText(e.target.value)}
      />
      <Button variant="primary" size="sm" disabled={!name || !command} onClick={add}>
        Add connector
      </Button>
    </Card>
  );
}
