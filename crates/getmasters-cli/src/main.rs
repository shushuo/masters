//! `getmasters` — optional terminal access to the same agent core the daemon uses (ADR-0001).
//!
//! - `getmasters ping` — an in-process chat turn over the mock provider (no network, no daemon).
//! - `getmasters agent --grant <DIR>` — the full gated tool loop: grant a folder, let the mock
//!   trigger a `files.create`, auto-approve it, and print the streamed events + audit log.
//!   The headless end-to-end of Phase 1a.

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use futures::StreamExt;

use getmasters_core::agent::{AgentEvent, AgentService};
use getmasters_core::config::Config;
use getmasters_core::extensions::ExtensionManager;
use getmasters_core::knowledge::{build_index, Embedder};
use getmasters_core::permission::GrantSet;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::FolderAccess;

#[derive(Parser)]
#[command(name = "getmasters", version, about = "Masters CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run one chat turn and print the streamed reply.
    Ping {
        /// The prompt to send.
        #[arg(default_value = "Hello from getmasters ping")]
        prompt: String,
    },
    /// Run the gated tool loop against a granted folder (mock provider, auto-approved).
    Agent {
        /// Folder to grant the agent read/write access to.
        #[arg(long)]
        grant: PathBuf,
        /// Prompt to run. Defaults to a mock trigger that creates a file in the grant.
        #[arg(long)]
        prompt: Option<String>,
    },
    /// Ingest a folder of documents and ask a grounded question (mock embedder, headless).
    Knowledge {
        /// Folder of documents to grant + ingest.
        #[arg(long)]
        grant: PathBuf,
        /// The question to answer from the ingested documents.
        #[arg(long)]
        ask: String,
    },
    /// Capture + recall file-backed Memory and Skills in a project (mock provider, headless).
    Learn {
        /// Folder to grant the project (also where the project data dir is rooted).
        #[arg(long)]
        grant: PathBuf,
    },
}

/// Host every implemented built-in (the CLI doesn't expose per-project toggles).
fn all_servers() -> std::collections::HashSet<String> {
    ["files", "knowledge", "memory", "skills"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    match Cli::parse().command {
        Command::Ping { prompt } => ping(&prompt).await,
        Command::Agent { grant, prompt } => agent(&grant, prompt).await,
        Command::Knowledge { grant, ask } => knowledge(&grant, &ask).await,
        Command::Learn { grant } => learn(&grant).await,
    }
}

/// Capture and recall durable Memory + Skills in a project, headlessly: the mock provider drives
/// `remember`→`recall` and `create_skill`→`recall_skill`, proving the file-backed learning loop.
async fn learn(grant: &std::path::Path) -> anyhow::Result<()> {
    let grant = grant
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("grant folder '{}' not found: {e}", grant.display()))?;

    let cfg = Config::from_env();
    let store = Store::open_in_memory()?;
    let project = store.create_project("cli-learn", None)?;
    let row =
        store.create_folder_grant(Some(&project), &grant.to_string_lossy(), FolderAccess::Read)?;
    let grants = Arc::new(GrantSet::new(vec![row]));

    let embedder = Arc::new(Embedder::resolve(&cfg, &store));
    let index = build_index(store.clone(), embedder.dim());
    let project_dir = grant.join(".getmasters").join(&project);
    let mgr = ExtensionManager::with_project(
        project.clone(),
        grants.clone(),
        store.clone(),
        embedder,
        index,
        project_dir.clone(),
        &all_servers(),
        &[],
    )
    .await
    .map_err(|e| anyhow::anyhow!("failed to host project servers: {e}"))?;
    let agent = AgentService::new(store, Arc::new(MockProvider::new()), "mock")
        .with_extensions(Arc::new(mgr), grants)
        .with_project_dir(project_dir.clone());
    let session = agent
        .store()
        .create_session(Some(&project), Some("learn"))?;
    eprintln!(
        "project_dir={} session={}",
        project_dir.display(),
        session.id
    );

    eprintln!("--- remember ---");
    drive(
        &agent,
        &session.id,
        "[[tool:memory.remember|title=Deadline|content=The thesis is due in March]]",
    )
    .await?;
    eprintln!("--- recall ---");
    drive(&agent, &session.id, "[[tool:memory.recall|query=deadline]]").await?;

    eprintln!("--- create_skill ---");
    drive(
        &agent,
        &session.id,
        "[[tool:skills.create_skill|name=Summarize PDF|summary=bullet notes|steps=read then outline]]",
    )
    .await?;
    eprintln!("--- recall_skill ---");
    drive(
        &agent,
        &session.id,
        "[[tool:skills.recall_skill|query=summarize pdf]]",
    )
    .await?;

    eprintln!("--- audit ---");
    for (tool, decision, _) in agent.store().list_audit(&session.id)? {
        eprintln!("  {tool}: {decision}");
    }
    Ok(())
}

