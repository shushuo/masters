//! Folder grants and path containment (docs/06 §1–2).
//!
//! `GrantSet` is the authoritative check the Permission gate uses to keep file tools inside
//! granted folders. Resolution canonicalizes paths, so `..` traversal and symlink escapes
//! that leave a grant root are rejected.

use std::path::{Path, PathBuf};

use getmasters_proto::FolderGrant;

/// The set of folders the agent may act within for a session.
#[derive(Clone, Debug, Default)]
pub struct GrantSet {
    grants: Vec<FolderGrant>,
}

impl GrantSet {
    pub fn new(grants: Vec<FolderGrant>) -> Self {
        Self { grants }
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.grants.is_empty()
    }

    pub fn grants(&self) -> &[FolderGrant] {
        &self.grants
    }

    /// Resolve `path` to a canonical target that lies within a grant with sufficient access.
    /// Returns the canonical path on success, or a human-readable reason on denial.
    pub fn resolve(&self, path: &str, need_write: bool) -> Result<PathBuf, String> {
        if self.grants.is_empty() {
            return Err("no folder grants are configured".to_string());
        }
        let target = canonical_target(Path::new(path))
            .map_err(|e| format!("cannot resolve path '{path}': {e}"))?;

        for g in &self.grants {
            let Ok(root) = Path::new(&g.path).canonicalize() else {
                continue;
            };
            if target.starts_with(&root) {
                if need_write && !g.access.allows_write() {
                    return Err(format!("'{path}' is inside a read-only grant"));
                }
                return Ok(target);
            }
        }
        Err(format!("'{path}' is outside any granted folder"))
    }
}

/// Canonicalize a path that may not exist yet (e.g. a file about to be created):
/// canonicalize the path if it exists, otherwise canonicalize its parent and re-join the name.
fn canonical_target(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        return path.canonicalize();
    }
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let cparent = parent.canonicalize()?;
    Ok(cparent.join(path.file_name().unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use getmasters_proto::FolderAccess;

    fn grant(dir: &std::path::Path, access: FolderAccess) -> FolderGrant {
        FolderGrant {
            id: "g".into(),
            project_id: None,
            path: dir.to_string_lossy().into_owned(),
            access,
            created_at: 0,
        }
    }

    #[test]
    fn allows_inside_grant_and_rejects_outside() {
        let dir = tempdir();
        let gs = GrantSet::new(vec![grant(&dir, FolderAccess::ReadWrite)]);
        // A new file under the grant resolves (write).
        let inside = dir.join("note.txt");
        assert!(gs.resolve(inside.to_str().unwrap(), true).is_ok());
        // Traversal outside the grant is rejected.
        let outside = dir.join("../escape.txt");
        assert!(gs.resolve(outside.to_str().unwrap(), true).is_err());
    }

    #[test]
    fn read_only_grant_blocks_writes() {
        let dir = tempdir();
        let gs = GrantSet::new(vec![grant(&dir, FolderAccess::Read)]);
        let f = dir.join("a.txt");
        assert!(gs.resolve(f.to_str().unwrap(), false).is_ok());
        assert!(gs.resolve(f.to_str().unwrap(), true).is_err());
    }

    #[test]
    fn empty_grantset_denies() {
        let gs = GrantSet::empty();
        assert!(gs.resolve("/tmp/x", false).is_err());
    }

    /// Minimal unique temp dir without an extra dependency.
    fn tempdir() -> PathBuf {
        let base = std::env::temp_dir();
        let mut p = base.join(format!("getmasters-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p = p.canonicalize().unwrap();
        p
    }
}
