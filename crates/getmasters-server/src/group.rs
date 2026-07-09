//! **Multi-master group chat** (Phase 4c, FR-43; ADR-0012) — *"shared read context, isolated gated
//! execution"*.
//!
//! A group session is bound to a master team. A user message is resolved against the team's
//! participants (`@mention`/`@all`/coordinator, [`getmasters_core::masters::mentions`]) and persisted into
//! the shared transcript attributed to `user`. Each addressed master then answers **from one snapshot**
//! of that transcript, **in parallel**, on its own persona + model + tools — and only its **final
//! attributed reply** is posted back into the group session. To keep parallel runs race-free and tool
//! scratch out of the shared transcript, each master runs in its own **isolated scratch session**
//! seeded with the snapshot (its tool calls stay there; only the reply returns to the group).
//!
//! Single round; multi-round turn-taking, live streaming, and workflows are deferred (ADR-0012).

use std::sync::{Arc, Mutex};

use futures::StreamExt;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::AbortHandle;

use getmasters_core::agent::AgentEvent;
use getmasters_core::masters::mentions;
use getmasters_core::masters::Master;
use getmasters_proto::{GroupMasterErrorDto, GroupPostResult, MessageDto};

use crate::master::{load_master_any, run_master_stream};
use crate::state::AppState;

/// Start a group chat for a team: create a session bound to it.
pub fn start(
    state: &AppState,
    project_id: &str,
    team_slug: &str,
    title: Option<&str>,
) -> Result<getmasters_proto::SessionDto, String> {
    let store = state.agent.store();
    // 404-able: the team must exist.
    store
        .get_team(project_id, team_slug)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("team '{team_slug}' not found"))?;
    let title = title
        .map(str::to_string)
        .unwrap_or_else(|| format!("group:{team_slug}"));
    let session = store
        .create_session(Some(project_id), Some(&title))
        .map_err(|e| e.to_string())?;
    store
        .bind_session_team(&session.id, team_slug)
        .map_err(|e| e.to_string())?;
    store.get_session(&session.id).map_err(|e| e.to_string())
}

/// The hard cap on mention-driven rounds per group post (Phase 4f): the user turn + up to two
/// follow-up rounds. The streaming path uses this; the sync path may override into `1..=5`.
const MAX_GROUP_ROUNDS: usize = 3;

/// Clamp an optional per-call round cap into `1..=5`, defaulting to [`MAX_GROUP_ROUNDS`].
fn clamp_rounds(max_rounds: Option<u32>) -> usize {
    match max_rounds {
        Some(n) => n.clamp(1, 5) as usize,
        None => MAX_GROUP_ROUNDS,
    }
}

/// The resolved-and-staged state shared by the synchronous [`post`] and streaming [`stream_post`]:
/// who round 0 addresses, the member masters, and the `(slug, name)` participants for follow-up
/// mention resolution. (Each round re-snapshots the transcript, so no snapshot is carried here.)
struct GroupSetup {
    project_id: String,
    addressed: Vec<String>,
    members: Vec<(String, Master)>,
    participants: Vec<(String, String)>,
}

/// Resolve round 0's addressed masters and persist the user message into the shared transcript.
async fn setup(state: &AppState, session_id: &str, text: &str) -> Result<GroupSetup, String> {
    let store = state.agent.store();
    let session = store.get_session(session_id).map_err(|e| e.to_string())?;
    let project_id = session
        .project_id
        .ok_or_else(|| "group session has no project".to_string())?;
    let team_slug = session
        .team_slug
        .ok_or_else(|| "session is not a group chat (no team bound)".to_string())?;
    let team = store
        .get_team(&project_id, &team_slug)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("team '{team_slug}' not found"))?;

    // Load the member masters (skip any whose file is missing) → participants for mention resolution.
    // Resolution falls back to the global store, so a quick-chat team of standalone masters works.
    let members: Vec<(String, Master)> = team
        .members
        .iter()
        .filter_map(|slug| match load_master_any(state, &project_id, slug) {
            Ok(Some(e)) => Some((slug.clone(), e)),
            _ => None,
        })
        .collect();
    let participants: Vec<(String, String)> = members
        .iter()
        .map(|(slug, e)| (slug.clone(), e.name.clone()))
        .collect();

    let addressed = mentions::resolve(text, &participants, &team.coordinator_slug);
    if addressed.is_empty() {
        return Err("no master to address (no mention match and no coordinator)".into());
    }

    // Persist the user message into the shared transcript, recording who it addressed.
    let addressed_json = serde_json::to_string(&addressed).ok();
    store
        .insert_message_attributed(session_id, "user", "user", text, addressed_json.as_deref())
        .map_err(|e| e.to_string())?;

    Ok(GroupSetup {
        project_id,
        addressed,
        members,
        participants,
    })
}

