//! Scheduler firing (Phase 3d, FR-17): seed a due schedule whose recipe writes a file, drive
//! `run_due` directly (deterministic — no timer), and assert the recipe ran, a run was recorded, and
//! the schedule advanced (cron) or disabled (once).

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_server::scheduler;
use getmasters_server::AppState;

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-sched-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

#[tokio::test]
async fn run_due_fires_recipe_and_advances() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let pid = store.create_project("automation", None).unwrap();

    // Grant write to the temp folder so the recipe's file write is allowed.
    store
        .create_folder_grant(
            Some(&pid),
            &dir.to_string_lossy(),
            getmasters_proto::FolderAccess::ReadWrite,
        )
        .unwrap();

    let agent = AgentService::new(store.clone(), Arc::new(MockProvider::new()), "mock");
    let cfg = Config {
        db_path: dir.join("getmasters.db"),
        ..Default::default()
    };
    let state = AppState::new(agent, "t".to_string()).with_config(cfg);

    // A recipe whose substituted prompt is a mock tool-trigger that writes a file.
    let out_path = dir.join("fired.txt");
    let recipe_store = getmasters_server::recipe::RecipeStore::new(
        state.project_dir(&pid),
        pid.clone(),
        store.clone(),
    );
    recipe_store
        .save(&getmasters_proto::RecipeDto {
            name: "writer".into(),
            title: "Writer".into(),
            description: String::new(),
            parameters: vec![],
            prompt: format!(
                "[[tool:files.create|path={}|content=scheduled]]",
                out_path.to_string_lossy()
            ),
            extensions: vec!["files".into()],
        })
        .unwrap();

    // A cron schedule already due (next_run_at in the past).
    let sid = store
        .create_schedule(
            &pid,
            "writer",
            "{}",
            "cron",
            Some("0 9 * * *"),
            Some(1_000),
            false,
            false,
        )
        .unwrap();

    // Fire everything due as of "now".
    let now = 2_000_000_000_000; // far future so the schedule is due
    scheduler::run_due(&state, now).await;

    // The recipe ran and wrote the file.
    let written = std::fs::read_to_string(&out_path).expect("scheduled run should write the file");
    assert_eq!(written, "scheduled");

    // A run was recorded as ok.
    let runs = store.list_scheduled_runs(&sid).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, "ok");
    assert!(runs[0].session_id.is_some());

    // The cron schedule advanced to a future fire and stayed enabled.
    let row = store.get_schedule(&sid).unwrap().unwrap();
    assert!(row.enabled);
    assert!(
        row.next_run_at.unwrap() > now,
        "next fire should be in the future"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn run_due_disables_one_off_after_firing() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let pid = store.create_project("automation", None).unwrap();

    let agent = AgentService::new(store.clone(), Arc::new(MockProvider::new()), "mock");
    let cfg = Config {
        db_path: dir.join("getmasters.db"),
        ..Default::default()
    };
    let state = AppState::new(agent, "t".to_string()).with_config(cfg);

    // A recipe with no side effects (the mock just echoes); we only care about scheduling state.
    getmasters_server::recipe::RecipeStore::new(
        state.project_dir(&pid),
        pid.clone(),
        store.clone(),
    )
    .save(&getmasters_proto::RecipeDto {
        name: "noop".into(),
        title: "Noop".into(),
        description: String::new(),
        parameters: vec![],
        prompt: "just say hi".into(),
        extensions: vec![],
    })
    .unwrap();

    let sid = store
        .create_schedule(&pid, "noop", "{}", "once", None, Some(1_000), false, false)
        .unwrap();

    scheduler::run_due(&state, 2_000_000_000_000).await;

    // A one-off disables and clears its next fire after running.
    let row = store.get_schedule(&sid).unwrap().unwrap();
    assert!(!row.enabled);
    assert!(row.next_run_at.is_none());
    assert_eq!(store.list_scheduled_runs(&sid).unwrap().len(), 1);

    std::fs::remove_dir_all(&dir).ok();
}
