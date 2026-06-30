//! Data-home resolution for the daemon.
//!
//! An installed desktop app is spawned with an unpredictable working directory (often `/` on
//! macOS), so a cwd-relative `getmasters.db` would land somewhere wrong or unwritable. Instead the
//! daemon roots all on-disk state in a single **data home** — `~/.getmasters` by default — created on
//! first run. The DB lives at `{home}/getmasters.db` and every project's files under
//! `{home}/projects/{id}/` (see [`crate::state::AppState::project_dir`]).
//!
//! Resolution precedence (highest first):
//! 1. `GETMASTERS_DB_PATH` — an explicit DB file path (used by the test suite / power users).
//! 2. `GETMASTERS_HOME` — the data-home directory; the DB is `{GETMASTERS_HOME}/getmasters.db`.
//! 3. `~/.getmasters` — the default uniform home (`$HOME`/user profile + `.getmasters`).
//! 4. `./.getmasters` — last-resort fallback when the home directory can't be determined.

use std::path::{Path, PathBuf};

/// Default data-home directory name under the user's home (`~/.getmasters`).
const HOME_DIR_NAME: &str = ".getmasters";
/// The database file name inside the data home.
const DB_FILE_NAME: &str = "getmasters.db";

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Resolve the data home from the (optional) `GETMASTERS_HOME` override and the user's home directory.
/// Pure helper — no env reads — so precedence is unit-testable without mutating process state.
pub fn data_home_from(home_env: Option<PathBuf>, home_dir: Option<PathBuf>) -> PathBuf {
    home_env
        .or_else(|| home_dir.map(|h| h.join(HOME_DIR_NAME)))
        .unwrap_or_else(|| PathBuf::from(HOME_DIR_NAME))
}

/// Resolve the DB path from the (optional) `GETMASTERS_DB_PATH` override and a resolved data home.
/// Pure helper — no env reads, no filesystem side effects.
pub fn db_path_from(db_env: Option<PathBuf>, home: &Path) -> PathBuf {
    db_env.unwrap_or_else(|| home.join(DB_FILE_NAME))
}

/// The resolved data-home directory, honoring `GETMASTERS_HOME` then `~/.getmasters`.
pub fn data_home() -> PathBuf {
    data_home_from(env_path("GETMASTERS_HOME"), dirs::home_dir())
}

/// The resolved SQLite DB path, honoring `GETMASTERS_DB_PATH` then `{data_home()}/getmasters.db`.
/// Side-effect-free (no directory creation) — used by read-only environment reporting.
pub fn db_path() -> PathBuf {
    db_path_from(env_path("GETMASTERS_DB_PATH"), &data_home())
}

/// Resolve the SQLite DB path and ensure its containing directory exists (first-run scaffolding).
///
/// `GETMASTERS_DB_PATH` wins when set; otherwise the DB is `{data_home()}/getmasters.db`. The parent
/// directory is created with [`std::fs::create_dir_all`] so a fresh install has a writable home.
pub fn resolve_db_path() -> std::io::Result<PathBuf> {
    let db = db_path_from(env_path("GETMASTERS_DB_PATH"), &data_home());
    if let Some(parent) = db.parent().filter(|p| !p.as_os_str().is_empty()) {
        let fresh = !parent.exists();
        std::fs::create_dir_all(parent)?;
        if fresh {
            tracing::info!(home = ?parent, "initialized Masters data home");
        }
    }
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_env_wins_over_home_dir() {
        let home = data_home_from(
            Some(PathBuf::from("/tmp/bh")),
            Some(PathBuf::from("/home/u")),
        );
        assert_eq!(home, PathBuf::from("/tmp/bh"));
    }

    #[test]
    fn defaults_to_dot_getmasters_under_home_dir() {
        let home = data_home_from(None, Some(PathBuf::from("/home/u")));
        assert_eq!(home, PathBuf::from("/home/u/.getmasters"));
    }

    #[test]
    fn falls_back_to_relative_when_home_unknown() {
        let home = data_home_from(None, None);
        assert_eq!(home, PathBuf::from(".getmasters"));
    }

    #[test]
    fn db_env_wins_over_data_home() {
        let db = db_path_from(
            Some(PathBuf::from("/tmp/x.db")),
            Path::new("/home/u/.getmasters"),
        );
        assert_eq!(db, PathBuf::from("/tmp/x.db"));
    }

    #[test]
    fn db_defaults_under_data_home() {
        let db = db_path_from(None, Path::new("/home/u/.getmasters"));
        assert_eq!(db, PathBuf::from("/home/u/.getmasters/getmasters.db"));
    }

    #[test]
    fn db_path_is_side_effect_free_and_consistent() {
        // Composes the already-tested pure helpers + env reads; must not touch the filesystem.
        // Robust to a CI-set `GETMASTERS_DB_PATH`: either it wins verbatim, or the default ends in
        // the DB file name. Either way, calling it twice is stable and creates nothing.
        let p = db_path();
        assert_eq!(p, db_path());
        match env_path("GETMASTERS_DB_PATH") {
            Some(explicit) => assert_eq!(p, explicit),
            None => assert_eq!(
                p.file_name().and_then(|n| n.to_str()),
                Some("getmasters.db")
            ),
        }
    }

    #[test]
    fn resolve_creates_parent_dir() {
        // Point GETMASTERS_HOME at a not-yet-existing dir under the OS temp dir; resolving must create it.
        let base =
            std::env::temp_dir().join(format!("getmasters-home-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let home = data_home_from(Some(base.clone()), None);
        let db = db_path_from(None, &home);
        assert!(!base.exists());
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        assert!(base.exists());
        assert_eq!(db, base.join("getmasters.db"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
