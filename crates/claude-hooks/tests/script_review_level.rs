//! `claude-hooks script review-level [branch] [--base BASE]` が、ローカルの /code-review に渡すレベルと対象を決める
//! ことを確かめる（ADR-0087。`~/.claude/scripts/review-level.sh` が呼ぶ）。文書の判定の細かい場合分け（ADR の
//! 「## 変更」の節、.claude の下の文書、移動）と比較元の決め方は、sh から呼ぶ `claude/tests/scripts/review-level.sh` も
//! 確かめる

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
            "review-level-{}-{}",
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
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(&self.root)
            // 利用者のグローバルの設定（core.hooksPath のグローバルの git hook など）を読まない
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
        self.run_in(".", args)
    }

    /// リポジトリの中の dir（リポジトリの直下からの相対）で起動する
    fn run_in(&self, dir: &str, args: &[&str]) -> Output {
        forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["script", "review-level"])
            .args(args)
            .current_dir(self.root.join(dir))
            .output()
            .unwrap()
    }

    /// 決定の節と「## 変更」の節を持つ ADR を main に置き、feat/adr でその path の決定の節を書き換える
    fn adr_with_a_frozen_change(&self, path: &str) {
        let file = self.root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "# ADR\n\n## 決定と理由\n\nA\n\n## 変更\n").unwrap();
        self.git(&["add", path]);
        self.git(&["commit", "-q", "-m", "adr"]);
        self.git(&["switch", "-q", "-c", "feat/adr"]);
        std::fs::write(&file, "# ADR\n\n## 決定と理由\n\nB\n\n## 変更\n").unwrap();
        self.git(&["add", path]);
        self.git(&["commit", "-q", "-m", "change the decision"]);
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn documents_only_is_low() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/docs"]);
    repo.commit("docs/plans/x.md", "plan");
    repo.commit("docs/issues/y.md", "issue");
    let out = repo.run(&[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let root = repo.git(&["rev-parse", "--show-toplevel"]);
    assert_eq!(
        stdout(&out),
        format!(
            "BASE: main\nTARGET: feat/docs\nWORKTREE: {root}\nFILES: 2\nLEVEL: low\n\
             NEXT: /code-review low {root}（1 周目はこの LEVEL で回す。直した後の周のレベルは /git-merge の「直す指摘の基準」の「終わり」の項で決める）\n",
            root = root.trim()
        )
    );
}

#[test]
fn the_next_line_names_the_worktree_quoted_for_the_shell() {
    let repo = Repo::new();
    repo.commit("src/a.rs", "a");
    let dir = repo.root.join("work tree");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feat/code",
        dir.to_str().unwrap(),
    ]);
    std::fs::write(dir.join("b.rs"), "b\n").unwrap();
    let at = dir.to_str().unwrap();
    repo.git(&["-C", at, "add", "b.rs"]);
    repo.git(&["-C", at, "commit", "-q", "-m", "b"]);
    let out = repo.run(&["feat/code", "--base", "main"]);
    assert_eq!(out.status.code(), Some(0));
    let next = stdout(&out)
        .lines()
        .find(|l| l.starts_with("NEXT: "))
        .unwrap_or_else(|| panic!("{}", stdout(&out)))
        .to_string();
    let worktree = repo
        .git(&["-C", at, "rev-parse", "--show-toplevel"])
        .trim()
        .to_string();
    assert!(
        next.starts_with(&format!("NEXT: /code-review high '{worktree}'（")),
        "{next}"
    );
}

#[test]
fn a_branch_checked_out_nowhere_is_reviewed_by_the_pr_url() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/away"]);
    repo.commit("src/a.rs", "a");
    repo.git(&["switch", "-q", "main"]);
    let out = repo.run(&["feat/away"]);
    assert_eq!(out.status.code(), Some(0));
    let text = stdout(&out);
    assert!(text.contains("WORKTREE: none\n"), "{text}");
    let next = text
        .lines()
        .find(|l| l.starts_with("NEXT: "))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(
        next.starts_with("NEXT: /code-review high <PR の URL>（"),
        "{next}"
    );
    assert!(next.contains("git worktree add"), "{next}");
}

#[test]
fn documents_under_the_harness_source_directory_are_low() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/rule-docs"]);
    repo.commit("claude/README.md", "readme");
    repo.commit("claude/skills/x/references/r.md", "reference");
    let out = repo.run(&["feat/rule-docs", "--base", "main"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout(&out).contains("FILES: 2\nLEVEL: low\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_file_that_is_not_a_document_is_high() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/mixed"]);
    repo.commit("docs/plans/x.md", "plan");
    repo.commit("claude/scripts/x.sh", "script");
    let out = repo.run(&["feat/mixed", "--base", "main"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout(&out).contains("FILES: 2\nLEVEL: high\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn an_empty_diff_or_an_unknown_ref_is_an_error() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/empty"]);
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("差分が空"));
    let out = repo.run(&["no-such-branch"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no-such-branch を解決できません"));
}

#[test]
fn bad_arguments_are_errors() {
    let repo = Repo::new();
    for args in [&["--base"][..], &["--force"][..]] {
        let out = repo.run(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(stdout(&out), "", "{args:?}");
    }
}

#[test]
fn a_change_to_a_frozen_section_of_an_adr_is_high_from_a_subdirectory() {
    // 差分のパスはリポジトリの直下から。cwd からの相対で読むと差分が空になり、low に倒れる
    let repo = Repo::new();
    repo.commit("src/main.rs", "src");
    repo.git(&["switch", "-q", "main"]);
    repo.adr_with_a_frozen_change("docs/adr/0001-x.md");
    let out = repo.run_in("src", &[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout(&out).contains("LEVEL: high\n"), "{}", stdout(&out));
}

#[test]
fn a_change_to_a_frozen_section_of_an_adr_is_high_with_colored_diffs() {
    // 利用者の設定の色で hunk の見出しを読めなくなっても、low に倒れない
    let repo = Repo::new();
    repo.git(&["config", "color.ui", "always"]);
    repo.adr_with_a_frozen_change("docs/adr/0001-x.md");
    let out = repo.run(&[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout(&out).contains("LEVEL: high\n"), "{}", stdout(&out));
}

#[test]
fn a_document_with_a_japanese_name_is_a_document() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/ja"]);
    repo.commit("docs/issues/日本語.md", "issue");
    let out = repo.run(&[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout(&out).contains("LEVEL: low\n"), "{}", stdout(&out));
}

#[test]
fn a_small_change_to_settings_files_is_medium() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/settings"]);
    repo.commit("claude-settings.json", "allow");
    repo.commit(".worktreelink", "references");
    repo.commit("docs/plans/x.md", "plan");
    let out = repo.run(&[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout(&out).contains("FILES: 3\nLEVEL: medium\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_change_of_more_than_twenty_lines_to_settings_files_is_high() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/settings"]);
    repo.commit("a.toml", "a");
    let lines: String = (0..20).map(|i| format!("k{i} = {i}\n")).collect();
    std::fs::write(repo.root.join("b.yaml"), lines).unwrap();
    repo.git(&["add", "b.yaml"]);
    repo.git(&["commit", "-q", "-m", "b"]);
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("LEVEL: high\n"), "{}", stdout(&out));
}

#[test]
fn a_github_workflow_is_not_a_settings_file() {
    // CI の手順はゲートの挙動を変えるので、短くても設定に数えない
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/ci"]);
    repo.commit(".github/workflows/ci.yml", "ci");
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("LEVEL: high\n"), "{}", stdout(&out));
}
