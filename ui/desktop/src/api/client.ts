// Thin typed client over the getmastersd loopback API.
//
// DTO types come from `schema.ts`, which is generated from the daemon's OpenAPI
// (`pnpm gen-api`) — the single source of truth for the contract (docs/02 §4).

import type { components } from "./schema";

export type SessionDto = components["schemas"]["SessionDto"];
export type MessageDto = components["schemas"]["MessageDto"];
export type AuditEntryDto = components["schemas"]["AuditEntryDto"];
export type HealthDto = components["schemas"]["HealthDto"];
export type ServerEvent = components["schemas"]["ServerEvent"];
export type SettingsDto = components["schemas"]["SettingsDto"];
export type SettingsUpdate = components["schemas"]["SettingsUpdate"];
export type ProvidersDto = components["schemas"]["ProvidersDto"];
export type ProviderStateDto = components["schemas"]["ProviderStateDto"];
export type EnvironmentDto = components["schemas"]["EnvironmentDto"];
export type ConfigCheckDto = components["schemas"]["ConfigCheckDto"];
export type ConfigCheckItem = components["schemas"]["ConfigCheckItem"];
export type RevertResult = components["schemas"]["RevertResult"];
export type MemoryDto = components["schemas"]["MemoryDto"];
export type SkillDto = components["schemas"]["SkillDto"];
export type DeckDto = components["schemas"]["DeckDto"];
export type StudyPlanDto = components["schemas"]["StudyPlanDto"];
export type RecipeDto = components["schemas"]["RecipeDto"];
export type RecipeSummaryDto = components["schemas"]["RecipeSummaryDto"];
export type RecipeRunResult = components["schemas"]["RecipeRunResult"];
export type MasterDto = components["schemas"]["MasterDto"];
export type MasterSummaryDto = components["schemas"]["MasterSummaryDto"];
export type MasterRunResult = components["schemas"]["MasterRunResult"];
export type DefaultMasterDto = components["schemas"]["DefaultMasterDto"];
export type AvailableHarnessDto = components["schemas"]["AvailableHarnessDto"];
export type CatalogStatusDto = components["schemas"]["CatalogStatusDto"];
export type TeamDto = components["schemas"]["TeamDto"];
export type TeamSummaryDto = components["schemas"]["TeamSummaryDto"];
export type TeamBundle = components["schemas"]["TeamBundle"];
export type BundleImportResult = components["schemas"]["BundleImportResult"];
export type CreateTeamRequest = components["schemas"]["CreateTeamRequest"];
export type RouteResultDto = components["schemas"]["RouteResultDto"];
export type TeamRunResult = components["schemas"]["TeamRunResult"];
export type GroupPostResult = components["schemas"]["GroupPostResult"];
export type ConnectorDto = components["schemas"]["ConnectorDto"];
export type CreateConnectorRequest = components["schemas"]["CreateConnectorRequest"];
export type ScheduleDto = components["schemas"]["ScheduleDto"];
export type CreateScheduleRequest = components["schemas"]["CreateScheduleRequest"];
export type SetScheduleRequest = components["schemas"]["SetScheduleRequest"];
export type ScheduledRunDto = components["schemas"]["ScheduledRunDto"];
export type EmailSettingsDto = components["schemas"]["EmailSettingsDto"];
export type EmailSettingsUpdate = components["schemas"]["EmailSettingsUpdate"];
export type ProjectDto = components["schemas"]["ProjectDto"];
export type ExtensionDto = components["schemas"]["ExtensionDto"];
export type KnowledgeStatusDto = components["schemas"]["KnowledgeStatusDto"];
export type AssetDto = components["schemas"]["AssetDto"];
export type QuoteDto = components["schemas"]["QuoteDto"];
export type InvestingWorkspaceDto = components["schemas"]["InvestingWorkspaceDto"];

/** Connection details handed over by the daemon handshake (`GETMASTERSD_READY`). */
export interface DaemonConn {
  port: number;
  token: string;
}

