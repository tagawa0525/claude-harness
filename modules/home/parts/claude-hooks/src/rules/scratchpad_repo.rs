//! セッションの scratchpad の下に `git init` で作った一時のリポジトリか（guard-git-add と block-no-verify の例外。ADR-0072）
//!
//! scratchpad は `<Claude Code の一時ディレクトリ>/<プロジェクト>/<セッションの ID>/scratchpad`。一時ディレクトリの
//! 根は環境変数（`CLAUDE_CODE_TMPDIR` など）で変わるので、根でなく `<セッションの ID>/scratchpad` の 2 つの
//! 名前で見分ける。

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::git;
use crate::input::Input;
use crate::shell::{Cmd, Shell};
use crate::unwrap::EnvChange;

/// 例外の条件を 1 文で示す、拒否の文言に足す行
pub const EXCEPTION: &str =
    "セッションの scratchpad の下に git init で作った、リモートの無い一時のリポジトリは対象外です";

/// リポジトリの場所を指し替える環境変数と git の全体オプション。対象のディレクトリからリポジトリを決められない
const GIT_DIR_VARS: &[&str] = &["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"];
const GIT_DIR_OPTIONS: &[&str] = &["--git-dir", "--work-tree"];

/// cmd の対象のリポジトリが、セッションの scratchpad の下に作った、リモートの無い一時のリポジトリなら Ok。
/// 当たらなければ、当たらない理由
pub fn check(input: &Input, shell: &Shell, cmd: &Cmd) -> Result<(), String> {
    if input.session_id.is_empty() {
        return Err("hook の入力にセッションの ID が無く、scratchpad を決められません".to_string());
    }
    if redirects_git_dir(shell, cmd) {
        return Err(
            "GIT_DIR / GIT_WORK_TREE / --git-dir / --work-tree でリポジトリを指し替えています"
                .to_string(),
        );
    }
    // target_dir は cd だけをたどり、pushd / popd の行き先は読まない
    if shell
        .commands_before(cmd)
        .any(|c| c.name == "pushd" || c.name == "popd")
    {
        return Err("前に pushd / popd があり、git が走るディレクトリを決められません".to_string());
    }
    let target = shell.target_dir(cmd, &input.cwd);
    // 判定の時点に無いディレクトリ（同じコマンドの mkdir の前、展開や `~user` を含む書き方）は、前の cd が
    // 失敗すれば git が hook の cwd で走り、作られても祖先の worktree や複製の中かもしれず、場所を決められない
    let Ok(dir) = std::fs::canonicalize(&target) else {
        return Err(format!(
            "対象の {} はまだ無いか、場所を決められません。mkdir と git init は先に別の呼び出しで実行してください",
            target.display()
        ));
    };
    let Some(root) = scratchpad_root(&dir, &input.session_id) else {
        return Err(format!(
            "対象の {} はセッションの scratchpad の外です",
            dir.display()
        ));
    };
    let Some(out) = git::git(
        &dir,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-common-dir",
        ],
    ) else {
        return Err(format!(
            "対象の {} は git が読めるリポジトリの中でありません。git init は先に別の呼び出しで実行してください",
            dir.display()
        ));
    };
    // worktree は、作業ツリーが scratchpad の下でも、コミットとブランチを本番のリポジトリの git のディレクトリに書く
    for p in out.lines() {
        let real = std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p));
        if !real.starts_with(&root) {
            return Err(format!(
                "リポジトリ（{}）が scratchpad の外にあります（本番のリポジトリの worktree など）",
                real.display()
            ));
        }
    }
    // 複製は origin が本番のリポジトリを指し、hook を通らないコミットをそこへ push できる
    let Some(remotes) = git::git(&dir, &["remote"]) else {
        return Err(format!("{} のリモートを読めません", dir.display()));
    };
    if !remotes.is_empty() {
        return Err(format!(
            "リモート（{}）があります（hook を通らないコミットを本番のリポジトリへ push できるため）",
            remotes.lines().collect::<Vec<_>>().join("、")
        ));
    }
    Ok(())
}