/// Fold [`mentions::followups`] over a round's replies → the next round's addressed masters (explicit
/// `@mentions` of *other* masters, deduped, order-preserving). Empty → the conversation has settled.
fn resolve_followups(replies: &[MessageDto], participants: &[(String, String)]) -> Vec<String> {
    let mut next: Vec<String> = Vec::new();
    for reply in replies {
        for slug in mentions::followups(&reply.content, participants, &reply.author) {
            if !next.contains(&slug) {
                next.push(slug);
            }
        }
    }
    next
}

/// Create a master's isolated scratch session, seeded with the snapshot transcript. The scratch
/// keeps the master's tool calls (and, for ACP masters, its file work) out of the shared group
/// transcript; the dispatch itself runs through [`run_master_stream`] (backend-agnostic).
async fn make_scratch(
    state: &AppState,
    project_id: &str,
    group_session_id: &str,
    slug: &str,
    snapshot: &[MessageDto],
) -> Result<String, String> {
    let store = state.agent.store();
    let scratch = store
        .create_session(
            Some(project_id),
            Some(&format!("group:{group_session_id}:{slug}")),
        )
        .map_err(|e| e.to_string())?;
    for m in snapshot {
        store
            .insert_message_attributed(&scratch.id, &m.author, &m.role, &m.content, None)
            .map_err(|e| e.to_string())?;
    }
    Ok(scratch.id)
}

/// Post a user message into a group session and run **bounded multi-round turn-taking** (Phase 4f):
/// resolve+persist the message, then for each round dispatch the addressed masters in parallel from a
/// fresh transcript snapshot, and continue while their replies explicitly `@mention` other masters —
/// up to `max_rounds` (clamped `1..=5`, default [`MAX_GROUP_ROUNDS`]). Returns round 0's addressed set
/// + every round's attributed replies in order.
pub async fn post(
    state: &AppState,
    session_id: &str,
    text: &str,
    max_rounds: Option<u32>,
) -> Result<GroupPostResult, String> {
    let setup = setup(state, session_id, text).await?;
    let cap = clamp_rounds(max_rounds);
    let first_addressed = setup.addressed.clone();

    let mut all_replies: Vec<MessageDto> = Vec::new();
    let mut all_errors: Vec<GroupMasterErrorDto> = Vec::new();
    let mut addressed = setup.addressed;
    for _round in 0..cap {
        if addressed.is_empty() {
            break;
        }
        let (replies, errors) = run_round(
            state,
            session_id,
            &setup.project_id,
            &setup.members,
            &addressed,
        )
        .await?;
        addressed = resolve_followups(&replies, &setup.participants);
        all_replies.extend(replies);
        all_errors.extend(errors);
    }

    Ok(GroupPostResult {
        addressed: first_addressed,
        replies: all_replies,
        errors: all_errors,
    })
}

/// Dispatch one round's addressed masters in parallel from a **fresh** snapshot (so they see prior
/// rounds' replies). Each runs in its own isolated scratch session; returns the posted replies.
async fn run_round(
    state: &AppState,
    session_id: &str,
    project_id: &str,
    members: &[(String, Master)],
    addressed: &[String],
) -> Result<(Vec<MessageDto>, Vec<GroupMasterErrorDto>), String> {
    let snapshot = state
        .agent
        .store()
        .list_messages(session_id)
        .map_err(|e| e.to_string())?;
    let roster: Vec<(String, String)> = members
        .iter()
        .map(|(slug, e)| (slug.clone(), e.name.clone()))
        .collect();

    let mut handles = Vec::new();
    for slug in addressed {
        let Some((_, master)) = members.iter().find(|(s, _)| s == slug) else {
            continue;
        };
        let state = state.clone();
        let project_id = project_id.to_string();
        let session_id = session_id.to_string();
        let slug = slug.clone();
        let master = master.clone();
        let snapshot = snapshot.clone();
        // Teammates from this master's perspective: everyone else on the roster.
        let teammates: Vec<(String, String)> =
            roster.iter().filter(|(s, _)| s != &slug).cloned().collect();
        handles.push((
            slug.clone(),
            tokio::spawn(async move {
                dispatch_master(
                    &state,
                    &project_id,
                    &session_id,
                    &slug,
                    &master,
                    &snapshot,
                    &teammates,
                )
                .await
            }),
        ));
    }

    // A failing master no longer sinks the round: collect the successes + per-master errors
    // (matching the streaming path's non-terminal MasterError semantics).
    let mut replies: Vec<MessageDto> = Vec::new();
    let mut errors: Vec<GroupMasterErrorDto> = Vec::new();
    for (slug, handle) in handles {
        match handle.await {
            Ok(Ok(reply)) => replies.push(reply),
            Ok(Err(e)) => errors.push(GroupMasterErrorDto {
                author: slug,
                message: e,
            }),
            Err(e) => errors.push(GroupMasterErrorDto {
                author: slug,
                message: format!("dispatch task failed: {e}"),
            }),
        }
    }
    Ok((replies, errors))
}

