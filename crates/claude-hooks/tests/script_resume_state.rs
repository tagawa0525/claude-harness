//! `claude-hooks script resume-state` が、再開時に worktree、開いている PR、Slurm のジョブ、docs/issues の開いている
//! issue を節ごとに集めることを確かめる（`~/.claude/skills/resume/scripts/resume-state.sh` が呼ぶ）。sh から呼ぶ配備の
//! 確認は `claude/tests/scripts/resume-state.sh`

mod common;

use common::{fake_tool, forget_git_env, place_fake_tool};
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
    /// docs/issues を持たない、コミットが 1 つのリポジトリ（コミットの本文に Claude-Session を付ける）
    fn new() -> Repo {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "resume-state-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "main"]);
        std::fs::write(repo.app().join("a.txt"), "a\n").unwrap();
        repo.git(&["add", "a.txt"]);
        repo.git(&["commit", "-q", "-m", "init", "-m", "Claude-Session: sess-1"]);
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

    /// docs/issues/<name> を書く（コミットしない）
    fn issue(&self, name: &str, text: &str) {
        let dir = self.app().join("docs/issues");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), text).unwrap();
    }

    /// PATH の先頭に偽の gh を置き、gh_out を出させる
    fn fake_gh(&self, gh_out: &str) -> Vec<(String, String)> {
        assert!(
            fake_tool().is_file(),
            "{} が無い（cargo build --examples を先に流す）",
            fake_tool().display()
        );
        place_fake_tool(
            &self
                .root
                .join("bin")
                .join(format!("gh{}", std::env::consts::EXE_SUFFIX)),
        );
        vec![
            ("FAKE_GH_OUT".into(), gh_out.into()),
            ("FAKE_GH_EXIT".into(), "0".into()),
        ]
    }

    fn run(&self, env: &[(String, String)]) -> Output {
        let path = std::env::join_paths(
            std::iter::once(self.root.join("bin"))
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["script", "resume-state"])
            .current_dir(self.app())
            .env("PATH", path)
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .envs(env.iter().map(|(k, v)| (k, v)))
            .output()
            .unwrap()
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `== <name>` の見出しから次の見出しの前までの行
fn section(text: &str, name: &str) -> String {
    let head = format!("== {name}\n");
    let start = text
        .find(&head)
        .unwrap_or_else(|| panic!("{head} が無い: {text}"))
        + head.len();
    let rest = &text[start..];
    rest[..rest.find("== ").unwrap_or(rest.len())].to_string()
}

#[test]
fn worktrees_show_the_branch_uncommitted_changes_and_the_session_of_the_tip() {
    let repo = Repo::new();
    let wt = repo.root.join("wt-x");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feat/x",
        wt.to_str().unwrap(),
    ]);
    std::fs::write(wt.join("new.txt"), "dirty\n").unwrap();
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(0));
    let text = stdout(&out);
    assert!(text.starts_with("== worktrees\n"), "{text}");
    assert_eq!(
        section(&text, "worktrees"),
        format!(
            "{} main session=sess-1\n{} feat/x dirty session=sess-1\n",
            repo.app().display(),
            wt.display()
        )
    );
}

#[test]
fn a_tip_without_a_session_trailer_shows_no_session() {
    let repo = Repo::new();
    std::fs::write(repo.app().join("b.txt"), "b\n").unwrap();
    repo.git(&["add", "b.txt"]);
    repo.git(&["commit", "-q", "-m", "plain"]);
    let text = stdout(&repo.run(&[]));
    assert_eq!(
        section(&text, "worktrees"),
        format!("{} main\n", repo.app().display())
    );
}

