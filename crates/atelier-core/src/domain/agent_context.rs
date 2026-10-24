use super::files::validate_path;

pub const FILE_LIMIT: usize = 1024 * 1024;
pub const TOTAL_LIMIT: usize = 8 * FILE_LIMIT;
pub const COUNT_LIMIT: usize = 2000;

pub fn allowed(path: &str) -> bool {
    if validate_path(path, true).is_err() || path.contains(['\n', '\r']) {
        return false;
    }
    if matches!(
        path,
        "CLAUDE.md" | "CLAUDE.local.md" | "AGENTS.md" | "AGENTS.override.md" | ".mcp.json"
    ) {
        return true;
    }
    let Some(relative) = path.strip_prefix(".claude/") else {
        return false;
    };
    if matches!(
        relative,
        "settings.json" | "settings.local.json" | "CLAUDE.md" | "AGENTS.md"
    ) {
        return true;
    }
    relative.split_once('/').is_some_and(|(directory, _)| {
        matches!(
            directory,
            "skills" | "agents" | "commands" | "rules" | "hooks" | "plugins"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_explicit_agent_context_is_copied() {
        for path in [
            "AGENTS.md",
            "AGENTS.override.md",
            "CLAUDE.local.md",
            ".mcp.json",
            ".claude/settings.local.json",
            ".claude/skills/review/SKILL.md",
            ".claude/hooks/check.mjs",
        ] {
            assert!(allowed(path), "{path}");
        }
        for path in [
            ".env",
            ".claude/worktrees/other/private.txt",
            ".git/config",
            ".claude/skills/../secret",
            ".claude/skills/.git/config",
            ".claude/skills/new\nline",
            ".codex/auth.json",
            "other/AGENTS.md",
        ] {
            assert!(!allowed(path), "{path}");
        }
    }
}