async fn knowledge(grant: &std::path::Path, ask: &str) -> anyhow::Result<()> {
    let grant = grant
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("grant folder '{}' not found: {e}", grant.display()))?;

    let cfg = Config::from_env();
    let store = Store::open_in_memory()?;
    let project = store.create_project("cli-knowledge", None)?;
    let row =
        store.create_folder_grant(Some(&project), &grant.to_string_lossy(), FolderAccess::Read)?;
    let grants = Arc::new(GrantSet::new(vec![row]));

    let embedder = Arc::new(Embedder::resolve(&cfg, &store));
    let index = build_index(store.clone(), embedder.dim());
    let project_dir = std::env::temp_dir().join(format!("getmasters-cli-{}", project));
    let mgr = ExtensionManager::with_project(
        project.clone(),
        grants.clone(),
        store.clone(),
        embedder.clone(),
        index,
        project_dir.clone(),
        &all_servers(),
        &[],
    )
    .await
    .map_err(|e| anyhow::anyhow!("failed to host project servers: {e}"))?;
    let agent = AgentService::new(store, Arc::new(MockProvider::new()), "mock")
        .with_extensions(Arc::new(mgr), grants)
        .with_project_dir(project_dir);
    let session = agent
        .store()
        .create_session(Some(&project), Some("knowledge"))?;
    eprintln!(
        "grant={} embedder={} session={}",
        grant.display(),
        embedder.provider_name(),
        session.id
    );

    eprintln!("--- ingest ---");
    drive(
        &agent,
        &session.id,
        &format!("[[tool:knowledge.ingest|path={}]]", grant.to_string_lossy()),
    )
    .await?;
    eprintln!("--- ask: {ask} ---");
    drive(
        &agent,
        &session.id,
        &format!("[[tool:knowledge.search|query={ask}]]"),
    )
    .await?;

    eprintln!("--- audit ---");
    for (tool, decision, _) in agent.store().list_audit(&session.id)? {
        eprintln!("  {tool}: {decision}");
    }
    Ok(())
}

