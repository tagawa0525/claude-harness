//! リポジトリの辞書（作業ツリーの直下の `.ja-terms/prh.yml`）を、共通の辞書（テストは crate の prh.yml を渡す）に重ねて当てること
//! （ADR-0096）。
//!
//! 辞書の語を 1 行に書くと、このファイルが pre-commit の語の検査に掛かるので、崩れた語は分けて書いてつなぐ。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 共通の辞書（ビルドに埋めず、ja-terms はホームの下か、この環境変数のパスから読む。ADR-0097）。テストは crate の辞書を渡す
const COMMON_DICTIONARY: (&str, &str) = (
    ja_terms::common::COMMON_DICTIONARY_ENV,
    concat!(env!("CARGO_MANIFEST_DIR"), "/prh.yml"),
);

/// 共通の辞書の崩れた語（最初の規則の、名詞の用法の語。直す語は「確認」）
const COMMON: &str = concat!("確か", "め");

/// リポジトリの辞書の規則（ふがふが => ほげ）
const REPO_RULES: &str = "version: 1\nrules:\n  - expected: ほげ\n    pattern: ふがふが\n";

/// 共通の辞書の最初の規則と同じ expected で、別の語を止める規則
const SAME_EXPECTED: &str = "version: 1\nrules:\n  - expected: 確認\n    pattern: ぴよぴよ\n";

/// 共通の辞書の語とリポジトリの辞書の語を、どちらも名詞の用法で含む文
fn both() -> String {
    format!("# 題\n\n{COMMON}の結果とふがふがの値とぴよぴよ。\n")
}

fn without_outer_repository(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
}

