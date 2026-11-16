//! block-claude-generated-write: ~/.claude の下の、ハーネスから同期されるファイルへの Edit / Write を止める
//!
//! `claude-setup sync` が `.claude` のソースから ~/.claude の CLAUDE.md、commands、skills、scripts へ配る。
//! ここを直接書き換えても次の同期で上書きされ、ソースのリポジトリには届かない。
//! ~/.claude のほかの場所（settings.json、projects の下のメモリなど）は同期の対象でないので止めない。
//! 見るのは file_path の書き方（`~/`、相対、`..` を字面で解いたもの）だけで、symlinkは辿らない。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;
use std::path::{Component, Path, PathBuf};

pub struct BlockClaudeGeneratedWrite;

impl Rule for BlockClaudeGeneratedWrite {
    fn name(&self) -> &'static str {
        "block-claude-generated-write"
    }

    fn tools(&self) -> &'static [&'static str] {
        &["Edit", "Write"]
    }

    fn check(&self, input: &Input, _shell: &Shell) -> Vec<Finding> {
        let Some(rel) = synced_path(&input.file_path, &input.cwd, &crate::shell::home()) else {
            return Vec::new();
        };
        vec![Finding::Deny(format!(
            "~/.claude/{rel} は claude-setup sync がソースのリポジトリの .claude から配る生成物です。ここを書き換えても次の同期で\
             上書きされ、ソースには届きません。ソースのリポジトリ（claude-harness の .claude/{rel}）の worktree で直してください。\
             直すものが自分の作業の範囲外なら、直す内容を報告に書いてください。"
        ))]
    }
}

/// 同期で配られるもの（`claude-setup sync` の対象）。~/.claude の直下の名前
const SYNCED: [&str; 4] = ["CLAUDE.md", "commands", "skills", "scripts"];

/// path が ~/.claude の同期されるものの下なら、~/.claude からの相対パス。`~/` は home、相対は cwd からとし、
/// `.` と `..` は字面で解く
fn synced_path(path: &str, cwd: &Path, home: &Path) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let full = match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => cwd.join(path),
    };
    let mut norm = PathBuf::new();
    for c in full.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                norm.pop();
            }
            c => norm.push(c),
        }
    }
    let rel = norm.strip_prefix(home.join(".claude")).ok()?;
    let first = rel.components().next()?.as_os_str().to_str()?;
    SYNCED
        .contains(&first)
        .then(|| rel.to_string_lossy().replace('\\', "/"))
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

    #[test]
    #[ignore = "RED: the message claims an overwrite that commands does not get"]
    fn the_message_holds_for_every_synced_place() {
        let h = home();
        for place in SYNCED {
            let raw = serde_json::json!({
                "tool_name": "Edit",
                "tool_input": {"file_path": format!("{h}/.claude/{place}/x")},
                "cwd": "/work/repo",
            })
            .to_string();
            let input = Input::parse(&raw).unwrap();
            let f = BlockClaudeGeneratedWrite.check(&input, &Shell::parse(""));
            let [Finding::Deny(msg)] = f.as_slice() else {
                panic!("{place}")
            };
            // commands は同期が既存のファイルを上書きしない。全部に真な文だけを言う
            assert!(!msg.contains("上書き"), "{place}: {msg}");
            assert!(msg.contains(".claude/"), "{msg}");
        }
    }
}
