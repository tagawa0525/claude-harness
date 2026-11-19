//! `claude-hooks script git-merge-state [branch]` が、/git-merge の事前の確認（対象の検証と、マージの前に要る状態）を
//! 出すことを確かめる（ADR-0087。`~/.claude/scripts/git-merge-state.sh` が呼ぶ）。積んだブランチ、worktree、
//! PROPOSED_ADRS の Refs: の読み方などの場合分けは、sh から呼ぶ `claude/tests/scripts/git-merge-state.sh` も確かめる

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
            "git-merge-state-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.write("docs/adr/0001-a.md", "---\nstatus: proposed\n---\n\n# A\n");
        repo.commit("docs(adr): add a");
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

    fn write(&self, path: &str, text: &str) {
        let file = self.root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
    }

    fn commit(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    fn run(&self, args: &[&str]) -> Output {
        forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["script", "git-merge-state"])
            .args(args)
            .current_dir(&self.root)
            // バイナリの中の git も、利用者のグローバルの設定を読まない
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
fn reports_the_state_before_the_merge() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("a.txt", "a\n");
    repo.commit("feat: a\n\nRefs: #1, ADR-0001");
    let root = repo.git(&["rev-parse", "--show-toplevel"]);
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        format!(
            "TARGET: feat/x\nDEFAULT: main\nCOMMITS: 1\nBEHIND: 0\nSTACKED_ON: none\n\
             TARGET_WORKTREE: {}\nDEFAULT_WORKTREE: none\nPROPOSED_ADRS: ADR-0001\n\
             STALE_CONFIRMATIONS: none\n",
            root.trim()
        )
    );
}

/// 「確認」に `src/impl.sh` と `src/` と `docs/adr/0001-a.md` を挙げた ADR-0002 を、実装の後に足したブランチ
fn confirmed_branch(repo: &Repo) {
    repo.write("src/impl.sh", "echo 1\n");
    repo.commit("feat: impl");
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write(
        "docs/adr/0002-b.md",
        "---\nstatus: proposed\n---\n\n# B\n\n## 確認\n\n- `src/impl.sh` が 1 を出す。`src/` と \
         `docs/adr/0001-a.md` も見る\n",
    );
    repo.commit("docs(adr): b");
}

fn stale_line(out: &Output) -> String {
    text(&out.stdout)
        .lines()
        .find(|l| l.starts_with("STALE_CONFIRMATIONS:"))
        .unwrap_or_default()
        .to_string()
}

#[test]
fn lists_files_named_in_a_confirmation_that_changed_after_the_adr() {
    let repo = Repo::new();
    confirmed_branch(&repo);
    repo.write("src/impl.sh", "echo 2\n");
    repo.write("docs/adr/0001-a.md", "---\nstatus: proposed\n---\n\n# A2\n");
    repo.commit("fix: impl");
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    // ディレクトリと ADR は見ない（design-records check の注意と同じ）
    assert_eq!(
        stale_line(&out),
        "STALE_CONFIRMATIONS: ADR-0002:src/impl.sh"
    );
}

#[test]
fn an_adr_updated_after_the_files_it_names_is_not_stale() {
    let repo = Repo::new();
    confirmed_branch(&repo);
    repo.write("src/impl.sh", "echo 2\n");
    repo.commit("fix: impl");
    repo.write(
        "docs/adr/0002-b.md",
        "---\nstatus: accepted\n---\n\n# B\n\n## 確認\n\n- `src/impl.sh` が 2 を出す\n",
    );
    repo.commit("docs(adr): follow impl");
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(stale_line(&out), "STALE_CONFIRMATIONS: none");
}

#[test]
fn the_default_branch_or_a_missing_branch_is_not_a_target() {
    let repo = Repo::new();
    let out = repo.run(&["main"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("既定ブランチ main 自身"));
    let out = repo.run(&["no-such"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("ローカルにブランチ no-such がありません"));
}

#[test]
fn uncommitted_changes_stop_the_merge() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("a.txt", "a\n");
    repo.commit("feat: a");
    repo.write("b.txt", "b\n");
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("未コミットの変更があります"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn a_github_remote_belongs_to_the_pr_flow() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/octo/repo.git",
    ]);
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("GitHub リモートがあります"));
}

#[test]
fn a_tag_named_like_the_target_or_default_branch_stops_the_merge() {
    for tag in ["feat/x", "main"] {
        let repo = Repo::new();
        repo.git(&["tag", tag]);
        repo.git(&["switch", "-q", "-c", "feat/x"]);
        repo.write("a.txt", "a\n");
        repo.commit("feat: a");
        let out = repo.run(&[]);
        assert_eq!(out.status.code(), Some(1), "tag {tag}");
        assert!(
            text(&out.stderr).contains(&format!("タグ {tag}")),
            "{}",
            text(&out.stderr)
        );
        assert_eq!(text(&out.stdout), "");
    }
}

#[test]
fn a_failed_tag_lookup_stops_instead_of_passing_as_no_collision() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("a.txt", "a\n");
    repo.commit("feat: a");
    // 壊れた packed-refs は、loose のブランチは読めてもタグの検索（loose に無ければ packed を読む）だけを失敗させる
    repo.write(".git/packed-refs", "garbage line\n");
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("git show-ref で失敗"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(text(&out.stdout), "");
}

/// PATH の本物の git
#[cfg(unix)]
fn real_git() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|d| d.join("git"))
        .find(|p| p.is_file())
        .expect("PATH に git が無い")
}

