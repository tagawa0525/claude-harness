//! block-claude-generated-write: ~/.claude の下の、ハーネスから同期されるファイルへの Edit / Write を止める
//!
//! `claude-setup sync` が `.claude` のソースから ~/.claude の CLAUDE.md、commands、skills、scripts へ配る。
//! ここを直接書き換えても次の同期で上書きされ、ソースのリポジトリには届かない。
//! ~/.claude の ほかの場所（settings.json、projects の下のメモリなど）は同期の対象でないので止めない。
//! 見るのは file_path の書き方（`~/`、相対、`..` を字面で解いたもの）だけで、symlinkは辿らない。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct BlockClaudeGeneratedWrite;

impl Rule for BlockClaudeGeneratedWrite {
    fn name(&self) -> &'static str {
        "block-claude-generated-write"
    }

    fn tools(&self) -> &'static [&'static str] {
        &["Edit", "Write"]
    }

    fn check(&self, input: &Input, _shell: &Shell) -> Vec<Finding> {
        let _ = &input.file_path;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deny(path: &str) -> bool {
        let raw = serde_json::json!({
            "tool_name": "Edit",
            "tool_input": {"file_path": path},
            "cwd": "/work/repo",
        })
        .to_string();
        let input = Input::parse(&raw).unwrap();
        !BlockClaudeGeneratedWrite
            .check(&input, &Shell::parse(""))
            .is_empty()
    }

    fn home() -> String {
        crate::shell::home().display().to_string()
    }

    #[test]
    #[ignore = "RED: the rule is not implemented yet"]
    fn synced_paths_under_the_claude_directory_are_denied() {
        let h = home();
        for p in [
            format!("{h}/.claude/skills/delegate/SKILL.md"),
            format!("{h}/.claude/scripts/gh-wait-review.sh"),
            format!("{h}/.claude/CLAUDE.md"),
            format!("{h}/.claude/commands/x.md"),
            "~/.claude/skills/x/SKILL.md".to_string(),
            format!("{h}/.claude/skills/../scripts/a.sh"),
            format!("{h}/.claude/bin/../CLAUDE.md"),
        ] {
            assert!(deny(&p), "{p}");
        }
    }

    #[test]
    fn other_paths_are_allowed() {
        let h = home();
        for p in [
            format!("{h}/.claude/settings.json"),
            format!("{h}/.claude/projects/-x/memory/a.md"),
            format!("{h}/.claude/plans/p.md"),
            format!("{h}/.claude-other/skills/a.md"),
            format!("{h}/nix/claude-harness/.claude/skills/a/SKILL.md"),
            "/work/repo/.claude/skills/a.md".to_string(),
            "relative/skills/a.md".to_string(),
            String::new(),
        ] {
            assert!(!deny(&p), "{p}");
        }
    }
}
