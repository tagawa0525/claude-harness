//! pre-pr-create-check が、検査に通った gh pr create に、PR を作った後の次のステップを additionalContext で添えること

mod common;

use common::forget_git_env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// GitHub の URL の remote（github）と、push できる origin（bare）を持ち、feat/x を push 済みの clone
struct Repo {
    root: PathBuf,
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Repo {
    fn new() -> Repo {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "pre-pr-create-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app")).unwrap();
        let repo = Repo { root };
        let bare = repo.root.join("origin.git");
        repo.git(&["init", "-q", "--bare", bare.to_str().unwrap()]);
        repo.git(&["init", "-q", "-b", "main"]);
        std::fs::write(repo.app().join("README.md"), "init\n").unwrap();
        repo.git(&["add", "README.md"]);
        repo.git(&["commit", "-q", "-m", "init"]);
        repo.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
        repo.git(&[
            "remote",
            "add",
            "github",
            "https://github.com/example/app.git",
        ]);
        repo.git(&["push", "-q", "origin", "main"]);
        repo.git(&["switch", "-q", "-c", "feat/x"]);
        std::fs::write(repo.app().join("a.txt"), "a\n").unwrap();
        repo.git(&["add", "a.txt"]);
        repo.git(&["commit", "-q", "-m", "a"]);
        repo.git(&["push", "-q", "-u", "origin", "feat/x"]);
        repo
    }

    fn app(&self) -> PathBuf {
        self.root.join("app")
    }

    fn git(&self, args: &[&str]) -> String {
        let out = forget_git_env(&mut Command::new("git"))
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(self.app())
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// command を pre-pr-create-check だけに掛け、stdout の JSON（空なら Null）を返す
    fn hook(&self, command: &str) -> serde_json::Value {
        let input = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": command},
            "cwd": self.app(),
        });
        let mut child = forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["pre-tool-use", "--rule", "pre-pr-create-check"])
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        let stdout = String::from_utf8(out.stdout).unwrap();
        if stdout.trim().is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&stdout).unwrap()
        }
    }
}

const CREATE: &str =
    "gh pr create --title 'feat: a' --body '## Summary\nx\n## Changes\n- a\n## Tests\n- t'";

#[test]
fn a_passing_pr_create_gets_the_next_steps_after_the_pr() {
    let repo = Repo::new();
    let out = repo.hook(CREATE);
    let o = &out["hookSpecificOutput"];
    assert!(o.get("permissionDecision").is_none(), "{out}");
    let context = o["additionalContext"].as_str().unwrap_or_default();
    for step in [
        "gh pr create が出力した URL",
        "~/.claude/scripts/gh-wait-review.sh <PR の URL>",
        "run_in_background=true",
        "/gh-actions-check",
        "/gh-pr-review",
        "/gh-pr-merge",
        "/git-info",
    ] {
        assert!(context.contains(step), "{step}: {context}");
    }
}

#[test]
fn a_denied_pr_create_gets_no_next_steps() {
    let repo = Repo::new();
    let out = repo.hook("gh pr create --title 'feat: a' --body 'x'");
    let o = &out["hookSpecificOutput"];
    assert_eq!(o["permissionDecision"], "deny", "{out}");
    assert!(o.get("additionalContext").is_none(), "{out}");
}

#[test]
fn other_gh_commands_get_no_next_steps() {
    let repo = Repo::new();
    assert!(repo.hook("gh pr view --json url").is_null());
}
