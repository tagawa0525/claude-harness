//! セッションの scratchpad の下に `git init` で作った一時のリポジトリか（guard-git-add と block-no-verify の例外。ADR-0072）
//!
//! scratchpad は `<Claude Code の一時ディレクトリ>/<プロジェクト>/<セッションの ID>/scratchpad`。一時ディレクトリの
//! 根は環境変数（`CLAUDE_CODE_TMPDIR` など）で変わるので、根でなく `<セッションの ID>/scratchpad` の 2 つの
//! 名前で見分ける。

use crate::input::Input;
use crate::shell::{Cmd, Shell};

/// cmd の対象のリポジトリが、セッションの scratchpad の下に作った、リモートの無い一時のリポジトリなら Ok。
/// 当たらなければ、当たらない理由
pub fn check(_input: &Input, _shell: &Shell, _cmd: &Cmd) -> Result<(), String> {
    Err("未実装".to_string())
}

/// ルールのテストが共有する、scratchpad と本番のリポジトリの代わりを持つ一時のディレクトリ
#[cfg(test)]
pub(crate) mod fixture {
    use crate::input::Input;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    pub const SID: &str = "0f0e0d0c-1111-2222-3333-444455556666";

    /// テストの間だけある一時のディレクトリ。テストは並行に走るので、呼ぶたびに別のディレクトリにする
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "claude-hooks-scratchpad-{tag}-{}-{seq}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 手元の設定（グローバルの core.hooksPath など）を読まずに git を実行する
    pub fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// `<root>/proj/<SID>/scratchpad`（scratchpad）と、その外の `<root>/prod`（本番のリポジトリの代わり。1 コミット）
    pub struct Fixture {
        root: TempDir,
    }

    impl Fixture {
        pub fn new(tag: &str) -> Fixture {
            let f = Fixture {
                root: TempDir::new(tag),
            };
            std::fs::create_dir_all(f.scratchpad()).unwrap();
            let prod = f.prod();
            std::fs::create_dir_all(&prod).unwrap();
            git(&prod, &["init", "-q", "-b", "main"]);
            std::fs::write(prod.join("a.txt"), "a\n").unwrap();
            git(&prod, &["add", "a.txt"]);
            git(&prod, &["commit", "-q", "-m", "init"]);
            f
        }

        pub fn scratchpad(&self) -> PathBuf {
            self.root.0.join("proj").join(SID).join("scratchpad")
        }

        pub fn prod(&self) -> PathBuf {
            self.root.0.join("prod")
        }

        /// scratchpad の下に git init で作ったリポジトリ
        pub fn temp_repo(&self, name: &str) -> PathBuf {
            let dir = self.scratchpad().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            git(&dir, &["init", "-q"]);
            dir
        }
    }