/// 引数が pattern（sh の case の形）に当たる呼び出しだけを 128 で失敗させ、ほかは本物の git に渡す git を置いた
/// PATH で、git-merge-state を実行する
#[cfg(unix)]
fn run_with_failing_git(repo: &Repo, pattern: &str) -> Output {
    // .git の下に置く（作業ツリーに置くと、未コミットの変更として止まる）
    let bin = repo.root.join(".git/fake-bin");
    std::fs::create_dir_all(&bin).unwrap();
    common::write_executable(
        &bin.join("git"),
        format!(
            "#!/bin/sh\ncase \"$*\" in\n  {pattern}) echo 'fatal: injected' >&2; exit 128 ;;\nesac\nexec '{}' \"$@\"\n",
            common::sh_path(&real_git())
        ),
    );
    let path = std::env::join_paths(
        std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
        .args(["script", "git-merge-state"])
        .current_dir(&repo.root)
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", repo.root.join("no-global-config"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn a_failed_default_branch_lookup_stops_instead_of_reporting_no_default() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("a.txt", "a\n");
    repo.commit("feat: a");
    let out = run_with_failing_git(&repo, "*'show-ref --verify --quiet refs/heads/main'*");
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stdout));
    assert!(
        text(&out.stderr).contains("git show-ref で失敗"),
        "{}",
        text(&out.stderr)
    );
    // 失敗した git の標準エラーも見せる（原因が分からないまま止めない）
    assert!(
        text(&out.stderr).contains("fatal: injected"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(text(&out.stdout), "");
}

#[cfg(unix)]
#[test]
fn a_failed_ancestry_check_stops_instead_of_reporting_no_stack() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/base"]);
    repo.write("a.txt", "a\n");
    repo.commit("feat: a");
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("b.txt", "b\n");
    repo.commit("feat: b");
    let out = run_with_failing_git(&repo, "*'merge-base --is-ancestor'*");
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stdout));
    assert!(
        text(&out.stderr).contains("git merge-base で失敗"),
        "{}",
        text(&out.stderr)
    );
    // 失敗した git の標準エラーも見せる（原因が分からないまま止めない）
    assert!(
        text(&out.stderr).contains("fatal: injected"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn a_renumber_commit_does_not_hide_earlier_changes_of_the_named_files() {
    let repo = Repo::new();
    confirmed_branch(&repo);
    repo.write("src/impl.sh", "echo 2\n");
    repo.commit("fix: impl");
    // design-records renumber が作るコミット（改名と表題の付け替え）
    repo.git(&["mv", "docs/adr/0002-b.md", "docs/adr/0003-b.md"]);
    repo.commit("docs: renumber ADR-0002 to ADR-0003");
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        stale_line(&out),
        "STALE_CONFIRMATIONS: ADR-0003:src/impl.sh"
    );
}

#[test]
fn changes_merged_from_the_default_branch_are_not_stale() {
    let repo = Repo::new();
    confirmed_branch(&repo);
    repo.git(&["switch", "-q", "main"]);
    repo.write("src/impl.sh", "echo 3\n");
    repo.commit("fix: impl on main");
    repo.git(&["switch", "-q", "feat/x"]);
    repo.git(&["merge", "-q", "--no-edit", "main"]);
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(stale_line(&out), "STALE_CONFIRMATIONS: none");
}

#[test]
fn a_submodule_named_in_a_confirmation_is_not_a_file() {
    let repo = Repo::new();
    confirmed_branch(&repo);
    let head = repo.git(&["rev-parse", "HEAD"]);
    let link = format!("160000,{},vendor/lib", head.trim());
    repo.git(&["update-index", "--add", "--cacheinfo", &link]);
    // 取ってきていないサブモジュールと同じく空のディレクトリを置く（無いと消した変更に見える）
    std::fs::create_dir_all(repo.root.join("vendor/lib")).unwrap();
    repo.git(&["commit", "-q", "-m", "chore: vendor"]);
    repo.write(
        "docs/adr/0002-b.md",
        "---\nstatus: proposed\n---\n\n# B\n\n## 確認\n\n- `vendor/lib` を使う\n",
    );
    // add -A は作業ツリーに無い gitlink を消すので、ADR だけをステージする
    repo.git(&["add", "docs/adr/0002-b.md"]);
    repo.git(&["commit", "-q", "-m", "docs(adr): b names the submodule"]);
    let other = repo.git(&["rev-parse", "HEAD~1"]);
    let link = format!("160000,{},vendor/lib", other.trim());
    repo.git(&["update-index", "--cacheinfo", &link]);
    repo.git(&["commit", "-q", "-m", "chore: bump vendor"]);
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(stale_line(&out), "STALE_CONFIRMATIONS: none");
}

#[test]
fn the_references_a_renumber_commit_rewrites_are_not_a_change() {
    let repo = Repo::new();
    confirmed_branch(&repo);
    // design-records renumber は、ブランチで足した行の参照も同じコミットで付け替える
    repo.git(&["mv", "docs/adr/0002-b.md", "docs/adr/0003-b.md"]);
    repo.write("src/impl.sh", "echo 1\n# ADR-0003\n");
    repo.commit("docs: renumber ADR-0002 to ADR-0003");
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(stale_line(&out), "STALE_CONFIRMATIONS: none");
}

#[test]
fn stale_confirmations_are_found_from_a_subdirectory() {
    let repo = Repo::new();
    confirmed_branch(&repo);
    repo.write("src/impl.sh", "echo 2\n");
    repo.commit("fix: impl");
    let out = forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
        .args(["script", "git-merge-state"])
        .current_dir(repo.root.join("src"))
        .env("GIT_CONFIG_GLOBAL", repo.root.join("no-global-config"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        stale_line(&out),
        "STALE_CONFIRMATIONS: ADR-0002:src/impl.sh"
    );
}
