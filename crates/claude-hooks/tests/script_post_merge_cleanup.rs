//! `claude-hooks script post-merge-cleanup <branch>` が、マージの後にブランチと worktree を片付けることを確かめる
//! （ADR-0087。`~/.claude/scripts/post-merge-cleanup.sh` が呼ぶ）。痕跡、fetch と pull、NOTE、予約のタグ、リモートの
//! ブランチの場合分けは、sh から呼ぶ `claude/tests/scripts/git-workflow.sh` も確かめる

mod common;

use common::forget_git_env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

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
            "post-merge-cleanup-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app")).unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.write("a.txt", "a\n");
        repo.commit("init");
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
            // 利用者のグローバルの設定（グローバルの git hook など）を読まない
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write(&self, path: &str, text: &str) {
        std::fs::write(self.app().join(path), text).unwrap();
    }

    fn commit(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    fn run(&self, args: &[&str]) -> Output {
        forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["script", "post-merge-cleanup"])
            .args(args)
            .current_dir(self.app())
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_merged_branch_and_its_worktree_are_removed() {
    let repo = Repo::new();
    let wt = repo.root.join("app-feat-x");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feat/x",
        wt.to_str().unwrap(),
    ]);
    repo.git(&["merge", "-q", "--ff-only", "feat/x"]);
    let out = repo.run(&["feat/x"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.starts_with("WORKTREE_REMOVED: "), "{stdout}");
    assert!(
        stdout
            .ends_with("RESERVATIONS_RELEASED: none\nLOCAL_BRANCH: deleted\nREMOTE_BRANCH: none\n"),
        "{stdout}"
    );
    assert!(!wt.exists());
    assert_eq!(repo.git(&["branch", "--list", "feat/x"]), "");
}