/** A before/after preview of a proposed file write, shown in the approval bar. */
export type FilePreview = components["schemas"]["FilePreview"];

/** A pending approval surfaced during a run. */
export interface PendingApproval {
  requestId: string;
  tool: string;
  summary: string;
  classes: string[];
  /** Present for write-class tools: a diff preview of the proposed change. */
  preview?: FilePreview | null;
}

/** Callbacks for a streaming run. */
export interface StreamHandlers {
  onStart?: () => void;
  onDelta: (text: string) => void;
  onToolCall?: (id: string, tool: string, summary: string) => void;
  onToolResult?: (id: string, summary: string, isError: boolean) => void;
  onApproval?: (approval: PendingApproval) => void;
  onComplete: (messageId: string) => void;
  onError: (message: string) => void;
  // Multi-master group streaming (Phase 4e/4f). `round` is 0 for the user's turn, then increments
  // for each mention-driven follow-up round.
  onGroupStart?: (round: number, addressed: string[]) => void;
  onMasterDelta?: (round: number, author: string, text: string) => void;
  onMasterComplete?: (round: number, author: string, messageId: string) => void;
  onMasterError?: (round: number, author: string, message: string) => void;
  onMasterToolCall?: (round: number, author: string, id: string, tool: string, summary: string) => void;
  onMasterToolResult?: (
    round: number,
    author: string,
    id: string,
    summary: string,
    isError: boolean,
  ) => void;
  onGroupComplete?: () => void;
}

export class MastersClient {
  constructor(private readonly conn: DaemonConn) {}

  private base(): string {
    return `http://127.0.0.1:${this.conn.port}`;
  }

  private headers(): HeadersInit {
    return {
      "content-type": "application/json",
      authorization: `Bearer ${this.conn.token}`,
    };
  }

  async health(): Promise<HealthDto> {
    const res = await fetch(`${this.base()}/health`);
    if (!res.ok) throw new Error(`health failed: ${res.status}`);
    return res.json();
  }

