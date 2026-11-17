//! `claude-hooks script base-moved [branch] [--base BASE]` が、比較元が分岐点から進み、ブランチと同じパスを触って
//! いれば知らせることを確かめる（ADR-0087。`~/.claude/scripts/base-moved.sh` が呼ぶ）。fetch するかの決め方（GitHub の
//! origin）と WARN の場合分けは、sh から呼ぶ `claude/tests/scripts/base-moved.sh` が確かめる

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
            "base-moved-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.commit("README.md", "init");
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let out = forget_git_env(&mut Command::new("git"))
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(&self.root)
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
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn commit(&self, path: &str, msg: &str) {
        let file = self.root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, format!("{msg}\n")).unwrap();
        self.git(&["add", path]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    fn run(&self, args: &[&str]) -> Output {
        forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["script", "base-moved"])
            .args(args)
            .current_dir(&self.root)
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .output()
            .unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_base_that_moved_and_touched_the_same_paths_is_reported() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/work"]);
    repo.commit("src/a.txt", "a");
    repo.commit("src/b.txt", "b");
    repo.git(&["switch", "-q", "main"]);
    repo.commit("src/a.txt", "main a");
    repo.commit("src/c.txt", "main c");
    let fork = repo.git(&["merge-base", "main", "feat/work"]);
    let head = repo.git(&["rev-parse", "--short", "main"]);
    let out = repo.run(&["feat/work", "--base", "main"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        format!(
            "BASE_MOVED: main が分岐点から 2 commits 進んでいる（先頭 {head}、分岐点 {fork}）\nCOMMON: src/a.txt\n\
             NEXT: git diff {fork}..{head} -- src/a.txt で main の変更を読む。同じ変更の二重なら作業を止め、衝突しうるなら PR 本文の Summary（/delegate の担当は報告）に書いて進める\n\
             BEHIND: 2\n",
            head = head.trim(),
            fork = &fork.trim()[..head.trim().len()]
        )
    );
    // リモートの無いリポジトリでも、ローカルのブランチの --base は確かめた比較として WARN を出さない
    assert_eq!(text(&out.stderr), "");
}

#[test]
fn no_common_path_or_no_move_prints_only_the_count_behind() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/work"]);
    repo.commit("src/a.txt", "a");
    let out = repo.run(&["--base", "main"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out.stdout), "BEHIND: 0\n");
    repo.git(&["switch", "-q", "main"]);
    repo.commit("src/c.txt", "main c");
    let out = repo.run(&["feat/work", "--base", "main"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out.stdout), "BEHIND: 1\n");
}

#[test]
fn the_count_behind_counts_every_commit_of_a_merged_branch() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/work"]);
    repo.commit("src/a.txt", "a");
    repo.git(&["switch", "-q", "-c", "side", "main"]);
    repo.commit("src/s1.txt", "s1");
    repo.commit("src/s2.txt", "s2");
    repo.git(&["switch", "-q", "main"]);
    repo.git(&["merge", "-q", "--no-ff", "-m", "Merge: side", "side"]);
    let out = repo.run(&["feat/work", "--base", "main"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    // マージコミットと、マージで入った 2 件（BASE_MOVED の件数と違い、マージを 1 件と数えない）
    assert_eq!(text(&out.stdout), "BEHIND: 3\n");
}

#[test]
fn common_paths_are_followed_by_the_diff_to_read() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/work"]);
    repo.commit("src/a.txt", "a");
    repo.commit("src/b.txt", "b");
    repo.git(&["switch", "-q", "main"]);
    repo.commit("src/a.txt", "main a");
    repo.commit("src/b.txt", "main b");
    let fork = repo.git(&["merge-base", "main", "feat/work"]);
    let head = repo.git(&["rev-parse", "--short", "main"]);
    let fork = &fork.trim()[..head.trim().len()];
    let out = repo.run(&["feat/work", "--base", "main"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    let next = stdout
        .lines()
        .find(|l| l.starts_with("NEXT: "))
        .unwrap_or_else(|| panic!("{stdout}"));
    assert!(
        next.contains(&format!(
            "git diff {fork}..{} -- src/a.txt src/b.txt",
            head.trim()
        )),
        "{next}"
    );
    assert!(next.contains("二重") && next.contains("Summary"), "{next}");
    assert!(stdout.ends_with("BEHIND: 2\n"), "{stdout}");
}

#[test]
fn without_a_github_origin_or_base_the_comparison_is_unverified() {
    let repo = Repo::new();
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = text(&out.stdout);
    assert!(stdout.starts_with("UNVERIFIED: "), "{stdout}");
    assert!(stdout.contains("--base"), "{stdout}");
    assert!(!stdout.contains("BEHIND:"), "{stdout}");
}

#[test]
fn a_warning_makes_the_comparison_unverified_instead_of_counting() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/work"]);
    repo.commit("src/a.txt", "a");
    // GitHub の無い clone で、ローカルのブランチでない --base は fetch していない追跡ブランチを指しうる
    let out = repo.run(&["--base", "main~0"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("WARN: "),
        "{}",
        text(&out.stderr)
    );
    let stdout = text(&out.stdout);
    assert!(!stdout.contains("BEHIND:"), "{stdout}");
    let unverified = stdout
        .lines()
        .find(|l| l.starts_with("UNVERIFIED: "))
        .unwrap_or_else(|| panic!("{stdout}"));
    assert!(
        unverified.contains("再実行") && unverified.contains("PR 本文"),
        "{unverified}"
    );
}

#[test]
fn bad_arguments_and_unknown_refs_are_errors() {
    let repo = Repo::new();
    for args in [
        &["--base"][..],
        &["--force"][..],
        &["no-such", "--base", "main"][..],
    ] {
        let out = repo.run(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            text(&out.stderr).starts_with("ERROR: "),
            "{args:?}: {}",
            text(&out.stderr)
        );
    }
}

/// GitHub の URL の remote（github）と、実際に通信できる origin（bare）、origin 以外のリモート up を持つ clone
fn github_clone_with_other_remote() -> Repo {
    let repo = Repo::new();
    for name in ["origin", "up"] {
        let bare = repo.root.join(format!("{name}.git"));
        repo.git(&["init", "-q", "--bare", bare.to_str().unwrap()]);
        repo.git(&["remote", "add", name, bare.to_str().unwrap()]);
        repo.git(&["push", "-q", name, "main"]);
    }
    repo.git(&[
        "remote",
        "add",
        "github",
        "https://github.com/example/app.git",
    ]);
    repo.git(&["fetch", "-q", "up"]);
    repo.git(&["switch", "-q", "-c", "feat/work"]);
    repo.commit("src/a.txt", "a");
    repo
}

#[test]
fn a_base_on_a_remote_other_than_origin_is_warned_as_unfetched() {
    let repo = github_clone_with_other_remote();
    let out = repo.run(&["--base", "up/main"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let err = text(&out.stderr);
    assert!(
        err.contains("WARN: up/main") && err.contains("未確認"),
        "{err}"
    );
}

#[test]
fn a_local_or_origin_base_in_a_github_clone_is_not_warned() {
    let repo = github_clone_with_other_remote();
    for base in ["main", "origin/main", "refs/remotes/origin/main"] {
        let out = repo.run(&["--base", base]);
        assert_eq!(out.status.code(), Some(0), "{base}");
        assert_eq!(text(&out.stderr), "", "{base}");
    }
}

#[test]
fn a_full_local_ref_is_not_warned_without_a_github_origin() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/work"]);
    repo.commit("src/a.txt", "a");
    let out = repo.run(&["--base", "refs/heads/main"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out.stderr), "");
}
