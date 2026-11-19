//! guard-memory-bash-write: 自動メモリのディレクトリのファイルを Bash で書き換えるコマンドを止める
//!
//! エスケープ: コマンドに `ALLOW_MEMORY_BASH_WRITE=<理由>` を付ける。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;
use std::path::{Path, PathBuf};

pub struct GuardMemoryBashWrite;

impl Rule for GuardMemoryBashWrite {
    fn name(&self) -> &'static str {
        "guard-memory-bash-write"
    }

    fn escape(&self) -> Option<&'static str> {
        Some("ALLOW_MEMORY_BASH_WRITE")
    }

    fn check(&self, _input: &Input, _shell: &Shell) -> Vec<Finding> {
        Vec::new()
    }
}

/// shell のコマンドが書き込み先にする、memory のディレクトリの下のパス（文書順）
fn memory_writes(_shell: &Shell, _cwd: &Path, _home: &Path, _config: Option<&str>) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEM: &str = "~/.claude/projects/-work-repo/memory";

    fn writes_with(src: &str, config: Option<&str>) -> Vec<PathBuf> {
        let home = crate::shell::home();
        memory_writes(&Shell::parse(src), Path::new("/work/repo"), &home, config)
    }

    fn denied(src: &str) -> bool {
        !writes_with(src, None).is_empty()
    }

    #[test]
    fn writes_into_the_memory_directory_are_denied() {
        for src in [
            format!("echo x > {MEM}/MEMORY.md"),
            format!("echo x >> {MEM}/MEMORY.md"),
            format!("cat > {MEM}/a.md <<'EOF'\nx\nEOF"),
            format!("echo x | tee {MEM}/a.md"),
            format!("echo x | tee -a /tmp/log {MEM}/a.md"),
            format!("sed -i 's/a/b/' {MEM}/MEMORY.md"),
            format!("sed -i.bak -e 's/a/b/' {MEM}/MEMORY.md"),
            format!("sed -Ei 's/a/b/' {MEM}/MEMORY.md"),
            format!("sed --in-place 's/a/b/' {MEM}/MEMORY.md"),
            format!("perl -pi -e 's/a/b/' {MEM}/MEMORY.md"),
            format!("perl -i -pe 's/a/b/' {MEM}/MEMORY.md"),
            format!("cp /tmp/a.md {MEM}/a.md"),
            format!("cp /tmp/a.md {MEM}/"),
            format!("cp -t {MEM} /tmp/a.md"),
            format!("mv /tmp/a.md {MEM}/a.md"),
            format!("mv --target-directory={MEM} /tmp/a.md"),
            format!("install -m 644 /tmp/a.md {MEM}/a.md"),
            format!("rm {MEM}/a.md"),
            format!("rm -f -- {MEM}/a.md"),
            format!("rm -r {MEM}"),
            format!("truncate -s 0 {MEM}/a.md"),
            format!("cd {MEM} && echo x >> MEMORY.md"),
            format!("cd {MEM} && sed -i 's/a/b/' MEMORY.md"),
            "cd ~/.claude/projects/-work-repo && rm memory/a.md".to_string(),
            format!("bash -c 'echo x >> {MEM}/MEMORY.md'"),
            "echo x >> $HOME/.claude/projects/-work-repo/memory/MEMORY.md".to_string(),
            "echo x >> \"${HOME}/.claude/projects/-work-repo/memory/MEMORY.md\"".to_string(),
            "d=~/.claude/projects/-work-repo/memory; echo x >> $d/MEMORY.md".to_string(),
            format!("echo x >> {MEM}/../memory/./MEMORY.md"),
            format!("/usr/bin/sed -i 's/a/b/' {MEM}/MEMORY.md"),
        ] {
            assert!(denied(&src), "{src}");
        }
    }

    #[test]
    fn paths_outside_the_memory_directory_are_allowed() {
        for src in [
            "echo x >> ~/.claude/projects/-work-repo/notes.md",
            "echo x >> ~/.claude/projects/-work-repo/a.jsonl",
            "echo x >> /work/repo/memory/MEMORY.md",
            "echo x >> memory/MEMORY.md",
            "echo x >> ~/.claude/memory/MEMORY.md",
            "echo x >> ~/.claude/projects/memory/a.md",
            "cp ~/.claude/projects/-work-repo/memory/a.md /tmp/a.md",
            "mv ~/.claude/projects/-work-repo/memory/a.md /tmp/",
            "sed 's/a/b/' ~/.claude/projects/-work-repo/memory/MEMORY.md > /tmp/out",
            "perl -pe 's/a/b/' ~/.claude/projects/-work-repo/memory/MEMORY.md",
            "install -d ~/.claude/projects/-work-repo/memory",
            "echo x >> ~/.claude/projects/-work-repo/memory/../notes.md",
            "echo x >> $UNKNOWN/MEMORY.md",
            "ssh r995 'echo x >> ~/.claude/projects/-work-repo/memory/MEMORY.md'",
        ] {
            assert!(!denied(src), "{src}");
        }
    }

    #[test]
    fn read_only_commands_are_allowed() {
        for src in [
            format!("cat {MEM}/MEMORY.md"),
            format!("grep -n x {MEM}/*.md"),
            format!("ls -la {MEM}"),
            format!("~/.claude/skills/retro/scripts/memory-index.sh {MEM}"),
            format!("wc -l < {MEM}/MEMORY.md"),
            format!("cd {MEM} && cat MEMORY.md"),
            format!("cat {MEM}/MEMORY.md 2>/dev/null > /tmp/copy"),
            format!("sed -n 1,5p {MEM}/MEMORY.md"),
            format!("cd {MEM} && sed -n 1p MEMORY.md 2>&1"),
        ] {
            assert!(!denied(&src), "{src}");
        }
    }

    #[test]
    fn a_moved_config_directory_holds_the_memory() {
        let home = crate::shell::home();
        let config = home.join("cfg").display().to_string();
        let config = Some(config.as_str());
        assert!(!writes_with("echo x >> ~/cfg/projects/-p/memory/a.md", config).is_empty());
        assert!(
            !writes_with(
                "echo x >> $CLAUDE_CONFIG_DIR/projects/-p/memory/a.md",
                config
            )
            .is_empty()
        );
        assert!(writes_with(&format!("echo x >> {MEM}/a.md"), config).is_empty());
    }

    #[test]
    fn the_message_names_the_path_and_the_tools_to_use() {
        let raw = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": format!("sed -i 's/a/b/' {MEM}/MEMORY.md")},
            "cwd": "/work/repo",
        })
        .to_string();
        let input = Input::parse(&raw).unwrap();
        let f = GuardMemoryBashWrite.check(&input, &Shell::parse(&input.command));
        let [Finding::Deny(msg)] = f.as_slice() else {
            panic!("{} 件", f.len())
        };
        assert!(msg.contains("MEMORY.md"), "{msg}");
        assert!(msg.contains("Read"), "{msg}");
        assert!(msg.contains("Write") && msg.contains("Edit"), "{msg}");
        assert!(msg.contains("ALLOW_MEMORY_BASH_WRITE=<理由>"), "{msg}");
    }
}