fn redirects_git_dir(shell: &Shell, cmd: &Cmd) -> bool {
    let global_args = cmd
        .git
        .as_ref()
        .and_then(|g| g.sub)
        .map_or(&cmd.args[..], |i| &cmd.args[..i]);
    shell
        .assignments()
        .iter()
        .any(|(name, _)| GIT_DIR_VARS.contains(&name.as_str()))
        || cmd.env.iter().any(|change| {
            matches!(change, EnvChange::Set(name, _) if GIT_DIR_VARS.contains(&name.as_str()))
        })
        || global_args.iter().any(|a| {
            GIT_DIR_OPTIONS
                .iter()
                .any(|o| a.text == *o || a.text.starts_with(&format!("{o}=")))
        })
}

/// path の祖先のうち `<session_id>/scratchpad` の名前で終わるもの
fn scratchpad_root(path: &Path, session_id: &str) -> Option<PathBuf> {
    path.ancestors()
        .find(|a| {
            a.file_name() == Some(OsStr::new("scratchpad"))
                && a.parent().and_then(Path::file_name) == Some(OsStr::new(session_id))
        })
        .map(Path::to_path_buf)
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
    fn directory_or_repository_not_made_yet_is_not_excepted_and_asks_for_git_init_first() {
        // 判定する時点に無いディレクトリは、前の cd が失敗すれば git が hook の cwd（本番のリポジトリ）で走り、
        // 作られても祖先の worktree や複製の中かもしれないので、場所を決められない
        let f = Fixture::new("mkdir");
        let r = f.scratchpad().join("new");
        let src = format!(
            "mkdir -p {r} && cd {r} && git init -q && git add -A",
            r = r.display()
        );
        let err = judge(&src, &f.prod()).unwrap_err();
        assert!(err.contains("git init"), "{err}");
        let err = judge(&format!("cd {}; git add -A", r.display()), &f.prod()).unwrap_err();
        assert!(err.contains("git init"), "{err}");
        // ディレクトリはあるが、まだ git init していない
        let plain = f.scratchpad().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        let err = judge("git add -A", &plain).unwrap_err();
        assert!(err.contains("git init"), "{err}");
    }

    #[test]
    fn directory_not_made_yet_inside_a_worktree_or_clone_under_scratchpad_is_not_excepted() {
        let f = Fixture::new("missing-in-wt");
        let wt = f.scratchpad().join("wt");
        git(&f.prod(), &["worktree", "add", "-q", wt.to_str().unwrap()]);
        let sub = wt.join("sub");
        let src = format!(
            "mkdir -p {s} && cd {s} && git -c core.hooksPath=/dev/null commit -m x",
            s = sub.display()
        );
        assert!(judge(&src, &f.prod()).is_err());
    }

    #[test]
    fn home_of_another_user_and_globs_are_not_read_as_paths_under_scratchpad() {
        let f = Fixture::new("tilde");
        let repo = f.temp_repo("r");
        for src in [
            "cd ~root && git add -A",
            "cd ../../../../*/prod && git add -A",
        ] {
            assert!(judge(src, &repo).is_err(), "{src}");
        }
    }

    #[test]
    fn repository_git_cannot_read_is_not_taken_as_absent_or_without_remotes() {
        let f = Fixture::new("bad-config");
        let repo = f.temp_repo("r");
        // 壊れた設定では rev-parse も remote も失敗する
        std::fs::write(repo.join(".git/config"), "[core\n").unwrap();
        assert!(judge("git add -A", &repo).is_err());
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
        assert!(err.contains("git init"), "{err}");
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
        assert!(err.contains("git init"), "{err}");
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
    fn pushd_and_popd_before_git_are_not_excepted() {
        // target_dir は cd だけをたどるので、pushd の先は hook の cwd（scratchpad のリポジトリ）と読まれる
        let f = Fixture::new("pushd");
        let repo = f.temp_repo("r");
        let prod = f.prod().display().to_string();
        for src in [
            format!("pushd {prod} && git add -A && popd"),
            format!("pushd {prod} && popd && git add -A"),
        ] {
            let err = judge(&src, &repo).unwrap_err();
            assert!(err.contains("pushd"), "{src}: {err}");
        }
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