    pub fn input(cwd: &Path, sid: &str) -> Input {
        let json = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": ""},
            "cwd": cwd.to_str().unwrap(),
            "session_id": sid,
        });
        Input::parse(&json.to_string()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use super::fixture::{Fixture, SID, git, input};
    use crate::shell::Shell;
    use std::path::Path;

    /// src の最初の git のコマンドを、cwd とセッションの ID で判定する
    fn judge_with(src: &str, cwd: &Path, sid: &str) -> Result<(), String> {
        let shell = Shell::parse(src);
        let cmd = shell
            .commands()
            .iter()
            .find(|c| c.git_subcommand().is_some())
            .expect("git のコマンドが無い");
        check(&input(cwd, sid), &shell, cmd)
    }

    fn judge(src: &str, cwd: &Path) -> Result<(), String> {
        judge_with(src, cwd, SID)
    }

    #[test]
    fn temp_repo_under_scratchpad_is_excepted_via_cd_dash_c_and_cwd() {
        let f = Fixture::new("cd-c-cwd");
        let repo = f.temp_repo("r");
        let elsewhere = f.prod();
        let r = repo.display();
        assert_eq!(judge(&format!("cd {r} && git add -A"), &elsewhere), Ok(()));
        assert_eq!(judge(&format!("git -C {r} add ."), &elsewhere), Ok(()));
        assert_eq!(judge("git add .", &repo), Ok(()));
        // サブディレクトリから
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        assert_eq!(judge("cd sub && git add -A", &repo), Ok(()));
    }

    #[test]
    fn directory_made_in_the_same_command_under_scratchpad_is_excepted() {
        let f = Fixture::new("mkdir");
        let r = f.scratchpad().join("new");
        let src = format!(
            "mkdir -p {r} && cd {r} && git init -q && git add -A",
            r = r.display()
        );
        assert_eq!(judge(&src, &f.prod()), Ok(()));
    }

    #[test]
    fn repository_outside_scratchpad_is_not_excepted() {
        let f = Fixture::new("outside");
        let prod = f.prod();
        let err = judge("git add -A", &prod).unwrap_err();
        assert!(err.contains("scratchpad の外"), "{err}");
        let err = judge(&format!("git -C {} add .", prod.display()), &f.scratchpad()).unwrap_err();
        assert!(err.contains("scratchpad の外"), "{err}");
    }

    #[test]
    fn worktree_of_a_real_repository_under_scratchpad_is_not_excepted() {
        let f = Fixture::new("worktree");
        let wt = f.scratchpad().join("wt");
        git(&f.prod(), &["worktree", "add", "-q", wt.to_str().unwrap()]);
        let err = judge("git add -A", &wt).unwrap_err();
        assert!(err.contains("scratchpad の外"), "{err}");
    }

    #[test]
    fn clone_with_a_remote_under_scratchpad_is_not_excepted() {
        let f = Fixture::new("clone");
        let clone = f.scratchpad().join("clone");
        git(
            &f.scratchpad(),
            &[
                "clone",
                "-q",
                "--shared",
                f.prod().to_str().unwrap(),
                "clone",
            ],
        );
        let err = judge("git add -A", &clone).unwrap_err();
        assert!(err.contains("リモート"), "{err}");
    }

    #[test]
    fn dot_dot_and_symlinks_do_not_leave_scratchpad_unnoticed() {
        let f = Fixture::new("escape");
        let repo = f.temp_repo("r");
        // `..` で scratchpad から出る（存在するパス）
        let up = repo.join("../../../../prod");
        let err = judge(&format!("git -C {} add .", up.display()), &repo).unwrap_err();
        assert!(err.contains("scratchpad の外"), "{err}");
        // まだ無いディレクトリの後の `..`
        let missing_up = f.scratchpad().join("missing/../../../../prod");
        let err = judge(&format!("cd {} && git add .", missing_up.display()), &repo).unwrap_err();
        assert!(err.contains(".."), "{err}");
        // scratchpad の中の symlink が本番のリポジトリを指す
        let link = f.scratchpad().join("link");
        std::os::unix::fs::symlink(f.prod(), &link).unwrap();
        let err = judge(&format!("cd {} && git add -A", link.display()), &repo).unwrap_err();
        assert!(err.contains("scratchpad の外"), "{err}");
    }

    #[test]
    fn unexpanded_path_is_not_excepted() {
        let f = Fixture::new("expansion");
        let repo = f.temp_repo("r");
        let err = judge("cd \"$D\" && git add -A", &repo).unwrap_err();
        assert!(err.contains("展開"), "{err}");
    }

    #[test]
    fn another_session_or_missing_session_id_is_not_excepted() {
        let f = Fixture::new("session");
        let repo = f.temp_repo("r");
        let err = judge_with("git add -A", &repo, "other-session").unwrap_err();
        assert!(err.contains("scratchpad の外"), "{err}");
        let err = judge_with("git add -A", &repo, "").unwrap_err();
        assert!(err.contains("セッションの ID"), "{err}");
    }

    #[test]
    fn git_dir_redirection_is_not_excepted() {
        let f = Fixture::new("gitdir");
        let repo = f.temp_repo("r");
        let prod_git = f.prod().join(".git");
        let g = prod_git.display();
        for src in [
            format!("GIT_DIR={g} git add -A"),
            format!("git --git-dir={g} add -A"),
            format!("git --git-dir {g} add -A"),
            format!("export GIT_WORK_TREE={g} && git add -A"),
        ] {
            let err = judge(&src, &repo).unwrap_err();
            assert!(err.contains("GIT_DIR"), "{src}: {err}");
        }
    }
}
