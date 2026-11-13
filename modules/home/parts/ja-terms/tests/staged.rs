//! `ja-terms staged` が、今の staged.sh（ja-terms-staged。グローバルの pre-commit が呼ぶ）と同じく、ステージした
//! 中身を検査すること（ADR-0085）。場合は modules/home/parts/tests/ja-terms.sh の 2〜4c から取る。
//!
//! 辞書の語を 1 行に書くと、このファイルが pre-commit の語の検査に掛かるので、崩れた語は分けて書いてつなぐ。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 崩れた語（辞書の最初の規則の、名詞の用法の語。直す語は「確認」）
const BROKEN: &str = concat!("確か", "め");

fn broken(text: &str) -> String {
    text.replace("{B}", BROKEN)
}

/// 利用者の git の設定に左右されない git のリポジトリ
struct Repo(PathBuf);

impl Repo {
    fn new() -> Repo {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("staged");
        let dir = base.join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(base.join("gitconfig"), "").unwrap();
        let repo = Repo(dir);
        repo.git(&["init", "-q", "-b", "main"]);
        repo
    }

    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(&self.0)
            .env(
                "GIT_CONFIG_GLOBAL",
                self.0.parent().unwrap().join("gitconfig"),
            )
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com");
        command
    }

    fn git(&self, args: &[&str]) {
        let out = self.command("git").args(args).output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, broken(text)).unwrap();
    }

    /// 書いてステージする
    fn stage(&self, path: &str, text: &str) {
        self.write(path, text);
        self.git(&["add", "--", path]);
    }

    fn commit(&self) {
        self.git(&["commit", "-q", "--no-verify", "-m", "chore: base"]);
    }

    fn staged(&self) -> Output {
        self.command(env!("CARGO_BIN_EXE_ja-terms"))
            .arg("staged")
            .output()
            .unwrap()
    }

    /// リポジトリの中のディレクトリ dir で検査する
    fn staged_in(&self, dir: &str) -> Output {
        self.command(env!("CARGO_BIN_EXE_ja-terms"))
            .current_dir(self.0.join(dir))
            .arg("staged")
            .output()
            .unwrap()
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// 崩れた語を報告して 1 で終わったか（それ以外の終わり方は理由つきで落とす）
fn stops(out: &Output) -> bool {
    match out.status.code() {
        Some(0) => false,
        Some(1) => true,
        code => panic!(
            "{code:?}: {}{}",
            stdout(out),
            String::from_utf8_lossy(&out.stderr)
        ),
    }
}

#[test]
fn broken_terms_are_reported_with_the_staged_path() {
    let repo = Repo::new();
    repo.stage("doc.md", "# t\n\n{B}の結果。\n");
    let out = repo.staged();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stdout(&out),
        format!(
            "🔍 Checking Japanese terms...\ndoc.md:3:1: {BROKEN} => 確認 [Error/prh]\n\n1 problem\n\
             ❌ 崩れた日本語の語があります。上の「誤り => 正しい語」に直してください（辞書: claude-harness の modules/home/parts/ja-terms/prh.yml）\n"
        )
    );
}

#[test]
fn nothing_staged_prints_nothing() {
    let repo = Repo::new();
    let out = repo.staged();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out), "");
}

#[test]
fn clean_files_pass_after_the_banner() {
    let repo = Repo::new();
    repo.stage("doc.md", "# t\n\n結果を確かめる。\n");
    let out = repo.staged();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out), "🔍 Checking Japanese terms...\n");
}

#[test]
fn comments_in_code_are_checked() {
    let repo = Repo::new();
    repo.stage("lib.rs", "// {B}の結果\n");
    assert!(stops(&repo.staged()));
}

#[test]
fn the_staged_content_is_checked_not_the_work_tree() {
    let repo = Repo::new();
    repo.stage("doc.txt", "結果を確かめる。\n");
    repo.write("doc.txt", "{B}の結果。\n");
    assert!(!stops(&repo.staged()));

    let repo = Repo::new();
    repo.stage("doc.txt", "{B}の結果。\n");
    repo.write("doc.txt", "結果を確かめる。\n");
    assert!(stops(&repo.staged()));
}