#[test]
fn issues_show_only_the_open_ones_with_their_blocked_by() {
    let repo = Repo::new();
    repo.issue(
        "0002-a.md",
        "---\nstatus: open\nblocked-by: [\"#0001\"]\n---\n\n# a\n",
    );
    repo.issue(
        "0001-b.md",
        "---\nstatus: closed\nresolution: done\n---\n\n# b\n",
    );
    repo.issue(
        "0003-q.md",
        "---\nstatus: \"open\"\nblocked-by: []\n---\n\n# q\n",
    );
    // フロントマターの外の status は読まない
    repo.issue("0004-n.md", "# n\n\nstatus: open\n");
    let text = stdout(&repo.run(&[]));
    assert!(
        section(&text, "issues")
            .starts_with("docs/issues/0002-a.md blocked-by=[\"#0001\"]\ndocs/issues/0003-q.md\n"),
        "{text}"
    );
    assert!(!text.contains("0001-b.md"), "{text}");
    assert!(!text.contains("0004-n.md"), "{text}");
}

#[test]
fn no_open_issue_is_none() {
    let repo = Repo::new();
    let text = stdout(&repo.run(&[]));
    assert_eq!(section(&text, "issues"), "(なし)\n");
}

#[test]
fn without_a_github_remote_the_pr_section_gives_the_reason() {
    let repo = Repo::new();
    let env = repo.fake_gh("fake-pr-listed\n");
    let text = stdout(&repo.run(&env));
    assert_eq!(
        section(&text, "prs"),
        "(なし: gh か GitHub の remote が無い)\n"
    );
    assert!(text.contains("== slurm\n"), "{text}");
}

#[test]
fn a_github_remote_of_any_name_lists_the_open_prs() {
    let repo = Repo::new();
    repo.git(&[
        "remote",
        "add",
        "upstream",
        "https://github.com/example/app.git",
    ]);
    let env = repo.fake_gh("#7 feat/x title\n");
    let text = stdout(&repo.run(&env));
    assert_eq!(section(&text, "prs"), "#7 feat/x title\n");
}

#[test]
fn an_issue_blocked_by_an_open_issue_is_marked_blocked_with_the_next_step() {
    let repo = Repo::new();
    repo.issue("0001-b.md", "---\nstatus: open\n---\n\n# b\n");
    repo.issue(
        "0003-c.md",
        "---\nstatus: closed\nresolution: done\n---\n\n# c\n",
    );
    repo.issue(
        "0002-a.md",
        "---\nstatus: open\nblocked-by: [\"#0001\", \"#0003\"]\n---\n\n# a\n",
    );
    let text = stdout(&repo.run(&[]));
    assert_eq!(
        section(&text, "issues"),
        "docs/issues/0001-b.md\n\
         docs/issues/0002-a.md blocked-by=[\"#0001\", \"#0003\"] blocked=#0001\n\
         NEXT: blocked= の issue は、そこに挙げた issue が閉じるまで着手しない\n"
    );
}

#[test]
fn an_issue_whose_blockers_are_all_closed_is_not_marked() {
    let repo = Repo::new();
    repo.issue(
        "0001-b.md",
        "---\nstatus: closed\nresolution: done\n---\n\n# b\n",
    );
    repo.issue(
        "0002-a.md",
        "---\nstatus: open\nblocked-by: [\"#0001\"]\n---\n\n# a\n",
    );
    let text = stdout(&repo.run(&[]));
    assert_eq!(
        section(&text, "issues"),
        "docs/issues/0002-a.md blocked-by=[\"#0001\"]\n"
    );
}

#[test]
fn a_blocker_that_is_not_in_docs_issues_is_marked_unknown() {
    let repo = Repo::new();
    repo.issue(
        "0002-a.md",
        "---\nstatus: open\nblocked-by: [\"#0009\"]\n---\n\n# a\n",
    );
    let text = stdout(&repo.run(&[]));
    assert_eq!(
        section(&text, "issues"),
        "docs/issues/0002-a.md blocked-by=[\"#0009\"] blocker-unknown=#0009\n\
         NEXT: blocker-unknown= の issue は、blocked-by の相手が docs/issues に無いか、相手の status を読めない。閉じたかを確かめられないので、着手の前に blocked-by か相手の status を直すか、ユーザーに確かめる\n"
    );
}