  async createSession(title?: string): Promise<SessionDto> {
    const res = await fetch(`${this.base()}/sessions`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({ title }),
    });
    if (!res.ok) throw new Error(`createSession failed: ${res.status}`);
    return res.json();
  }

  /** All chat sessions, newest first (for the chat history switcher). */
  async listSessions(): Promise<SessionDto[]> {
    const res = await fetch(`${this.base()}/sessions`, { headers: this.headers() });
    if (!res.ok) throw new Error(`listSessions failed: ${res.status}`);
    return res.json();
  }

  /** Delete a chat session (and its messages/events). */
  async deleteSession(sessionId: string): Promise<void> {
    const res = await fetch(`${this.base()}/sessions/${sessionId}`, {
      method: "DELETE",
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`deleteSession failed: ${res.status}`);
  }

  async listMessages(sessionId: string): Promise<MessageDto[]> {
    const res = await fetch(`${this.base()}/sessions/${sessionId}/messages`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`listMessages failed: ${res.status}`);
    return res.json();
  }

  /** A session's gated tool-call audit trail, oldest first (docs/06). */
  async listAudit(sessionId: string): Promise<AuditEntryDto[]> {
    const res = await fetch(`${this.base()}/sessions/${sessionId}/audit`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`listAudit failed: ${res.status}`);
    return res.json();
  }

  /** Revert the last file operation in a session. */
  async revert(sessionId: string): Promise<RevertResult> {
    const res = await fetch(`${this.base()}/sessions/${sessionId}/revert`, {
      method: "POST",
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`revert failed: ${res.status}`);
    return res.json();
  }

  /** All projects (context containers; ADR-0011). */
  async listProjects(): Promise<ProjectDto[]> {
    const res = await fetch(`${this.base()}/projects`, { headers: this.headers() });
    if (!res.ok) throw new Error(`listProjects failed: ${res.status}`);
    return res.json();
  }

  async getProject(id: string): Promise<ProjectDto> {
    const res = await fetch(`${this.base()}/projects/${id}`, { headers: this.headers() });
    if (!res.ok) throw new Error(`getProject failed: ${res.status}`);
    return res.json();
  }

  async createProject(name: string, instructions?: string): Promise<ProjectDto> {
    const res = await fetch(`${this.base()}/projects`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({ name, instructions }),
    });
    if (!res.ok) throw new Error(`createProject failed: ${res.status}`);
    return res.json();
  }

  /** Grant a folder to a project. `access` is `"read"` | `"read_write"`. */
  async addGrant(projectId: string, path: string, access: string): Promise<void> {
    const res = await fetch(`${this.base()}/projects/${projectId}/grants`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({ path, access }),
    });
    if (!res.ok) throw new Error(`addGrant failed: ${res.status}`);
  }

  async setInstructions(projectId: string, instructions: string): Promise<ProjectDto> {
    const res = await fetch(`${this.base()}/projects/${projectId}/instructions`, {
      method: "PUT",
      headers: this.headers(),
      body: JSON.stringify({ instructions }),
    });
    if (!res.ok) throw new Error(`setInstructions failed: ${res.status}`);
    return res.json();
  }

  /** A project's knowledge-index status + indexed documents (read-only; ADR-0004). */
  async getKnowledgeStatus(projectId: string): Promise<KnowledgeStatusDto> {
    const res = await fetch(`${this.base()}/projects/${projectId}/knowledge`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`getKnowledgeStatus failed: ${res.status}`);
    return res.json();
  }

  /** A project's built-in servers + enabled state (FR-19). */
  async getExtensions(projectId: string): Promise<ExtensionDto[]> {
    const res = await fetch(`${this.base()}/projects/${projectId}/extensions`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`getExtensions failed: ${res.status}`);
    return res.json();
  }

  /** Enable/disable a built-in server for a project. */
  async setExtension(projectId: string, name: string, enabled: boolean): Promise<ExtensionDto> {
    const res = await fetch(`${this.base()}/projects/${projectId}/extensions/${name}`, {
      method: "PUT",
      headers: this.headers(),
      body: JSON.stringify({ enabled }),
    });
    if (!res.ok) throw new Error(`setExtension failed: ${res.status}`);
    return res.json();
  }

  /** A project's durable memories (read-only; ADR-0007). */
  async listMemories(projectId: string): Promise<MemoryDto[]> {
    const res = await fetch(`${this.base()}/projects/${projectId}/memories`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`listMemories failed: ${res.status}`);
    return res.json();
  }

  /** A project's saved skills (read-only; ADR-0006). */
  async listSkills(projectId: string): Promise<SkillDto[]> {
    const res = await fetch(`${this.base()}/projects/${projectId}/skills`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`listSkills failed: ${res.status}`);
    return res.json();
  }

  /** A project's flashcard decks with their due counts (read-only; Phase 3a). */
  async listDecks(projectId: string): Promise<DeckDto[]> {
    const res = await fetch(`${this.base()}/projects/${projectId}/decks`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`listDecks failed: ${res.status}`);
    return res.json();
  }

  /** Idempotently seed the investing workspace (default project + expert team). */
  async ensureInvestingWorkspace(): Promise<InvestingWorkspaceDto> {
    const res = await fetch(`${this.base()}/investing/workspace`, {
      method: "POST",
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`ensureInvestingWorkspace failed: ${res.status}`);
    return res.json();
  }

  /** The project's tracked assets (newest interest first). */
  async listAssets(projectId: string): Promise<AssetDto[]> {
    const res = await fetch(`${this.base()}/projects/${projectId}/assets`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`listAssets failed: ${res.status}`);
    return res.json();
  }

  /** Stop watching a symbol (the one-click revocation of a silent track). */
  async untrackAsset(projectId: string, symbol: string): Promise<void> {
    const res = await fetch(
      `${this.base()}/projects/${projectId}/assets/${encodeURIComponent(symbol)}`,
      { method: "DELETE", headers: this.headers() },
    );
    if (!res.ok) throw new Error(`untrackAsset failed: ${res.status}`);
  }

  /** Latest EOD quotes with provenance; unavailable symbols are omitted, never invented. */
  async listQuotes(projectId: string, symbols: string[]): Promise<QuoteDto[]> {
    if (symbols.length === 0) return [];
    const qs = encodeURIComponent(symbols.join(","));
    const res = await fetch(
      `${this.base()}/projects/${projectId}/quotes?symbols=${qs}`,
      { headers: this.headers() },
    );
    if (!res.ok) throw new Error(`listQuotes failed: ${res.status}`);
    return res.json();
  }

  /** A project's active adaptive study plan, or null (read-only; Phase 3b). */
  async studyPlan(projectId: string): Promise<StudyPlanDto | null> {
    const res = await fetch(`${this.base()}/projects/${projectId}/study-plan`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`studyPlan failed: ${res.status}`);
    return res.json();
  }

  /** A project's recipes (metadata only; Phase 3c). */
  async listRecipes(projectId: string): Promise<RecipeSummaryDto[]> {
    const res = await fetch(`${this.base()}/projects/${projectId}/recipes`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`listRecipes failed: ${res.status}`);
    return res.json();
  }

  /** Run a recipe now with optional parameter values; returns the run's final message. */
  async runRecipe(
    projectId: string,
    name: string,
    params: Record<string, string> = {},
  ): Promise<RecipeRunResult> {
    const res = await fetch(
      `${this.base()}/projects/${projectId}/recipes/${encodeURIComponent(name)}/run`,
      {
        method: "POST",
        headers: this.headers(),
        body: JSON.stringify({ params }),
      },
    );
    if (!res.ok) throw new Error(`runRecipe failed: ${res.status}`);
    return res.json();
  }

  /** A project's recipe schedules (Phase 3d). */
  async listSchedules(projectId: string): Promise<ScheduleDto[]> {
    const res = await fetch(`${this.base()}/projects/${projectId}/schedules`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`listSchedules failed: ${res.status}`);
    return res.json();
  }

  /** Create a schedule (once at `run_at`, or recurring via `cron_expr`). */
  async createSchedule(
    projectId: string,
    body: CreateScheduleRequest,
  ): Promise<ScheduleDto> {
    const res = await fetch(`${this.base()}/projects/${projectId}/schedules`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify(body),
    });
    if (!res.ok) throw new Error(`createSchedule failed: ${res.status}`);
    return res.json();
  }

  /** Update a schedule: enable/disable and/or its delivery flags (only present fields change). */
  async setSchedule(
    projectId: string,
    scheduleId: string,
    update: SetScheduleRequest,
  ): Promise<ScheduleDto> {
    const res = await fetch(
      `${this.base()}/projects/${projectId}/schedules/${scheduleId}`,
      {
        method: "PUT",
        headers: this.headers(),
        body: JSON.stringify(update),
      },
    );
    if (!res.ok) throw new Error(`setSchedule failed: ${res.status}`);
    return res.json();
  }

  /** Delete a schedule. */
  async deleteSchedule(projectId: string, scheduleId: string): Promise<void> {
    const res = await fetch(
      `${this.base()}/projects/${projectId}/schedules/${scheduleId}`,
      { method: "DELETE", headers: this.headers() },
    );
    if (!res.ok) throw new Error(`deleteSchedule failed: ${res.status}`);
  }

  /** A schedule's run history. */
  async scheduleRuns(projectId: string, scheduleId: string): Promise<ScheduledRunDto[]> {
    const res = await fetch(
      `${this.base()}/projects/${projectId}/schedules/${scheduleId}/runs`,
      { headers: this.headers() },
    );
    if (!res.ok) throw new Error(`scheduleRuns failed: ${res.status}`);
    return res.json();
  }

  /** A project's masters (listing metadata; Phase 4a). */
  async listMasters(projectId: string): Promise<MasterSummaryDto[]> {
    const res = await fetch(`${this.base()}/projects/${projectId}/masters`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`listMasters failed: ${res.status}`);
    return res.json();
  }

  /** The full master (persona, model, tool allow-list) by slug. */
  async getMaster(projectId: string, slug: string): Promise<MasterDto> {
    const res = await fetch(`${this.base()}/projects/${projectId}/masters/${slug}`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`getMaster failed: ${res.status}`);
    return res.json();
  }

  /** Create or overwrite a master; returns it with its canonical slug. */
  async createMaster(projectId: string, master: MasterDto): Promise<MasterDto> {
    const res = await fetch(`${this.base()}/projects/${projectId}/masters`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify(master),
    });
    if (!res.ok) throw new Error(`createMaster failed: ${res.status}`);
    return res.json();
  }

  async deleteMaster(projectId: string, slug: string): Promise<void> {
    const res = await fetch(`${this.base()}/projects/${projectId}/masters/${slug}`, {
      method: "DELETE",
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`deleteMaster failed: ${res.status}`);
  }

  /** Run a brief through one master (its persona + model + tools); returns the final message. */
  async runMaster(projectId: string, slug: string, brief: string): Promise<MasterRunResult> {
    const res = await fetch(`${this.base()}/projects/${projectId}/masters/${slug}/run`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({ brief }),
    });
    if (!res.ok) throw new Error(`runMaster failed: ${res.status}`);
    return res.json();
  }

  /** Pre-installed ACP coding harnesses detected on this machine (Phase 4i; detection never spawns). */
  async getHarnesses(): Promise<AvailableHarnessDto[]> {
    const res = await fetch(`${this.base()}/acp/harnesses`, { headers: this.headers() });
    if (!res.ok) throw new Error(`getHarnesses failed: ${res.status}`);
    return res.json();
  }

  // --- Standalone (global) masters — Masters sidebar, no project required -----

  /** The built-in "system master" template gallery. */
  async listMasterTemplates(): Promise<MasterDto[]> {
    const res = await fetch(`${this.base()}/masters/templates`, { headers: this.headers() });
    if (!res.ok) throw new Error(`listMasterTemplates failed: ${res.status}`);
    return res.json();
  }

  /** The user's standalone masters (listing metadata). */
  async listGlobalMasters(): Promise<MasterSummaryDto[]> {
    const res = await fetch(`${this.base()}/masters`, { headers: this.headers() });
    if (!res.ok) throw new Error(`listGlobalMasters failed: ${res.status}`);
    return res.json();
  }

  /** The full standalone master by slug. */
  async getGlobalMaster(slug: string): Promise<MasterDto> {
    const res = await fetch(`${this.base()}/masters/${slug}`, { headers: this.headers() });
    if (!res.ok) throw new Error(`getGlobalMaster failed: ${res.status}`);
    return res.json();
  }

  /** Create or overwrite a standalone master; returns it with its canonical slug. */
  async createGlobalMaster(master: MasterDto): Promise<MasterDto> {
    const res = await fetch(`${this.base()}/masters`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify(master),
    });
    if (!res.ok) throw new Error(`createGlobalMaster failed: ${res.status}`);
    return res.json();
  }

  async deleteGlobalMaster(slug: string): Promise<void> {
    const res = await fetch(`${this.base()}/masters/${slug}`, {
      method: "DELETE",
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`deleteGlobalMaster failed: ${res.status}`);
  }

  /** The user-starred default master slug (empty when none). */
  async getDefaultMaster(): Promise<string> {
    const res = await fetch(`${this.base()}/masters/default`, { headers: this.headers() });
    if (!res.ok) throw new Error(`getDefaultMaster failed: ${res.status}`);
    const dto: DefaultMasterDto = await res.json();
    return dto.slug ?? "";
  }

  /** Star a master as the default (used by quick chat when none is picked). */
  async setDefaultMaster(slug: string): Promise<void> {
    const res = await fetch(`${this.base()}/masters/default`, {
      method: "PUT",
      headers: this.headers(),
      body: JSON.stringify({ slug }),
    });
    if (!res.ok) throw new Error(`setDefaultMaster failed: ${res.status}`);
  }

  /** Start a quick chat over one or many masters; returns a team-bound group session. */
  async startQuickChat(masters: string[]): Promise<SessionDto> {
    const res = await fetch(`${this.base()}/masters/quickchat`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({ masters }),
    });
    if (!res.ok) throw new Error(`startQuickChat failed: ${res.status}`);
    return res.json();
  }

  // --- Cloud catalog (public system masters + skills) + global skills --------

  /** Force a sync of the public system masters + skills catalog from the cloud. */
  async syncCatalog(): Promise<CatalogStatusDto> {
    const res = await fetch(`${this.base()}/catalog/sync`, {
      method: "POST",
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`syncCatalog failed: ${res.status}`);
    return res.json();
  }

  /** Last-synced catalog version/time + installed counts. */
  async getCatalogStatus(): Promise<CatalogStatusDto> {
    const res = await fetch(`${this.base()}/catalog/status`, { headers: this.headers() });
    if (!res.ok) throw new Error(`getCatalogStatus failed: ${res.status}`);
    return res.json();
  }

  /** The standalone (system) skills synced from the cloud catalog. */
  async listGlobalSkills(): Promise<SkillDto[]> {
    const res = await fetch(`${this.base()}/skills`, { headers: this.headers() });
    if (!res.ok) throw new Error(`listGlobalSkills failed: ${res.status}`);
    return res.json();
  }

  async deleteGlobalSkill(slug: string): Promise<void> {
    const res = await fetch(`${this.base()}/skills/${slug}`, {
      method: "DELETE",
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`deleteGlobalSkill failed: ${res.status}`);
  }

  /** A project's master teams (listing metadata; Phase 4b). */
  async listTeams(projectId: string): Promise<TeamSummaryDto[]> {
    const res = await fetch(`${this.base()}/projects/${projectId}/teams`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`listTeams failed: ${res.status}`);
    return res.json();
  }

  async getTeam(projectId: string, slug: string): Promise<TeamDto> {
    const res = await fetch(`${this.base()}/projects/${projectId}/teams/${slug}`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`getTeam failed: ${res.status}`);
    return res.json();
  }

  async createTeam(projectId: string, team: CreateTeamRequest): Promise<TeamDto> {
    const res = await fetch(`${this.base()}/projects/${projectId}/teams`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify(team),
    });
    if (!res.ok) throw new Error(`createTeam failed: ${res.status}`);
    return res.json();
  }

  async deleteTeam(projectId: string, slug: string): Promise<void> {
    const res = await fetch(`${this.base()}/projects/${projectId}/teams/${slug}`, {
      method: "DELETE",
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`deleteTeam failed: ${res.status}`);
  }

  /** Export a team + its masters as a portable JSON bundle (Phase 4h). */
  async exportTeamBundle(projectId: string, slug: string): Promise<TeamBundle> {
    const res = await fetch(`${this.base()}/projects/${projectId}/teams/${slug}/bundle`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`exportTeamBundle failed: ${res.status}`);
    return res.json();
  }

  /** Import a bundle into a project, recreating its masters + the team (overwrites same slugs). */
  async importBundle(projectId: string, bundle: TeamBundle): Promise<BundleImportResult> {
    const res = await fetch(`${this.base()}/projects/${projectId}/bundles`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify(bundle),
    });
    if (!res.ok) throw new Error(`importBundle failed: ${res.status}`);
    return res.json();
  }

  /** Router recommendation for a brief (ranked masters + the selected one); executes nothing. */
  async routeTeam(projectId: string, slug: string, brief: string): Promise<RouteResultDto> {
    const res = await fetch(`${this.base()}/projects/${projectId}/teams/${slug}/route`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({ brief }),
    });
    if (!res.ok) throw new Error(`routeTeam failed: ${res.status}`);
    return res.json();
  }

  /** Route (or override) and dispatch the chosen master; returns who ran + their result. */
  async runTeam(
    projectId: string,
    slug: string,
    brief: string,
    master?: string,
  ): Promise<TeamRunResult> {
    const res = await fetch(`${this.base()}/projects/${projectId}/teams/${slug}/run`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({ brief, master }),
    });
    if (!res.ok) throw new Error(`runTeam failed: ${res.status}`);
    return res.json();
  }

  /** Start a group chat session bound to a team (Phase 4c). */
  async startTeamSession(projectId: string, slug: string): Promise<SessionDto> {
    const res = await fetch(`${this.base()}/projects/${projectId}/teams/${slug}/session`, {
      method: "POST",
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`startTeamSession failed: ${res.status}`);
    return res.json();
  }

  /** Post a user message into a group session; returns the addressed masters + their replies. */
  async postGroup(sessionId: string, content: string): Promise<GroupPostResult> {
    const res = await fetch(`${this.base()}/sessions/${sessionId}/group`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({ content }),
    });
    if (!res.ok) throw new Error(`postGroup failed: ${res.status}`);
    return res.json();
  }

  /** A project's external MCP connectors (Phase 4d). */
  async listConnectors(projectId: string): Promise<ConnectorDto[]> {
    const res = await fetch(`${this.base()}/projects/${projectId}/connectors`, {
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`listConnectors failed: ${res.status}`);
    return res.json();
  }

  async createConnector(projectId: string, body: CreateConnectorRequest): Promise<ConnectorDto> {
    const res = await fetch(`${this.base()}/projects/${projectId}/connectors`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify(body),
    });
    if (!res.ok) throw new Error(`createConnector failed: ${res.status}`);
    return res.json();
  }

  async setConnectorEnabled(
    projectId: string,
    name: string,
    enabled: boolean,
  ): Promise<ConnectorDto> {
    const res = await fetch(`${this.base()}/projects/${projectId}/connectors/${name}`, {
      method: "PUT",
      headers: this.headers(),
      body: JSON.stringify({ enabled }),
    });
    if (!res.ok) throw new Error(`setConnectorEnabled failed: ${res.status}`);
    return res.json();
  }

  async deleteConnector(projectId: string, name: string): Promise<void> {
    const res = await fetch(`${this.base()}/projects/${projectId}/connectors/${name}`, {
      method: "DELETE",
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`deleteConnector failed: ${res.status}`);
  }

  async getSettings(): Promise<SettingsDto> {
    const res = await fetch(`${this.base()}/settings`, { headers: this.headers() });
    if (!res.ok) throw new Error(`getSettings failed: ${res.status}`);
    return res.json();
  }

  async updateSettings(update: SettingsUpdate): Promise<SettingsDto> {
    const res = await fetch(`${this.base()}/settings`, {
      method: "PUT",
      headers: this.headers(),
      body: JSON.stringify(update),
    });
    if (!res.ok) throw new Error(`updateSettings failed: ${res.status}`);
    return res.json();
  }

  /** The configurable provider catalog + per-provider state (key presence, base URL) + active default. */
  async getProviders(): Promise<ProvidersDto> {
    const res = await fetch(`${this.base()}/settings/providers`, { headers: this.headers() });
    if (!res.ok) throw new Error(`getProviders failed: ${res.status}`);
    return res.json();
  }

  /** Resolved runtime environment (data home, effective provider, config sources, env overrides). */
  async getEnvironment(): Promise<EnvironmentDto> {
    const res = await fetch(`${this.base()}/settings/environment`, { headers: this.headers() });
    if (!res.ok) throw new Error(`getEnvironment failed: ${res.status}`);
    return res.json();
  }

  /** Validate the current configuration with a live provider test call (the `config check` analogue). */
  async checkConfig(): Promise<ConfigCheckDto> {
    const res = await fetch(`${this.base()}/settings/check`, {
      method: "POST",
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`checkConfig failed: ${res.status}`);
    return res.json();
  }

  /** Store an API key / SMTP password in the OS keychain (never returned by the API). */
  async setSecret(name: string, value: string): Promise<void> {
    const res = await fetch(`${this.base()}/settings/secret`, {
      method: "PUT",
      headers: this.headers(),
      body: JSON.stringify({ name, value }),
    });
    if (!res.ok) throw new Error(`setSecret failed: ${res.status}`);
  }

  /** Remove a secret (API key) from the OS keychain. */
  async deleteSecret(name: string): Promise<void> {
    const res = await fetch(`${this.base()}/settings/secret/${encodeURIComponent(name)}`, {
      method: "DELETE",
      headers: this.headers(),
    });
    if (!res.ok) throw new Error(`deleteSecret failed: ${res.status}`);
  }

  /** Outbound-email (SMTP) settings for routine delivery (Phase 3e, FR-27). */
  async getEmailSettings(): Promise<EmailSettingsDto> {
    const res = await fetch(`${this.base()}/settings/email`, { headers: this.headers() });
    if (!res.ok) throw new Error(`getEmailSettings failed: ${res.status}`);
    return res.json();
  }

  async updateEmailSettings(update: EmailSettingsUpdate): Promise<EmailSettingsDto> {
    const res = await fetch(`${this.base()}/settings/email`, {
      method: "PUT",
      headers: this.headers(),
      body: JSON.stringify(update),
    });
    if (!res.ok) throw new Error(`updateEmailSettings failed: ${res.status}`);
    return res.json();
  }

  /**
   * Open a streaming run over the WebSocket. Returns the socket so the caller can `Stop`
   * (send `{type:"stop"}`) or close it. The token rides in the query string because browser
   * WebSocket APIs cannot set headers.
   */
  openStream(
    sessionId: string,
    content: string,
    handlers: StreamHandlers,
    maxRounds?: number,
  ): WebSocket {
    const url = `ws://127.0.0.1:${this.conn.port}/sessions/${sessionId}/ws?token=${encodeURIComponent(
      this.conn.token,
    )}`;
    const ws = new WebSocket(url);

    ws.onopen = () => {
      // `max_rounds` only applies to team-bound group sessions (Phase 4f); ignored otherwise.
      ws.send(JSON.stringify({ type: "send", content, max_rounds: maxRounds }));
    };

    ws.onmessage = (ev) => {
      const event = JSON.parse(ev.data as string) as ServerEvent;
      switch (event.type) {
        case "message_start":
          handlers.onStart?.();
          break;
        case "token_delta":
          handlers.onDelta(event.text);
          break;
        case "tool_call_started":
          handlers.onToolCall?.(event.id, event.tool, event.summary);
          break;
        case "tool_result":
          handlers.onToolResult?.(event.id, event.summary, event.is_error);
          break;
        case "approval_request":
          handlers.onApproval?.({
            requestId: event.request_id,
            tool: event.tool,
            summary: event.summary,
            classes: event.classes,
            preview: event.preview,
          });
          break;
        case "message_complete":
          handlers.onComplete(event.message_id);
          ws.close();
          break;
        case "error":
          handlers.onError(event.message);
          ws.close();
          break;
        case "group_start":
          handlers.onGroupStart?.(event.round, event.addressed);
          break;
        case "master_delta":
          handlers.onMasterDelta?.(event.round, event.author, event.text);
          break;
        case "master_complete":
          handlers.onMasterComplete?.(event.round, event.author, event.message_id);
          break;
        case "master_error":
          handlers.onMasterError?.(event.round, event.author, event.message);
          break;
        case "master_tool_call":
          handlers.onMasterToolCall?.(event.round, event.author, event.id, event.tool, event.summary);
          break;
        case "master_tool_result":
          handlers.onMasterToolResult?.(
            event.round,
            event.author,
            event.id,
            event.summary,
            event.is_error,
          );
          break;
        case "group_complete":
          handlers.onGroupComplete?.();
          ws.close();
          break;
      }
    };

    ws.onerror = () => handlers.onError("websocket error");
    return ws;
  }

  /** Send an approval decision for a pending request over an open run socket. */
  static sendApproval(
    ws: WebSocket,
    requestId: string,
    decision: "allow" | "allow_folder" | "always_tool" | "deny",
  ): void {
    ws.send(JSON.stringify({ type: "approval_decision", request_id: requestId, decision }));
  }
}
