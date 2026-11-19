//! `claude-hooks user-prompt-submit`（Claude Code の UserPromptSubmit の hook）が、作業ディレクトリのリポジトリの既定ブランチ
//! （手元のブランチと origin の追跡ブランチ）の先端が、同じセッションの前のプロンプトの時点から進んでいたら、新しいコミットの
//! 件名を出すこと（#363）。現在時刻の行は、登録のコマンドの `date` が出す（claude-setup の settings）

mod common;

use common::forget_git_env;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// テストごとの一時の場所。`home/` を HOME に、`gitconfig` をグローバルの設定にする
struct Work {
    root: PathBuf,
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Work {
    fn new() -> Work {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "user-prompt-submit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::create_dir_all(root.join("outside")).unwrap();
        let work = Work { root };
        std::fs::write(
            work.root.join("gitconfig"),
            "[user]\n\tname = test\n\temail = test@example.com\n",
        )
        .unwrap();
        work
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn env<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        forget_git_env(cmd)
            .env("HOME", self.path("home"))
            .env("USERPROFILE", self.path("home"))
            .env("GIT_CONFIG_GLOBAL", self.path("gitconfig"))
            // 一時の場所は target の下（手元では worktree の中）にあるので、外のリポジトリを見つけさせない
            .env("GIT_CEILING_DIRECTORIES", &self.root)
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        let out = self
            .env(&mut cmd)
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    /// branch を既定ブランチにしたリポジトリ `repo/` を作り、最初のコミットを置く
    fn repo(&self, branch: &str) -> PathBuf {
        let repo = self.path("repo");
        std::fs::create_dir_all(&repo).unwrap();
        self.git(&repo, &["init", "-q", "-b", branch]);
        self.git(
            &repo,
            &["commit", "-q", "--allow-empty", "-m", "chore: start"],
        );
        repo
    }

    /// 今の HEAD にコミットを積む
    fn commit(&self, repo: &Path, subject: &str) {
        self.git(repo, &["commit", "-q", "--allow-empty", "-m", subject]);
    }

    /// origin の追跡ブランチ（`refs/remotes/origin/<branch>`）だけを、その先にコミットを 1 つ足した先へ進める
    /// （ほかのセッションが fetch した後と同じ ref の形。手元のブランチは動かさない）
    fn advance_origin(&self, repo: &Path, branch: &str, subject: &str) {
        let tracking = format!("refs/remotes/origin/{branch}");
        let base = self.git(repo, &["rev-parse", "--verify", "-q", &tracking]);
        let tree = self.git(repo, &["rev-parse", &format!("{base}^{{tree}}")]);
        let new = self.git(repo, &["commit-tree", &tree, "-p", &base, "-m", subject]);
        self.git(repo, &["update-ref", &tracking, &new]);
    }

    /// session の UserPromptSubmit の hook を cwd で 1 回実行する
    fn prompt(&self, session: &str, cwd: &Path) -> Output {
        let input = json!({
            "session_id": session,
            "cwd": cwd.to_str().unwrap(),
            "hook_event_name": "UserPromptSubmit",
            "prompt": "next",
        })
        .to_string();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
        let mut child = self
            .env(&mut cmd)
            .arg("user-prompt-submit")
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    /// hook を実行し、終了コード 0 を確かめて標準出力を返す
    fn stdout(&self, session: &str, cwd: &Path) -> String {
        let out = self.prompt(session, cwd);
        assert_eq!(
            out.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

const SESSION: &str = "11111111-2222-3333-4444-555555555555";
const OTHER: &str = "99999999-8888-7777-6666-555555555555";

#[test]
fn the_first_prompt_of_a_session_says_nothing() {
    let work = Work::new();
    let repo = work.repo("main");
    assert_eq!(work.stdout(SESSION, &repo), "");
}

#[test]
fn nothing_is_said_while_the_default_branch_stays() {
    let work = Work::new();
    let repo = work.repo("main");
    work.stdout(SESSION, &repo);
    assert_eq!(work.stdout(SESSION, &repo), "");
    assert_eq!(work.stdout(SESSION, &repo), "");
}

#[test]
fn new_commits_on_the_local_default_branch_are_listed() {
    let work = Work::new();
    let repo = work.repo("main");
    work.stdout(SESSION, &repo);
    work.commit(&repo, "Merge: another session's change");
    let out = work.stdout(SESSION, &repo);
    assert!(out.contains("Merge: another session's change"), "{out}");
    assert!(out.contains("main"), "{out}");
    // 知らせた後の次のプロンプトは、同じコミットをもう出さない
    assert_eq!(work.stdout(SESSION, &repo), "");
}

#[test]
fn a_move_of_only_the_origin_tracking_branch_is_listed() {
    let work = Work::new();
    let repo = work.repo("main");
    let head = work.git(&repo, &["rev-parse", "HEAD"]);
    work.git(&repo, &["update-ref", "refs/remotes/origin/main", &head]);
    work.stdout(SESSION, &repo);
    work.advance_origin(&repo, "main", "Merge: fetched from origin");
    let out = work.stdout(SESSION, &repo);
    assert!(out.contains("Merge: fetched from origin"), "{out}");
    assert!(out.contains("origin/main"), "{out}");
}

#[test]
fn more_than_five_new_commits_show_five_subjects_and_the_rest_as_a_count() {
    let work = Work::new();
    let repo = work.repo("main");
    work.stdout(SESSION, &repo);
    for n in 1..=7 {
        work.commit(&repo, &format!("feat: change {n}"));
    }
    let out = work.stdout(SESSION, &repo);
    // 新しい順に 5 件
    for n in 3..=7 {
        assert!(out.contains(&format!("feat: change {n}")), "{out}");
    }
    for n in 1..=2 {
        assert!(!out.contains(&format!("feat: change {n}")), "{out}");
    }
    assert!(out.contains("ほか 2 件"), "{out}");
    let subjects = out.lines().filter(|l| l.contains("feat: change")).count();
    assert_eq!(subjects, 5, "{out}");
}

#[test]
fn only_first_parent_subjects_are_counted() {
    // マージした側のブランチのコミットは数えず、マージコミットの件名が並ぶ
    let work = Work::new();
    let repo = work.repo("main");
    work.stdout(SESSION, &repo);
    work.git(&repo, &["switch", "-q", "-c", "feat/a"]);
    for n in 1..=6 {
        work.commit(&repo, &format!("feat: inner {n}"));
    }
    work.git(&repo, &["switch", "-q", "main"]);
    work.git(
        &repo,
        &["merge", "-q", "--no-ff", "-m", "Merge: feat/a", "feat/a"],
    );
    let out = work.stdout(SESSION, &repo);
    assert!(out.contains("Merge: feat/a"), "{out}");
    assert!(!out.contains("feat: inner"), "{out}");
    assert!(!out.contains("ほか"), "{out}");
}

#[test]
fn each_session_compares_with_its_own_previous_prompt() {
    let work = Work::new();
    let repo = work.repo("main");
    work.stdout(SESSION, &repo);
    work.stdout(OTHER, &repo);
    work.commit(&repo, "Merge: first");
    // SESSION が知らせを受けても、OTHER の前の先端は動かない
    assert!(work.stdout(SESSION, &repo).contains("Merge: first"));
    assert!(work.stdout(OTHER, &repo).contains("Merge: first"));
    assert_eq!(work.stdout(SESSION, &repo), "");
    assert_eq!(work.stdout(OTHER, &repo), "");
}

#[test]
fn a_session_started_after_the_move_is_not_told_about_it() {
    let work = Work::new();
    let repo = work.repo("main");
    work.stdout(SESSION, &repo);
    work.commit(&repo, "Merge: before the other session");
    assert_eq!(work.stdout(OTHER, &repo), "");
}

#[test]
fn a_linked_worktree_shares_the_previous_tip_of_its_clone() {
    // 前の先端はセッションとリポジトリ（共通の .git）の組で覚える。同じ clone の worktree に移っても同じ組
    let work = Work::new();
    let repo = work.repo("main");
    let tree = work.path("tree");
    work.git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/b",
            tree.to_str().unwrap(),
        ],
    );
    work.stdout(SESSION, &repo);
    work.commit(&repo, "Merge: seen from the worktree");
    let out = work.stdout(SESSION, &tree);
    assert!(out.contains("Merge: seen from the worktree"), "{out}");
}

#[test]
fn outside_a_git_repository_nothing_is_said() {
    let work = Work::new();
    let outside = work.path("outside");
    assert_eq!(work.stdout(SESSION, &outside), "");
    assert_eq!(work.stdout(SESSION, &outside), "");
}

#[test]
fn a_repository_without_a_known_default_branch_says_nothing() {
    let work = Work::new();
    let repo = work.repo("trunk");
    work.stdout(SESSION, &repo);
    work.commit(&repo, "feat: on trunk");
    assert_eq!(work.stdout(SESSION, &repo), "");
}

#[test]
fn a_missing_cwd_says_nothing() {
    let work = Work::new();
    let gone = work.path("gone");
    assert_eq!(work.stdout(SESSION, &gone), "");
}
