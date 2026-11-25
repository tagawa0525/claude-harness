//! block-main-commit が、同じコマンドの中で先にブランチを移ってからの git commit を、コミットの時点のブランチで
//! 判定すること

mod common;

use common::{forget_git_env, sh_path};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// main にいて、ファイルを 1 つステージしたリポジトリ（repo）と、その親のディレクトリ（hook の cwd）
struct Repo {
    root: PathBuf,
    repo: PathBuf,
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
            "block-main-commit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let r = Repo { root, repo };
        r.git(&["init", "-q", "-b", "main"]);
        r.git(&["commit", "-q", "--allow-empty", "-m", "init"]);
        std::fs::write(r.repo.join("a.txt"), "a\n").unwrap();
        r.git(&["add", "a.txt"]);
        r
    }

    fn git(&self, args: &[&str]) {
        let out = forget_git_env(&mut Command::new("git"))
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(&self.repo)
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// `cd <repo> && <rest>` を、repo の外（親のディレクトリ）を cwd にして block-main-commit だけに掛け、止めたか
    fn denied(&self, rest: &str) -> bool {
        let command = format!("cd '{}' && {rest}", sh_path(&self.repo));
        let input = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": command},
            "cwd": self.root,
        });
        let mut child = forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["pre-tool-use", "--rule", "block-main-commit"])
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CEILING_DIRECTORIES", &self.root)
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
            return false;
        }
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        json["hookSpecificOutput"]["permissionDecision"] == "deny"
    }
}

#[test]
fn a_commit_on_main_without_a_switch_is_denied() {
    let repo = Repo::new();
    assert!(repo.denied("git commit -m x"));
}

#[test]
#[ignore = "RED: 実装の前"]
fn a_commit_after_creating_a_feature_branch_in_the_same_command_is_allowed() {
    let repo = Repo::new();
    for rest in [
        "git switch -c feat/x && git commit -m x",
        "git checkout -b feat/x && git commit -m x",
    ] {
        assert!(!repo.denied(rest), "{rest}");
    }
}

#[test]
#[ignore = "RED: 実装の前"]
fn a_commit_after_switching_to_an_existing_feature_branch_in_the_same_command_is_allowed() {
    let repo = Repo::new();
    repo.git(&["branch", "feat/x"]);
    assert!(!repo.denied("git switch feat/x && git add a.txt && git commit -m x"));
}

#[test]
#[ignore = "RED: 実装の前"]
fn a_commit_after_switching_to_main_in_the_same_command_is_denied() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    for rest in [
        "git switch main && git commit -m x",
        "git checkout main && git commit -m x",
    ] {
        assert!(repo.denied(rest), "{rest}");
    }
}

/// 移り先の分からない switch（`-`、`--detach`）の後は、今の HEAD（main）で判定する
#[test]
fn a_commit_after_a_switch_to_an_unknown_target_is_judged_on_the_current_head() {
    let repo = Repo::new();
    repo.git(&["branch", "feat/x"]);
    for rest in [
        "git switch - && git commit -m x",
        "git switch --detach && git commit -m x",
        "git switch feat/x && git switch - && git commit -m x",
    ] {
        assert!(repo.denied(rest), "{rest}");
    }
}