/// 利用者の git の設定に左右されない git のリポジトリ
struct Repo(PathBuf);

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(base: &Path, name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = base.join(format!(
        "ja-terms-repo-dictionary-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

impl Repo {
    fn new() -> Repo {
        let repo = Repo(scratch(Path::new(env!("CARGO_TARGET_TMPDIR")), "repo"));
        std::fs::create_dir_all(repo.0.join("repo")).unwrap();
        repo.git(&["init", "-q", "-b", "main"]);
        repo
    }

    fn top(&self) -> PathBuf {
        self.0.join("repo")
    }

    fn command(&self, program: &str, dir: &Path) -> Command {
        let mut command = Command::new(program);
        without_outer_repository(&mut command);
        command
            .current_dir(dir)
            .env(COMMON_DICTIONARY.0, COMMON_DICTIONARY.1)
            .env("GIT_CONFIG_GLOBAL", self.0.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com");
        command
    }

    fn git(&self, args: &[&str]) {
        let out = self
            .command("git", &self.top())
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn write(&self, path: &str, text: &str) {
        write(&self.top().join(path), text);
    }

    /// dir で ja-terms を実行する
    fn ja_terms(&self, dir: &Path, args: &[&str], stdin: &str) -> Output {
        run(
            self.command(env!("CARGO_BIN_EXE_ja-terms"), dir),
            args,
            stdin,
        )
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn run(mut command: Command, args: &[&str], stdin: &str) -> Output {
    let mut child = command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// 報告の「誤り => 正しい語」を出た順に
fn findings(out: &Output) -> Vec<String> {
    stdout(out)
        .lines()
        .filter_map(|l| l.split_once(": ").map(|(_, rest)| rest))
        .filter_map(|rest| rest.split(" [Error/prh]").next())
        .filter(|rest| rest.contains(" => "))
        .map(|rest| {
            // check と staged は「<パス>:<行>:<列>: 」、message は「<行>:<列>: 」の後
            rest.rsplit_once(": ").map_or(rest, |(_, r)| r).to_string()
        })
        .collect()
}

fn common_finding() -> String {
    format!("{COMMON} => 確認")
}

const REPO_FINDING: &str = "ふがふが => ほげ";

#[test]
fn check_applies_the_repository_dictionary_over_the_common_one() {
    let repo = Repo::new();
    repo.write(".ja-terms/prh.yml", REPO_RULES);
    repo.write("doc.md", &both());
    let out = repo.ja_terms(&repo.top(), &["check", "doc.md"], "");
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert_eq!(findings(&out), [common_finding(), REPO_FINDING.to_string()]);
}

#[test]
fn a_subdirectory_reads_the_dictionary_at_the_top_of_the_work_tree() {
    let repo = Repo::new();
    repo.write(".ja-terms/prh.yml", REPO_RULES);
    repo.write("sub/dir/doc.md", &both());
    let out = repo.ja_terms(&repo.top().join("sub/dir"), &["check", "doc.md"], "");
    assert_eq!(findings(&out), [common_finding(), REPO_FINDING.to_string()]);
}

#[test]
fn a_dictionary_below_the_top_is_not_the_repository_dictionary() {
    // 根まで上って探さない。直下の .ja-terms/prh.yml だけを読む
    let repo = Repo::new();
    repo.write("sub/.ja-terms/prh.yml", REPO_RULES);
    repo.write("sub/doc.md", &both());
    let out = repo.ja_terms(&repo.top().join("sub"), &["check", "doc.md"], "");
    assert_eq!(findings(&out), [common_finding()]);
}

#[test]
fn a_repository_without_a_dictionary_applies_only_the_common_one() {
    let repo = Repo::new();
    repo.write("doc.md", &both());
    let out = repo.ja_terms(&repo.top(), &["check", "doc.md"], "");
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert_eq!(findings(&out), [common_finding()]);
}

#[test]
fn outside_a_work_tree_applies_only_the_common_one() {
    let repo = Repo::new();
    let outside = scratch(&std::env::temp_dir(), "outside");
    write(&outside.join(".ja-terms/prh.yml"), REPO_RULES);
    write(&outside.join("doc.md"), &both());
    let out = repo.ja_terms(&outside, &["check", "doc.md"], "");
    let _ = std::fs::remove_dir_all(&outside);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert_eq!(findings(&out), [common_finding()]);
}

#[test]
fn git_failing_inside_a_work_tree_stops_the_check() {
    // GIT_DIR があれば作業ツリーの中とみなし、git の失敗を共通の辞書だけにすり替えない
    let repo = Repo::new();
    repo.write("doc.md", &both());
    let mut command = repo.command(env!("CARGO_BIN_EXE_ja-terms"), &repo.top());
    command.env("GIT_DIR", repo.0.join("nonexistent"));
    let out = run(command, &["check", "doc.md"], "");
    assert_eq!(out.status.code(), Some(2), "{}", stdout(&out));
}

#[test]
fn a_rule_with_the_same_expected_replaces_the_common_rule() {
    // prh の Engine.merge（imports）と同じく、前の辞書（リポジトリ）にある expected の規則を、後の辞書（共通）から外す
    let repo = Repo::new();
    repo.write(".ja-terms/prh.yml", SAME_EXPECTED);
    repo.write("doc.md", &both());
    let out = repo.ja_terms(&repo.top(), &["check", "doc.md"], "");
    assert_eq!(findings(&out), ["ぴよぴよ => 確認"]);
}

#[test]
fn the_same_rule_in_both_dictionaries_reports_once() {
    // 共通の辞書から移す間、両方に同じ規則があっても 1 度だけ報告する
    let repo = Repo::new();
    let copied = format!(
        "version: 1\nrules:\n  - expected: 確認\n    pattern: /{COMMON}(?=[のでをだ：:）」]|が(?!たい|ち|てら))/\n"
    );
    repo.write(".ja-terms/prh.yml", &copied);
    repo.write("doc.md", &both());
    let out = repo.ja_terms(&repo.top(), &["check", "doc.md"], "");
    assert_eq!(findings(&out), [common_finding()]);
}

#[test]
fn message_and_commit_msg_apply_the_repository_dictionary() {
    let repo = Repo::new();
    repo.write(".ja-terms/prh.yml", REPO_RULES);
    let text = "feat: 足す\n\nふがふがを直す。\n";
    let out = repo.ja_terms(&repo.top(), &["message"], text);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert_eq!(findings(&out), [REPO_FINDING]);

    let file = repo.0.join("COMMIT_EDITMSG");
    write(&file, text);
    let out = repo.ja_terms(&repo.top(), &["commit-msg", file.to_str().unwrap()], "");
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(stdout(&out).contains(REPO_FINDING), "{}", stdout(&out));
}

#[test]
fn staged_reads_the_staged_dictionary_and_not_the_work_tree_one() {
    let repo = Repo::new();
    repo.write(".ja-terms/prh.yml", REPO_RULES);
    repo.git(&["add", ".ja-terms/prh.yml"]);
    // 作業ツリーの辞書だけを変える（ステージしない）
    repo.write(".ja-terms/prh.yml", SAME_EXPECTED);
    repo.write("doc.md", &both());
    repo.git(&["add", "doc.md"]);
    let out = repo.ja_terms(&repo.top(), &["staged"], "");
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert_eq!(findings(&out), [common_finding(), REPO_FINDING.to_string()]);
}

#[test]
fn staged_ignores_a_dictionary_that_is_not_staged() {
    let repo = Repo::new();
    repo.write(".ja-terms/prh.yml", REPO_RULES);
    repo.write("doc.md", &both());
    repo.git(&["add", "doc.md"]);
    let out = repo.ja_terms(&repo.top(), &["staged"], "");
    assert_eq!(findings(&out), [common_finding()]);
}

#[test]
fn a_worktree_reads_its_own_dictionary() {
    let repo = Repo::new();
    repo.write(".ja-terms/prh.yml", REPO_RULES);
    repo.write("doc.md", &both());
    repo.git(&["add", "."]);
    repo.git(&["commit", "-q", "-m", "init"]);
    let worktree = repo.0.join("wt");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "topic",
        worktree.to_str().unwrap(),
    ]);
    write(&worktree.join(".ja-terms/prh.yml"), SAME_EXPECTED);
    let out = repo.ja_terms(&worktree, &["check", "doc.md"], "");
    assert_eq!(findings(&out), ["ぴよぴよ => 確認"]);
    // 元の作業ツリーは、元の辞書のまま
    let out = repo.ja_terms(&repo.top(), &["check", "doc.md"], "");
    assert_eq!(findings(&out), [common_finding(), REPO_FINDING.to_string()]);
}

#[test]
fn an_explicit_dictionary_is_applied_alone() {
    let repo = Repo::new();
    repo.write(".ja-terms/prh.yml", REPO_RULES);
    repo.write("doc.md", &both());
    let given = repo.0.join("given.yml");
    write(&given, SAME_EXPECTED);
    let out = repo.ja_terms(
        &repo.top(),
        &["check", "--dictionary", given.to_str().unwrap(), "doc.md"],
        "",
    );
    assert_eq!(findings(&out), ["ぴよぴよ => 確認"]);
}

/// 読めない辞書の、理由に出るべき語
const BROKEN_DICTIONARIES: [(&str, &str); 3] = [
    ("version: 1\nrules: [\n", "読めない"),
    ("version: 1\nimports: other.yml\nrules: []\n", "imports"),
    (
        "version: 1\nrules:\n  - expected: ほげ\n    pattern: ふがふが\n    specs:\n      - from: ふがふが\n        to: ふがふが\n",
        "ふがふが",
    ),
];

#[test]
fn an_unreadable_repository_dictionary_stops_every_command() {
    for (yaml, why) in BROKEN_DICTIONARIES {
        let repo = Repo::new();
        repo.write(".ja-terms/prh.yml", yaml);
        repo.write("doc.md", "# 題\n");
        let file = repo.0.join("COMMIT_EDITMSG");
        write(&file, "feat: 足す\n\n本文。\n");
        repo.git(&["add", "."]);
        for args in [
            &["check", "doc.md"][..],
            &["message"],
            &["commit-msg", file.to_str().unwrap()],
            &["staged"],
        ] {
            let out = repo.ja_terms(&repo.top(), args, "feat: 足す\n\n本文。\n");
            let said = format!("{}{}", stdout(&out), stderr(&out));
            assert_eq!(out.status.code(), Some(2), "{args:?} {yaml:?}: {said}");
            assert!(
                said.contains(".ja-terms/prh.yml") && said.contains(why),
                "{args:?} {yaml:?}: {said}"
            );
        }
    }
}

#[test]
fn a_staged_dictionary_that_is_not_utf8_stops_like_the_work_tree_one() {
    // ステージした辞書も、作業ツリーの辞書（Dictionary::load）と同じく、UTF-8 でなければ読めないとして止める
    let repo = Repo::new();
    let path = repo.top().join(".ja-terms/prh.yml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        b"version: 1\nrules:\n  - expected: \x82\xd9\n    pattern: \x82\xd3\n",
    )
    .unwrap();
    repo.write("doc.md", "# 題\n");
    repo.git(&["add", "."]);
    for args in [&["staged"][..], &["check", "doc.md"]] {
        let out = repo.ja_terms(&repo.top(), args, "");
        let said = format!("{}{}", stdout(&out), stderr(&out));
        assert_eq!(out.status.code(), Some(2), "{args:?}: {said}");
        assert!(said.contains(".ja-terms/prh.yml"), "{args:?}: {said}");
    }
}

#[cfg(unix)]
#[test]
fn a_dangling_symlink_dictionary_stops_instead_of_being_absent() {
    // 行き先の無い symlink は「辞書が無い」でなく、読めない辞書として止める
    let repo = Repo::new();
    std::fs::create_dir_all(repo.top().join(".ja-terms")).unwrap();
    std::os::unix::fs::symlink("missing.yml", repo.top().join(".ja-terms/prh.yml")).unwrap();
    repo.write("doc.md", &both());
    let out = repo.ja_terms(&repo.top(), &["check", "doc.md"], "");
    let said = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(said.contains(".ja-terms/prh.yml"), "{said}");
}

#[cfg(unix)]
#[test]
fn a_directory_reached_through_a_symlink_reads_the_dictionary_of_its_work_tree() {
    // git と同じく、symlink を辿った実体のディレクトリの作業ツリーの辞書を読む
    let repo = Repo::new();
    repo.write(".ja-terms/prh.yml", REPO_RULES);
    repo.write("sub/doc.md", &both());
    // symlink は作業ツリーの外（根までに .git の無い所）に置く
    let outside = scratch(&std::env::temp_dir(), "link");
    let link = outside.join("link");
    std::os::unix::fs::symlink(repo.top().join("sub"), &link).unwrap();
    let out = repo.ja_terms(&link, &["check", "doc.md"], "");
    let _ = std::fs::remove_dir_all(&outside);
    assert_eq!(findings(&out), [common_finding(), REPO_FINDING.to_string()]);
}