/// Run one master to completion and post its final reply into the group session (synchronous path).
async fn dispatch_master(
    state: &AppState,
    project_id: &str,
    group_session_id: &str,
    slug: &str,
    master: &Master,
    snapshot: &[MessageDto],
    teammates: &[(String, String)],
) -> Result<MessageDto, String> {
    let scratch_id = make_scratch(state, project_id, group_session_id, slug, snapshot).await?;
    let mut stream = run_master_stream(
        state,
        project_id,
        &scratch_id,
        slug,
        master,
        None,
        teammates,
    )
    .await?;
    let store = state.agent.store();
    let mut posted: Option<MessageDto> = None;
    let mut failure: Option<String> = None;
    while let Some(ev) = stream.next().await {
        match ev {
            AgentEvent::Complete { message_id } => {
                if let Ok(m) = store.get_message(&message_id) {
                    posted = Some(
                        store
                            .insert_message_attributed(
                                group_session_id,
                                slug,
                                "assistant",
                                &m.content,
                                None,
                            )
                            .map_err(|e| e.to_string())?,
                    );
                }
            }
            AgentEvent::Error(e) => {
                failure = Some(e);
                break;
            }
            _ => {}
        }
    }
    // The reply (or error) is back in the group transcript — the scratch has served its purpose.
    drop_scratch(state, &scratch_id);
    if let Some(e) = failure {
        return Err(e);
    }
    posted.ok_or_else(|| format!("master '{slug}' produced no reply"))
}

/// Delete a finished scratch session (messages + events; audit rows are kept). Best-effort —
/// a failed delete only leaves a row the startup GC sweeps later.
fn drop_scratch(state: &AppState, scratch_id: &str) {
    if let Err(e) = state.agent.store().delete_session(scratch_id) {
        tracing::debug!(error = %e, scratch = %scratch_id, "failed to delete group scratch session");
    }
}

/// An item on the live group stream: a round boundary, or one master's event within a round.
pub enum GroupStreamEvent {
    /// A new round began; `addressed` will reply (round 0 = the user's turn, then follow-ups).
    RoundStart { round: u32, addressed: Vec<String> },
    /// One master's `AgentEvent` within `round`, attributed to `author`.
    Master {
        round: u32,
        author: String,
        event: AgentEvent,
    },
}

/// A live group turn: a channel of [`GroupStreamEvent`]s that closes when every round has finished.
/// `aborts` holds the orchestrator + each in-flight master task's handle, so a Stop cancels them all.
pub struct GroupTurn {
    pub events: UnboundedReceiver<GroupStreamEvent>,
    pub aborts: Arc<Mutex<Vec<AbortHandle>>>,
}

/// Streaming variant of [`post`]: set up the turn, then spawn a single **orchestrator** task that runs
/// the mention-driven rounds sequentially — each round streams its masters into the shared channel and
/// posts their replies back — and closes the channel when the loop ends. Returns immediately.
pub async fn stream_post(
    state: &AppState,
    session_id: &str,
    text: &str,
    max_rounds: Option<u32>,
) -> Result<GroupTurn, String> {
    let setup = setup(state, session_id, text).await?;
    let cap = clamp_rounds(max_rounds);
    let (tx, rx) = mpsc::unbounded_channel::<GroupStreamEvent>();
    let aborts: Arc<Mutex<Vec<AbortHandle>>> = Arc::new(Mutex::new(Vec::new()));

    let aborts_for_task = aborts.clone();
    let state = state.clone();
    let session_id = session_id.to_string();
    let orchestrator = tokio::spawn(async move {
        run_rounds_streaming(
            state,
            session_id,
            setup.project_id,
            setup.members,
            setup.participants,
            setup.addressed,
            cap,
            aborts_for_task,
            tx,
        )
        .await;
    });
    aborts.lock().unwrap().push(orchestrator.abort_handle());

    Ok(GroupTurn { events: rx, aborts })
}