/// Run one turn and print streamed text + tool activity.
async fn drive(agent: &AgentService, session_id: &str, prompt: &str) -> anyhow::Result<()> {
    let mut stream = agent.run_turn(session_id, prompt).await;
    while let Some(event) = stream.next().await {
        match event {
            AgentEvent::Delta(text) => {
                print!("{text}");
                flush();
            }
            AgentEvent::ToolCallStarted { name, summary, .. } => {
                eprintln!("\n→ {name} ({summary})")
            }
            AgentEvent::ToolResult {
                summary, is_error, ..
            } => {
                eprintln!(
                    "← {}{}",
                    if is_error { "error: " } else { "" },
                    truncate(&summary, 240)
                )
            }
            AgentEvent::ApprovalRequest(_) => {}
            AgentEvent::Complete { .. } => println!(),
            AgentEvent::Error(e) => anyhow::bail!("agent error: {e}"),
        }
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

async fn ping(prompt: &str) -> anyhow::Result<()> {
    let cfg = Config::from_env();
    let store = Store::open_in_memory()?;
    // A pure offline smoke test over the mock provider (no key, no network), consistent with the
    // other CLI dev commands.
    let provider: Arc<dyn getmasters_core::provider::Provider> = Arc::new(MockProvider::new());
    let provider_name = provider.name();
    let agent = AgentService::new(store, provider, cfg.model.clone());

    let session = agent.store().create_session(None, Some("ping"))?;
    eprintln!(
        "provider={provider_name} model={} session={}",
        cfg.model, session.id
    );
    print!("> ");
    flush();

    let mut stream = agent.run_turn(&session.id, prompt).await;
    while let Some(event) = stream.next().await {
        match event {
            AgentEvent::Delta(text) => {
                print!("{text}");
                flush();
            }
            AgentEvent::Complete { message_id } => {
                println!();
                eprintln!("complete: message={message_id}");
            }
            AgentEvent::Error(e) => {
                println!();
                anyhow::bail!("agent error: {e}");
            }
            // Ping has no tools.
            AgentEvent::ToolCallStarted { .. }
            | AgentEvent::ApprovalRequest(_)
            | AgentEvent::ToolResult { .. } => {}
        }
    }
    Ok(())
}

async fn agent(grant: &std::path::Path, prompt: Option<String>) -> anyhow::Result<()> {
    let grant = grant
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("grant folder '{}' not found: {e}", grant.display()))?;

    let store = Store::open_in_memory()?;
    let project = store.create_project("cli-agent", None)?;
    let row = store.create_folder_grant(
        Some(&project),
        &grant.to_string_lossy(),
        FolderAccess::ReadWrite,
    )?;
    let grants = GrantSet::new(vec![row.clone()]);
    let extensions = ExtensionManager::with_builtin_files(vec![row])
        .await
        .map_err(|e| anyhow::anyhow!("failed to host files server: {e}"))?;

    // The CLI auto-approves (no approval registry attached).
    let agent = AgentService::new(store, Arc::new(MockProvider::new()), "mock")
        .with_extensions(Arc::new(extensions), Arc::new(grants));
    let session = agent
        .store()
        .create_session(Some(&project), Some("agent"))?;

    let prompt = prompt.unwrap_or_else(|| {
        format!(
            "[[tool:files.create|path={}/getmasters-demo.txt|content=Hello from Masters Phase 1a]]",
            grant.to_string_lossy()
        )
    });
    eprintln!("grant={} session={}", grant.display(), session.id);

    let mut stream = agent.run_turn(&session.id, &prompt).await;
    while let Some(event) = stream.next().await {
        match event {
            AgentEvent::Delta(text) => {
                print!("{text}");
                flush();
            }
            AgentEvent::ToolCallStarted { name, summary, .. } => {
                eprintln!("\n→ tool: {name} ({summary})");
            }
            AgentEvent::ApprovalRequest(req) => {
                eprintln!(
                    "? approval requested for {} (auto-approved by CLI)",
                    req.tool
                );
            }
            AgentEvent::ToolResult {
                summary, is_error, ..
            } => {
                eprintln!(
                    "← result{}: {summary}",
                    if is_error { " (error)" } else { "" }
                );
            }
            AgentEvent::Complete { .. } => println!(),
            AgentEvent::Error(e) => anyhow::bail!("agent error: {e}"),
        }
    }

    // Print the audit trail.
    eprintln!("--- audit log ---");
    for (tool, decision, summary) in agent.store().list_audit(&session.id)? {
        eprintln!(
            "  {tool}: {decision}{}",
            summary.map(|s| format!(" — {s}")).unwrap_or_default()
        );
    }
    Ok(())
}

fn flush() {
    use std::io::Write;
    std::io::stdout().flush().ok();
}