#[test]
fn an_unmerged_branch_is_left_as_it_is() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/y"]);
    repo.write("b.txt", "b\n");
    repo.commit("feat: b");
    repo.git(&["switch", "-q", "main"]);
    let out = repo.run(&["feat/y"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("feat/y は main にまだマージされていません"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(repo.git(&["branch", "--list", "feat/y"]), "feat/y");
}

#[test]
fn a_branch_without_any_trace_changes_nothing() {
    let repo = Repo::new();
    let out = repo.run(&["feat/none"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("feat/none の痕跡"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn bad_arguments_and_the_default_branch_are_errors() {
    let repo = Repo::new();
    for args in [&[][..], &["a", "b", "c"][..], &[""][..]] {
        let out = repo.run(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            text(&out.stderr).contains("Usage: post-merge-cleanup.sh"),
            "{args:?}"
        );
    }
    let out = repo.run(&["main"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("既定ブランチ main は削除しません"));
}

#[test]
fn a_branch_checked_out_in_the_main_worktree_is_switched_away_from() {
    // worktree を作らずにメインの clone で作業したブランチ。メインの worktree は消さずに、既定のブランチへ切り替える
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/main-wt"]);
    repo.write("c.txt", "c\n");
    repo.commit("feat: c");
    repo.git(&["switch", "-q", "main"]);
    repo.git(&["merge", "-q", "--ff-only", "feat/main-wt"]);
    repo.git(&["switch", "-q", "feat/main-wt"]);
    let out = repo.run(&["feat/main-wt"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).starts_with("WORKTREE_REMOVED: none\n"),
        "{}",
        text(&out.stdout)
    );
    assert!(repo.app().exists());
    assert_eq!(repo.git(&["branch", "--show-current"]), "main");
    assert_eq!(repo.git(&["branch", "--list", "feat/main-wt"]), "");
}

#[test]
fn a_remote_branch_with_a_longer_name_is_not_the_branch() {
    // ls-remote の名前は末尾で一致するので、alice/fix-typo を fix-typo と取り違えない
    let repo = Repo::new();
    let bare = repo.root.join("origin.git");
    repo.git(&["init", "-q", "--bare", bare.to_str().unwrap()]);
    repo.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    repo.git(&["push", "-q", "-u", "origin", "main"]);
    repo.git(&["push", "-q", "origin", "main:refs/heads/alice/fix-typo"]);
    repo.git(&["branch", "fix-typo"]);
    let out = repo.run(&["fix-typo"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).ends_with("REMOTE_BRANCH: none\n"),
        "{}",
        text(&out.stdout)
    );
}

#[test]
fn uncommitted_changes_in_the_main_worktree_stop_the_cleanup() {
    // 既定のブランチへ切り替えると、変更をそこへ持ち込む。linked worktree の remove と同じく止める
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/dirty"]);
    // 追跡しているファイルの変更（switch が既定のブランチへ持ち込むもの）
    repo.write("a.txt", "changed\n");
    let out = repo.run(&["feat/dirty"]);
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert!(err.contains("未コミットの変更"), "{err}");
    // マージ済みのブランチにコミットすると次の実行が止まるので、既定のブランチから作る worktree への移動を勧める。
    // git stash は guard-git-stash が止めるので勧めない
    assert!(err.contains("worktree-add.sh"), "{err}");
    assert!(err.contains("--carry-changes"), "{err}");
    assert!(!err.contains("git stash"), "{err}");
    assert_eq!(repo.git(&["branch", "--show-current"]), "feat/dirty");
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn the_fix_command_quotes_a_branch_name_the_shell_would_expand() {
    // 修正方法のコマンドはそのまま貼って打つので、git が許す $ を含むブランチ名を展開させない
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/a$b"]);
    repo.write("a.txt", "changed\n");
    let out = repo.run(&["feat/a$b"]);
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert!(err.contains("--from 'feat/a$b' --carry-changes"), "{err}");
}

#[test]
fn following_the_fix_for_uncommitted_changes_lets_the_rerun_clean_up() {
    // /git-merge の流れ: GitHub でない origin があり、マージはローカルの main にだけある（push の前）。origin/main は
    // マージを含まないので、そこから作ると変更の土台が無い。示す手順で変更を移せば、再実行が片付けまで進む
    let repo = Repo::new();
    let bare = repo.root.join("origin.git");
    repo.git(&["init", "-q", "--bare", bare.to_str().unwrap()]);
    repo.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    repo.git(&["push", "-q", "-u", "origin", "main"]);
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("b.txt", "b\n");
    repo.commit("feat");
    repo.git(&["switch", "-q", "main"]);
    repo.git(&["merge", "-q", "--no-ff", "-m", "Merge feat/x", "feat/x"]);
    repo.git(&["switch", "-q", "feat/x"]);
    // マージしたブランチの上で書いた、残す変更
    repo.write("b.txt", "b\nmore\n");

    let out = repo.run(&["feat/x"]);
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert!(err.contains("--from feat/x --carry-changes"), "{err}");

    let carry = forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
        .args([
            "script",
            "worktree-add",
            "fix/y",
            "--from",
            "feat/x",
            "--carry-changes",
        ])
        .current_dir(repo.app())
        .env("GIT_CONFIG_GLOBAL", repo.root.join("no-global-config"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert_eq!(carry.status.code(), Some(0), "{}", text(&carry.stderr));
    assert_eq!(
        std::fs::read_to_string(repo.root.join("app-fix-y/b.txt")).unwrap(),
        "b\nmore\n"
    );

    let rerun = repo.run(&["feat/x"]);
    assert_eq!(rerun.status.code(), Some(0), "{}", text(&rerun.stderr));
    assert_eq!(repo.git(&["branch", "--list", "feat/x"]), "");
}

#[test]
fn a_main_worktree_branch_with_the_default_branch_elsewhere_stops_before_changing_anything() {
    // 既定のブランチは別の worktree にあるので、メインの worktree をそこへ切り替えられない
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/elsewhere"]);
    let wt = repo.root.join("app-main");
    repo.git(&["worktree", "add", "-q", wt.to_str().unwrap(), "main"]);
    let out = repo.run(&["feat/elsewhere"]);
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert!(err.contains("は別の worktree"), "{err}");
    assert!(!err.contains("未コミットの変更"), "{err}");
    assert_eq!(
        repo.git(&["branch", "--list", "feat/elsewhere"]),
        "* feat/elsewhere"
    );
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn the_default_branch_is_updated_without_depending_on_fetch_head() {
    // pull.rebase の pull は FETCH_HEAD のマージの対象が 2 つ以上あると止まる（同じ clone の別の fetch が
    // FETCH_HEAD を書き換えたときに出た）。同じ症状を、branch.main.merge を 2 つにして作る
    let repo = Repo::new();
    let bare = repo.root.join("origin.git");
    repo.git(&["init", "-q", "--bare", bare.to_str().unwrap()]);
    repo.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    repo.git(&["push", "-q", "-u", "origin", "main"]);
    repo.git(&["push", "-q", "origin", "main:refs/heads/other"]);
    repo.git(&["config", "pull.rebase", "true"]);
    repo.git(&["config", "--add", "branch.main.merge", "refs/heads/other"]);
    repo.git(&["branch", "feat/x"]);
    // リモートだけが進んだ状態にする
    repo.write("b.txt", "b\n");
    repo.commit("remote only");
    repo.git(&["push", "-q", "origin", "main"]);
    let remote_tip = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["reset", "-q", "--hard", "HEAD~1"]);
    let out = repo.run(&["feat/x"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), remote_tip);
}

#[test]
fn a_default_branch_without_an_upstream_skips_the_update_and_deletes_the_branch() {
    // GitHub でないリモートを足しただけで、main に上流を設定していないリポジトリ（#406）
    let repo = Repo::new();
    let bare = repo.root.join("other-host.git");
    repo.git(&["init", "-q", "--bare", bare.to_str().unwrap()]);
    repo.git(&["remote", "add", "r995", bare.to_str().unwrap()]);
    let wt = repo.root.join("app-feat-x");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feat/x",
        wt.to_str().unwrap(),
    ]);
    std::fs::write(wt.join("b.txt"), "b\n").unwrap();
    repo.git(&["-C", wt.to_str().unwrap(), "add", "-A"]);
    repo.git(&["-C", wt.to_str().unwrap(), "commit", "-q", "-m", "feat: b"]);
    let before = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["merge", "-q", "--no-ff", "-m", "merge", "feat/x"]);
    let out = repo.run(&["feat/x", &before]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).contains("LOCAL_BRANCH: deleted"),
        "{}",
        text(&out.stdout)
    );
    assert_eq!(repo.git(&["branch", "--list", "feat/x"]), "");
}

#[test]
fn a_remote_branch_moved_past_the_unfetched_tracking_ref_is_kept() {
    // main に上流が無いと fetch しないので、追跡ブランチ（r995/feat/x）は古いまま。ほかの clone が feat/x に
    // push したコミットを、古い追跡ブランチの比較で消さない
    let repo = Repo::new();
    let bare = repo.root.join("other-host.git");
    repo.git(&["init", "-q", "--bare", bare.to_str().unwrap()]);
    repo.git(&["remote", "add", "r995", bare.to_str().unwrap()]);
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("b.txt", "b\n");
    repo.commit("feat: b");
    repo.git(&["push", "-q", "-u", "r995", "feat/x"]);
    let other = repo.root.join("other-clone");
    repo.git(&[
        "clone",
        "-q",
        "-b",
        "feat/x",
        bare.to_str().unwrap(),
        other.to_str().unwrap(),
    ]);
    std::fs::write(other.join("c.txt"), "c\n").unwrap();
    repo.git(&["-C", other.to_str().unwrap(), "add", "c.txt"]);
    repo.git(&[
        "-C",
        other.to_str().unwrap(),
        "commit",
        "-q",
        "-m",
        "feat: c",
    ]);
    repo.git(&[
        "-C",
        other.to_str().unwrap(),
        "push",
        "-q",
        "origin",
        "feat/x",
    ]);
    repo.git(&["switch", "-q", "main"]);
    let before = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["merge", "-q", "--no-ff", "-m", "merge", "feat/x"]);
    repo.git(&["push", "-q", "r995", "main"]);
    let out = repo.run(&["feat/x", &before]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("REMOTE_BRANCH: kept"), "{stdout}");
    assert_ne!(
        repo.git(&["ls-remote", "--heads", "r995", "refs/heads/feat/x"]),
        ""
    );
}

#[test]
fn a_local_merge_without_a_remote_notes_the_rebuild_from_the_given_base() {
    let repo = Repo::new();
    // marker は before にも要るので、先に main へ入れる
    std::fs::create_dir_all(repo.app().join("modules/home/scripts")).unwrap();
    repo.write("modules/home/scripts/nix-rebuild.sh", "#!/bin/sh\n");
    repo.commit("add rebuild script");
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write(
        "modules/home/scripts/nix-rebuild.sh",
        "#!/bin/sh\n# changed\n",
    );
    repo.commit("change module");
    repo.git(&["switch", "-q", "main"]);
    let before = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["merge", "-q", "--no-ff", "-m", "merge", "feat/x"]);
    let out = repo.run(&["feat/x", &before]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).contains("NOTE: rebuild が要る"),
        "{}",
        text(&out.stdout)
    );
}

#[test]
fn a_base_that_is_not_an_ancestor_of_head_is_an_error() {
    let repo = Repo::new();
    repo.git(&["branch", "feat/x"]);
    let out = repo.run(&["feat/x", "refs/heads/feat/nope"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("ERROR"), "{}", text(&out.stderr));
    assert_eq!(repo.git(&["branch", "--list", "feat/x"]), "feat/x");
}

#[test]
fn a_merge_that_deletes_the_rebuild_script_still_notes_the_rebuild() {
    let repo = Repo::new();
    std::fs::create_dir_all(repo.app().join("modules/home/scripts")).unwrap();
    repo.write("modules/home/scripts/nix-rebuild.sh", "#!/bin/sh\n");
    repo.commit("add rebuild script");
    let before = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["switch", "-q", "-c", "feat/y", "main"]);
    repo.git(&["rm", "-q", "-r", "modules"]);
    repo.commit("remove modules");
    repo.git(&["switch", "-q", "main"]);
    repo.git(&["merge", "-q", "--no-ff", "-m", "merge", "feat/y"]);
    let out = repo.run(&["feat/y", &before]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).contains("NOTE: rebuild が要る"),
        "{}",
        text(&out.stdout)
    );
}

#[test]
fn a_base_on_an_unrelated_branch_is_an_error_that_changes_nothing() {
    let repo = Repo::new();
    repo.git(&["branch", "feat/x"]);
    repo.git(&["switch", "-q", "-c", "other"]);
    repo.write("b.txt", "b\n");
    repo.commit("unrelated");
    let unrelated = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["switch", "-q", "main"]);
    let out = repo.run(&["feat/x", &unrelated]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("の祖先ではありません"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(repo.git(&["branch", "--list", "feat/x"]), "feat/x");
}

#[test]
fn with_a_base_a_merge_without_the_branch_tip_as_a_second_parent_is_an_error() {
    let repo = Repo::new();
    let before = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("b.txt", "b\n");
    repo.commit("feat: b");
    repo.git(&["switch", "-q", "main"]);
    // fast-forward はマージコミットを作らない（/git-merge の --no-ff のマージが済んでいない）
    repo.git(&["merge", "-q", "--ff-only", "feat/x"]);
    let out = repo.run(&["feat/x", &before]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stdout));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("第 2 親"), "{stderr}");
    assert!(stderr.contains("何も消していません"), "{stderr}");
    // 先端はもう main にあるので、マージのやり直しは「Already up to date」で何もしない。戻してからやり直す方法を示す
    assert!(
        stderr.contains(&format!("git reset --keep {before}")),
        "{stderr}"
    );
    assert!(!stderr.contains("別の worktree"), "{stderr}");
    // 戻すと消えるコミットと、第 2 引数の誤りも、戻す前に確かめさせる
    assert!(stderr.contains("後に積んだコミット"), "{stderr}");
    assert!(stderr.contains("第 2 引数がマージの第 1 親"), "{stderr}");
    assert_eq!(repo.git(&["branch", "--list", "feat/x"]), "feat/x");
}

#[test]
fn with_a_base_the_merge_commit_of_the_branch_passes_even_after_later_commits() {
    let repo = Repo::new();
    let before = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("b.txt", "b\n");
    repo.commit("feat: b");
    repo.git(&["switch", "-q", "main"]);
    repo.git(&["merge", "-q", "--no-ff", "-m", "merge", "feat/x"]);
    repo.write("c.txt", "c\n");
    repo.commit("later");
    let out = repo.run(&["feat/x", &before]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(repo.git(&["branch", "--list", "feat/x"]), "");
}