/// The streaming orchestrator: run up to `cap` mention-driven rounds (clamped from the client's
/// `max_rounds`, default [`MAX_GROUP_ROUNDS`]). Each round emits a `RoundStart`, streams its masters in
/// parallel (registering their abort handles), awaits the whole round, then resolves follow-up mentions
/// for the next. Dropping `tx` at the end closes the stream.
#[allow(clippy::too_many_arguments)]
async fn run_rounds_streaming(
    state: AppState,
    session_id: String,
    project_id: String,
    members: Vec<(String, Master)>,
    participants: Vec<(String, String)>,
    mut addressed: Vec<String>,
    cap: usize,
    aborts: Arc<Mutex<Vec<AbortHandle>>>,
    tx: UnboundedSender<GroupStreamEvent>,
) {
    for round in 0..cap as u32 {
        if addressed.is_empty() {
            break;
        }
        let _ = tx.send(GroupStreamEvent::RoundStart {
            round,
            addressed: addressed.clone(),
        });

        // Fresh snapshot so this round's masters see prior rounds' replies.
        let snapshot = match state.agent.store().list_messages(&session_id) {
            Ok(s) => s,
            Err(_) => break,
        };

        let mut handles = Vec::new();
        for slug in &addressed {
            let Some((_, master)) = members.iter().find(|(s, _)| s == slug) else {
                continue;
            };
            let state = state.clone();
            let project_id = project_id.clone();
            let session_id = session_id.clone();
            let slug = slug.clone();
            let master = master.clone();
            let snapshot = snapshot.clone();
            let tx = tx.clone();
            // Teammates from this master's perspective: everyone else on the roster.
            let teammates: Vec<(String, String)> = participants
                .iter()
                .filter(|(s, _)| s != &slug)
                .cloned()
                .collect();
            let handle = tokio::spawn(async move {
                stream_master(
                    &state,
                    &project_id,
                    &session_id,
                    &slug,
                    &master,
                    &snapshot,
                    &teammates,
                    round,
                    tx,
                )
                .await
            });
            aborts.lock().unwrap().push(handle.abort_handle());
            handles.push(handle);
        }

        // Await the whole round; collect posted replies to scan for follow-up mentions.
        let mut replies: Vec<MessageDto> = Vec::new();
        for handle in handles {
            if let Ok(Some(reply)) = handle.await {
                replies.push(reply);
            }
        }
        addressed = resolve_followups(&replies, &participants);
    }
}

/// Stream one master's answer into `tx` (tagged by round + author). On completion, post the final reply
/// into the group session and emit a `Complete` carrying the **group** message id (not the scratch id).
/// Returns the posted group message so the orchestrator can scan it for follow-up mentions.
#[allow(clippy::too_many_arguments)]
async fn stream_master(
    state: &AppState,
    project_id: &str,
    group_session_id: &str,
    slug: &str,
    master: &Master,
    snapshot: &[MessageDto],
    teammates: &[(String, String)],
    round: u32,
    tx: UnboundedSender<GroupStreamEvent>,
) -> Option<MessageDto> {
    let send = |event: AgentEvent| {
        let _ = tx.send(GroupStreamEvent::Master {
            round,
            author: slug.to_string(),
            event,
        });
    };

    let scratch_id = match make_scratch(state, project_id, group_session_id, slug, snapshot).await {
        Ok(v) => v,
        Err(e) => {
            send(AgentEvent::Error(e));
            return None;
        }
    };

    let store = state.agent.store();
    let mut stream = match run_master_stream(
        state,
        project_id,
        &scratch_id,
        slug,
        master,
        None,
        teammates,
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            send(AgentEvent::Error(e));
            drop_scratch(state, &scratch_id);
            return None;
        }
    };
    let mut posted: Option<MessageDto> = None;
    while let Some(ev) = stream.next().await {
        match ev {
            AgentEvent::Complete { message_id } => {
                // Post-back: copy the scratch reply into the group transcript, attributed.
                let group_msg = store.get_message(&message_id).and_then(|m| {
                    store.insert_message_attributed(
                        group_session_id,
                        slug,
                        "assistant",
                        &m.content,
                        None,
                    )
                });
                match group_msg {
                    Ok(m) => {
                        send(AgentEvent::Complete {
                            message_id: m.id.clone(),
                        });
                        posted = Some(m);
                    }
                    Err(e) => send(AgentEvent::Error(e.to_string())),
                }
            }
            other => send(other),
        }
    }
    drop_scratch(state, &scratch_id);
    posted
}
