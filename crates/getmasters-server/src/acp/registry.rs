//! Known ACP coding-harness registry + machine detection (Phase 4i, ADR-0014).
//!
//! A small, static list of the **coding** agents Masters knows how to drive over ACP. `detect`
//! probes the current `PATH` for each entry's launch command so the desktop can offer one-click
//! registration of a pre-installed harness as an external master agent. Detection never spawns the
//! agent — it only checks for the executable — and never auto-creates a master (the user names and
//! registers it). Adding a harness later is one entry here.

use std::path::Path;

use getmasters_proto::AvailableHarnessDto;

/// One known coding harness. `command` is what we detect on `PATH`; `suggested_command`/`args` are
/// what we prefill when the user registers it (may be an `npx` invocation when no native bin exists).
struct KnownHarness {
    id: &'static str,
    display_name: &'static str,
    command: &'static str,
    suggested_command: &'static str,
    suggested_args: &'static [&'static str],
    homepage: &'static str,
}

/// The supported coding harnesses (coding agents only — general assistants are out of scope).
const KNOWN: &[KnownHarness] = &[
    KnownHarness {
        id: "claude-code",
        display_name: "Claude Code",
        command: "claude-code-acp",
        suggested_command: "claude-code-acp",
        suggested_args: &[],
        homepage: "https://www.npmjs.com/package/@zed-industries/claude-code-acp",
    },
    KnownHarness {
        id: "codex",
        display_name: "Codex",
        command: "codex-acp",
        suggested_command: "codex-acp",
        suggested_args: &[],
        homepage: "https://github.com/cola-io/codex-acp",
    },
    KnownHarness {
        id: "opencode",
        display_name: "OpenCode",
        command: "opencode",
        suggested_command: "opencode",
        suggested_args: &["acp"],
        homepage: "https://opencode.ai",
    },
    KnownHarness {
        id: "gemini",
        display_name: "Gemini CLI",
        command: "gemini",
        suggested_command: "gemini",
        suggested_args: &["--experimental-acp"],
        homepage: "https://github.com/google-gemini/gemini-cli",
    },
];

/// Whether `command` resolves to an executable file on the current `PATH`. A command containing a
/// path separator is checked directly. Pure `PATH` walk — no new dependency, no subprocess.
fn on_path(command: &str) -> bool {
    if command.contains('/') {
        return Path::new(command).is_file();
    }
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(command).is_file())
}

/// Detect which known coding harnesses are installed on this machine.
pub fn detect() -> Vec<AvailableHarnessDto> {
    KNOWN
        .iter()
        .map(|h| AvailableHarnessDto {
            id: h.id.to_string(),
            display_name: h.display_name.to_string(),
            command: h.command.to_string(),
            available: on_path(h.command),
            suggested_command: h.suggested_command.to_string(),
            suggested_args: h.suggested_args.iter().map(|s| s.to_string()).collect(),
            homepage: h.homepage.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_lists_every_known_harness() {
        let found = detect();
        assert_eq!(found.len(), KNOWN.len());
        assert!(found.iter().any(|h| h.id == "claude-code"));
    }

    #[test]
    fn absent_command_is_unavailable() {
        assert!(!on_path("definitely-not-a-real-harness-xyz"));
    }

    #[test]
    fn present_command_is_detected() {
        // `sh` exists on any POSIX test host; prove the PATH walk resolves a real binary.
        assert!(on_path("sh"));
    }
}
