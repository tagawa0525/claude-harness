//! `claude-hooks script worktree-add <branch> [--from <ref>] [--carry-changes]` が、命名の規則に従って worktree を作る
//! ことを確かめる（ADR-0087。`~/.claude/scripts/worktree-add.sh` が呼ぶ）。fetch、--carry-changes、.worktreelink の
//! 場合分けは、sh から呼ぶ `claude/tests/scripts/git-workflow.sh` も確かめる

mod common;

use common::forget_git_env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Repo {
    /// 一時のディレクトリ。メインの worktree は `<root>/app`、作る worktree は `<root>/app-<branch>`
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
            "worktree-add-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app")).unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.write(".gitignore", "/shared\n/token.txt\n");
        repo.commit("init");
        repo
    }

    fn app(&self) -> PathBuf {
        self.root.join("app")
    }

    fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let out = forget_git_env(&mut Command::new("git"))
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir)
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

    fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.app(), args)
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.app().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
    }

    fn commit(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_in(&self.app(), args)
    }

    fn run_in(&self, dir: &Path, args: &[&str]) -> Output {
        forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["script", "worktree-add"])
            .args(args)
            .current_dir(dir)
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
fn a_new_branch_is_made_from_the_default_branch_next_to_the_main_worktree() {
    let repo = Repo::new();
    let out = repo.run(&["feat/login"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    // 期待するパスは git の出す形（Windows でも / で区切る）でつなぐ
    let main_root = repo.git(&["rev-parse", "--show-toplevel"]);
    let (parent, _) = main_root.rsplit_once('/').unwrap();
    let dir = PathBuf::from(format!("{parent}/app-feat-login"));
    assert_eq!(
        text(&out.stdout),
        format!(
            "WORKTREE: {}\nBRANCH: feat/login\nCREATED_BRANCH: yes\nBASE: main\n",
            dir.display()
        )
    );
    assert_eq!(
        repo.git_in(&dir, &["branch", "--show-current"]),
        "feat/login"
    );
    // 起点の追跡ブランチを上流にしない（上流が main だと push 先が main になる）
    let upstream = forget_git_env(&mut Command::new("git"))
        .args(["config", "--get", "branch.feat/login.merge"])
        .current_dir(repo.app())
        .env("GIT_CONFIG_GLOBAL", repo.root.join("no-global-config"))
        .output()
        .unwrap();
    assert_eq!(text(&upstream.stdout), "");
}

#[test]
fn an_existing_branch_is_checked_out_and_from_is_refused_for_it() {
    let repo = Repo::new();
    repo.git(&["branch", "fix/typo"]);
    let out = repo.run(&["fix/typo", "--from", "main"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("--from は新しいブランチにだけ"));
    let out = repo.run(&["fix/typo"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("CREATED_BRANCH: no\n"));
    assert!(!text(&out.stdout).contains("BASE:"));
}

#[test]
fn worktreelink_shares_ignored_paths_of_the_main_worktree() {
    let repo = Repo::new();
    // ディレクトリとファイル（Windows は symlink_dir と symlink_file で作り方が違う）
    std::fs::create_dir_all(repo.app().join("shared")).unwrap();
    std::fs::write(repo.app().join("token.txt"), "t\n").unwrap();
    repo.write(".worktreelink", "# 共有する\nshared\ntoken.txt\n");
    repo.commit("add worktreelink");
    let out = repo.run(&["feat/x"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).ends_with("SHARED: shared\nSHARED: token.txt\n"),
        "{}",
        text(&out.stdout)
    );
    for name in ["shared", "token.txt"] {
        let link = repo.root.join("app-feat-x").join(name);
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "{name}"
        );
        assert_eq!(
            std::fs::canonicalize(&link).unwrap(),
            std::fs::canonicalize(repo.app().join(name)).unwrap(),
            "{name}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(repo.root.join("app-feat-x/token.txt")).unwrap(),
        "t\n"
    );
}

#[test]
fn carry_changes_moves_every_kind_of_change_without_touching_stash() {
    let repo = Repo::new();
    repo.write("edit.txt", "one\n");
    repo.write("gone.txt", "gone\n");
    repo.write("old-name.txt", "renamed content\nsecond line\n");
    repo.write("script.sh", "#!/bin/sh\n");
    repo.commit("fixtures");
    // 別のセッションが積んだ stash（触れてはいけない）
    repo.write("other.txt", "other\n");
    repo.git(&["add", "other.txt"]);
    repo.git(&["stash", "push", "-q", "-m", "someone else"]);
    let stash_before = repo.git(&["rev-parse", "refs/stash"]);

    repo.write("edit.txt", "one\ntwo\n");
    repo.git(&["add", "edit.txt"]);
    repo.write("edit.txt", "one\ntwo\nthree\n"); // index と作業ツリーで違う
    std::fs::remove_file(repo.app().join("gone.txt")).unwrap();
    repo.git(&["mv", "old-name.txt", "new-name.txt"]);
    std::fs::write(repo.app().join("bin.dat"), [0u8, 159, 146, 150, 255, 0]).unwrap();
    repo.git(&["add", "bin.dat"]);
    repo.write("untracked/sub/file.txt", "untracked\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = repo.app().join("script.sh");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let out = repo.run(&["feat/carry", "--carry-changes"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let wt = repo.root.join("app-feat-carry");

    // 元の作業ツリーからは取り除く（移動）
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
    assert!(!repo.app().join("untracked").exists());
    // 並行する別セッションの stash には触れない
    assert_eq!(repo.git(&["rev-parse", "refs/stash"]), stash_before);
    assert_eq!(repo.git(&["stash", "list"]).lines().count(), 1);

    let read = |p: &str| std::fs::read(wt.join(p)).unwrap();
    assert_eq!(read("edit.txt"), b"one\ntwo\nthree\n");
    assert!(!wt.join("gone.txt").exists());
    assert!(!wt.join("old-name.txt").exists());
    assert_eq!(read("new-name.txt"), b"renamed content\nsecond line\n");
    assert_eq!(read("bin.dat"), [0u8, 159, 146, 150, 255, 0]);
    assert_eq!(read("untracked/sub/file.txt"), b"untracked\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(wt.join("script.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
    }
}

#[test]
fn carry_changes_works_from_a_subdirectory() {
    let repo = Repo::new();
    repo.write("sub/tracked.txt", "one\n");
    repo.commit("fixtures");
    repo.write("sub/tracked.txt", "one\ntwo\n");
    repo.write("sub/new.txt", "new\n");
    let out = repo.run_in(&repo.app().join("sub"), &["feat/sub", "--carry-changes"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let wt = repo.root.join("app-feat-sub");
    assert_eq!(
        std::fs::read_to_string(wt.join("sub/tracked.txt")).unwrap(),
        "one\ntwo\n"
    );
    assert_eq!(
        std::fs::read_to_string(wt.join("sub/new.txt")).unwrap(),
        "new\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.app().join("sub/tracked.txt")).unwrap(),
        "one\n"
    );
    assert!(!repo.app().join("sub/new.txt").exists());
    assert!(repo.git(&["status", "--porcelain"]).is_empty());
}

#[test]
fn carry_changes_keeps_the_original_changes_when_they_do_not_apply() {
    let repo = Repo::new();
    repo.write("a.txt", "base\n");
    repo.commit("a");
    repo.write("a.txt", "newer\n");
    repo.commit("a newer");
    // 起点（HEAD~1）に当たらない変更: a.txt の base は起点にある newer と食い違う
    repo.write("a.txt", "newer\nlocal\n");
    repo.write("fresh.txt", "fresh\n");
    let out = repo.run(&["feat/fail", "--from", "HEAD~1", "--carry-changes"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stdout));
    assert_eq!(repo.git(&["stash", "list"]), "");
    // git_in は出力の前後の空白を落とすので、先頭の " M" は "M" になる
    assert_eq!(
        repo.git(&["status", "--porcelain"]),
        "M a.txt\n?? fresh.txt"
    );
    assert_eq!(
        std::fs::read_to_string(repo.app().join("a.txt")).unwrap(),
        "newer\nlocal\n"
    );
}

#[test]
fn carry_changes_keeps_untracked_files_that_the_patch_could_not_hold() {
    let repo = Repo::new();
    // secret/ は未追跡のまま、同時に .gitignore で無視する変更をすると、git add -A では patch に入らない
    repo.write("secret/key.txt", "secret\n");
    repo.write(".gitignore", "/shared\n/token.txt\n/secret/\n");
    let out = repo.run(&["feat/keep", "--carry-changes"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        std::fs::read_to_string(repo.app().join("secret/key.txt")).unwrap(),
        "secret\n"
    );
}

#[test]
fn bad_arguments_or_an_existing_directory_are_errors() {
    let repo = Repo::new();
    for args in [
        &[][..],
        &["a", "b"][..],
        &["a", "--from"][..],
        &["a", "--force"][..],
    ] {
        let out = repo.run(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            text(&out.stderr).contains("Usage: worktree-add.sh"),
            "{args:?}"
        );
    }
    std::fs::create_dir_all(repo.root.join("app-feat-y")).unwrap();
    let out = repo.run(&["feat/y"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("作成先が既に存在します"));
}
