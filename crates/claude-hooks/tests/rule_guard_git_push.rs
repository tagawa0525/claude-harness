//! guard-git-push が main / master への push を止めるとき、コミットを feature ブランチに移す修正方法を示すこと

mod common;

use common::forget_git_env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// GitHub の origin を持ち、main にいるリポジトリ
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
            "guard-git-push-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["commit", "-q", "--allow-empty", "-m", "init"]);
        repo.git(&[
            "remote",
            "add",
            "origin",
            "https://github.com/example/app.git",
        ]);
        repo
    }

    fn git(&self, args: &[&str]) {
        let out = forget_git_env(&mut Command::new("git"))
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(&self.root)
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

    /// command を guard-git-push だけに掛け、止めた理由を返す（止めなければ None）
    fn deny_reason(&self, command: &str) -> Option<String> {
        let input = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": command},
            "cwd": self.root,
        });
        let mut child = forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["pre-tool-use", "--rule", "guard-git-push"])
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
            return None;
        }
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        json["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .map(str::to_string)
    }
}

#[test]
fn a_push_to_main_shows_how_to_move_the_commits_to_a_feature_branch() {
    let repo = Repo::new();
    for command in [
        "git push",
        "git push origin main",
        "git push origin HEAD:master",
    ] {
        let reason = repo
            .deny_reason(command)
            .unwrap_or_else(|| panic!("{command} が止まらない"));
        assert!(
            reason.contains("/git-cherry-pick") && reason.contains("/gh-pr-create"),
            "{command}: {reason}"
        );
        let cherry = reason.find("/git-cherry-pick").unwrap();
        assert!(
            cherry < reason.find("/gh-pr-create").unwrap(),
            "{command}: {reason}"
        );
    }
}

#[test]
fn a_push_from_a_feature_branch_to_main_is_not_told_to_move_commits() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    // refspec で main を指す: コミットは main に無いので、push 先を直す
    let reason = repo.deny_reason("git push origin feat/x:main").unwrap();
    assert!(!reason.contains("/git-cherry-pick"), "{reason}");
    assert!(reason.contains("git push origin feat/x"), "{reason}");
    // 上流が main: 上流を直す
    repo.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    repo.git(&["branch", "-q", "--set-upstream-to=origin/main", "feat/x"]);
    let reason = repo.deny_reason("git push").unwrap();
    assert!(!reason.contains("/git-cherry-pick"), "{reason}");
    assert!(reason.contains("git push -u origin feat/x"), "{reason}");
}

#[test]
fn a_push_of_local_main_from_a_feature_branch_is_told_to_move_the_commits() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    // 送る元は main（コミットは main にある）
    let reason = repo.deny_reason("git push origin main").unwrap();
    assert!(reason.contains("/git-cherry-pick"), "{reason}");
    // 送る元がほかのブランチなら、そのブランチを feature branch の名前で送る
    let reason = repo.deny_reason("git push origin other:main").unwrap();
    assert!(!reason.contains("/git-cherry-pick"), "{reason}");
    assert!(reason.contains("git push origin other:"), "{reason}");
}

#[test]
fn deleting_main_is_not_told_to_move_commits() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    let reason = repo.deny_reason("git push origin :main").unwrap();
    assert!(!reason.contains("/git-cherry-pick"), "{reason}");
    assert!(reason.contains("消す push"), "{reason}");
}