#[test]
fn a_blocker_without_a_readable_status_or_a_reference_that_is_not_an_issue_is_unknown() {
    let repo = Repo::new();
    // フロントマターの無い相手、# の無い参照、5 桁の参照（番号の打ち間違い）
    repo.issue("0001-old.md", "# old\n");
    repo.issue("0012-x.md", "---\nstatus: closed\n---\n\n# x\n");
    repo.issue(
        "0002-a.md",
        "---\nstatus: open\nblocked-by: [\"#0001\", \"0012\", \"#00012\"]\n---\n\n# a\n",
    );
    let text = stdout(&repo.run(&[]));
    let issues = section(&text, "issues");
    assert!(
        issues.starts_with(
            "docs/issues/0002-a.md blocked-by=[\"#0001\", \"0012\", \"#00012\"] \
             blocker-unknown=#0001,0012,#00012\n"
        ),
        "{issues}"
    );
    assert!(
        issues.contains("NEXT: blocker-unknown= の issue は"),
        "{issues}"
    );
    assert!(!issues.contains("blocked="), "{issues}");
}

#[test]
fn a_blocked_by_that_cannot_be_parsed_is_unknown() {
    let repo = Repo::new();
    repo.issue("0003-c.md", "---\nstatus: open\n---\n\n# c\n");
    repo.issue(
        "0002-a.md",
        "---\nstatus: open\nblocked-by:\n  - \"#0003\"\n---\n\n# a\n",
    );
    let text = stdout(&repo.run(&[]));
    assert!(
        section(&text, "issues").contains("docs/issues/0002-a.md blocker-unknown=unparsed\n"),
        "{text}"
    );
}

#[test]
fn a_status_with_a_trailing_space_or_a_comment_is_read_like_design_records() {
    let repo = Repo::new();
    repo.issue("0001-b.md", "---\nstatus: closed \n---\n\n# b\n");
    repo.issue("0003-c.md", "---\nstatus: closed # done\n---\n\n# c\n");
    repo.issue(
        "0002-a.md",
        "---\nstatus: open\nblocked-by: [\"#0001\", \"#0003\"]\n---\n\n# a\n",
    );
    let text = stdout(&repo.run(&[]));
    assert_eq!(
        section(&text, "issues"),
        "docs/issues/0002-a.md blocked-by=[\"#0001\", \"#0003\"]\n"
    );
}

#[test]
fn a_file_with_a_date_for_a_number_is_not_an_issue_number() {
    let repo = Repo::new();
    repo.issue("20261010-notes.md", "---\nstatus: closed\n---\n\n# n\n");
    repo.issue(
        "0002-a.md",
        "---\nstatus: open\nblocked-by: [\"#0010\"]\n---\n\n# a\n",
    );
    let text = stdout(&repo.run(&[]));
    assert!(
        section(&text, "issues")
            .starts_with("docs/issues/0002-a.md blocked-by=[\"#0010\"] blocker-unknown=#0010\n"),
        "{text}"
    );
}

#[test]
fn a_blocker_closed_as_a_duplicate_of_an_open_issue_still_blocks() {
    let repo = Repo::new();
    repo.issue(
        "0001-b.md",
        "---\nstatus: closed\nresolution: duplicate\nduplicate-of: \"#0005\"\n---\n\n# b\n",
    );
    repo.issue("0005-e.md", "---\nstatus: open\n---\n\n# e\n");
    repo.issue(
        "0002-a.md",
        "---\nstatus: open\nblocked-by: [\"#0001\"]\n---\n\n# a\n",
    );
    let text = stdout(&repo.run(&[]));
    assert!(
        section(&text, "issues")
            .starts_with("docs/issues/0002-a.md blocked-by=[\"#0001\"] blocked=#0001\n"),
        "{text}"
    );
}