#[test]
fn names_with_glob_characters_spaces_and_upper_case_extensions_are_checked() {
    for name in ["[draft].md", "a b/doc.md", "UPPER.MD", "a].txt"] {
        let repo = Repo::new();
        repo.stage(name, "# t\n\n{B}の結果。\n");
        let out = repo.staged();
        assert!(stops(&out), "{name}");
        assert!(stdout(&out).contains(&format!("\n{name}:3:1: ")), "{name}");
    }
}

#[test]
fn files_of_other_extensions_are_not_checked() {
    let repo = Repo::new();
    repo.stage("page.html", "{B}の結果。\n");
    let out = repo.staged();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out), "");
}

#[test]
fn the_prh_dictionary_is_not_checked() {
    let repo = Repo::new();
    repo.stage(
        "ja-terms/prh.yml",
        "rules:\n  - expected: 確認\n    pattern: {B}の\n",
    );
    assert!(!stops(&repo.staged()));
}

#[test]
fn files_listed_in_the_staged_textlintignore_are_not_checked() {
    let repo = Repo::new();
    repo.stage(".textlintignore", "fixtures/**\n");
    for name in ["fixtures/case.md", "fixtures/[ab].md", "fixtures/a].md"] {
        repo.stage(name, "# t\n\n{B}の結果。\n");
    }
    assert!(!stops(&repo.staged()));
    // 作業ツリーだけの .textlintignore は使わない
    let repo = Repo::new();
    repo.write(".textlintignore", "fixtures/**\n");
    repo.stage("fixtures/case.md", "# t\n\n{B}の結果。\n");
    assert!(stops(&repo.staged()));
}

#[test]
fn renamed_files_do_not_hide_other_files() {
    let repo = Repo::new();
    repo.stage("a.md", "# t\n\nok\n");
    repo.commit();
    repo.git(&["mv", "a.md", "b.md"]);
    repo.stage("z.md", "# t\n\n{B}の結果。\n");
    let out = repo.staged();
    assert!(stops(&out));
    assert!(stdout(&out).contains("\nz.md:3:1: "));
}

#[test]
fn submodules_are_not_checked() {
    let repo = Repo::new();
    repo.stage("README.md", "init\n");
    repo.commit();
    let head = repo
        .command("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let head = String::from_utf8(head.stdout).unwrap();
    repo.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{},sub.txt", head.trim()),
    ]);
    assert!(!stops(&repo.staged()));
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_not_checked_but_a_change_to_a_file_is() {
    let repo = Repo::new();
    repo.stage("target.txt", "{B}の結果。\n");
    repo.commit();
    std::os::unix::fs::symlink("target.txt", repo.0.join("link.txt")).unwrap();
    repo.git(&["add", "link.txt"]);
    assert!(!stops(&repo.staged()));
    repo.commit();
    std::fs::remove_file(repo.0.join("link.txt")).unwrap();
    repo.stage("link.txt", "{B}の結果。\n");
    let out = repo.staged();
    assert!(stops(&out));
    assert!(stdout(&out).contains("\nlink.txt:1:1: "));
}

#[test]
fn outside_a_repository_is_a_failure() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("not-a-repo");
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ja-terms"))
        .arg("staged")
        .current_dir(&dir)
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn paths_are_from_the_top_of_the_repository_wherever_it_runs() {
    let repo = Repo::new();
    repo.stage(".textlintignore", "docs/fixtures/**\n");
    repo.stage("docs/fixtures/case.md", "# t\n\n{B}の結果。\n");
    repo.stage("docs/doc.md", "# t\n\n{B}の結果。\n");
    repo.stage("top.md", "# t\n\n{B}の結果。\n");
    let out = repo.staged_in("docs");
    assert!(stops(&out));
    let report = stdout(&out);
    let reported: Vec<&str> = report
        .lines()
        .filter(|l| l.ends_with("[Error/prh]"))
        .map(|l| l.split(':').next().unwrap())
        .collect();
    assert_eq!(reported, ["docs/doc.md", "top.md"]);
}
