//! `claude-hooks script prose-size [--since <日時>] [--until <日時>] [--memory <ディレクトリ>] [--repo <パス>]...` が、区間の
//! 前後の文章（メモリ、スキル、CLAUDE.md）の大きさをバイトで出すことを確かめる（ADR-0089。
//! `.claude/skills/retro/scripts/prose-size.sh` が呼ぶ）。sh から呼ぶ配備の確認は `.claude/tests/scripts/retro.sh`

mod common;

use common::forget_git_env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const SINCE: &str = "2026-11-10T00:00+09:00";
const UNTIL: &str = "2026-11-11T00:00+09:00";

/// テストごとの HOME。リポジトリと `~/.claude` をこの下に作る
struct Home {
    root: PathBuf,
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Home {
    fn new() -> Home {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "prose-size-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Home { root }
    }

    fn git(&self, dir: &Path, args: &[&str], date: Option<&str>) {
        let mut cmd = Command::new("git");
        forget_git_env(&mut cmd)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1");
        if let Some(date) = date {
            cmd.env("GIT_COMMITTER_DATE", date)
                .env("GIT_AUTHOR_DATE", date);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// 作業ツリーの全体を、committer の日時を指定してコミットする
    fn commit_at(&self, dir: &Path, date: &str) {
        self.git(dir, &["add", "-A"], None);
        self.git(
            dir,
            &["commit", "-q", "-m", &format!("chore: at {date}")],
            Some(date),
        );
    }

    /// スキルの .md、スクリプト、CLAUDE.md、ほかの .md を 3 日に分けてコミットしたリポジトリ（既定ブランチは main）。
    /// skills は 100 → 70 → 70、claude-md は 50 → 30 → 80
    fn repo(&self) -> PathBuf {
        let repo = self.root.join("prose-repo");
        std::fs::create_dir_all(&repo).unwrap();
        self.git(&repo, &["init", "-q", "-b", "main"], None);
        write_bytes(&repo.join(".claude/skills/a/SKILL.md"), 100);
        write_bytes(&repo.join(".claude/skills/a/scripts/x.sh"), 50);
        write_bytes(&repo.join(".claude/CLAUDE.md"), 30);
        write_bytes(&repo.join("CLAUDE.md"), 20);
        write_bytes(&repo.join("docs/x.md"), 40);
        self.commit_at(&repo, "2026-11-09T12:00:00+09:00");
        write_bytes(&repo.join(".claude/skills/a/SKILL.md"), 60);
        write_bytes(&repo.join(".claude/skills/b/references/r.md"), 10);
        write_bytes(&repo.join(".claude/skills/a/scripts/x.sh"), 500);
        std::fs::remove_file(repo.join("CLAUDE.md")).unwrap();
        self.commit_at(&repo, "2026-11-10T12:00:00+09:00");
        write_bytes(&repo.join(".claude/CLAUDE.md"), 80);
        self.commit_at(&repo, "2026-11-11T12:00:00+09:00");
        repo
    }

    /// `~/.claude` の git に 2 回退避したメモリのディレクトリ（100 → 210）。今のディスクは 50
    fn memory(&self) -> PathBuf {
        let home = self.claude_home();
        let memory = home.join("projects/-p/memory");
        self.git(&self.root, &["init", "-q", ".claude"], None);
        write_bytes(&memory.join("MEMORY.md"), 10);
        write_bytes(&memory.join("a.md"), 90);
        self.commit_at(&home, "2026-11-09T12:00:00+09:00");
        write_bytes(&memory.join("a.md"), 200);
        self.commit_at(&home, "2026-11-10T12:00:00+09:00");
        self.git(&home, &["update-ref", "refs/memory-backup", "HEAD"], None);
        write_bytes(&memory.join("a.md"), 40);
        memory
    }

    fn claude_home(&self) -> PathBuf {
        self.root.join(".claude")
    }

    fn prose(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
        forget_git_env(&mut cmd)
            .args(["script", "prose-size"])
            .args(args)
            .env("HOME", &self.root)
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap()
    }
}

/// n バイトのファイルを書く
fn write_bytes(path: &Path, n: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![b'x'; n]).unwrap();
}

fn s(path: &Path) -> String {
    path.display().to_string()
}

/// 終了コード、標準出力、標準エラー
fn parts(out: &Output) -> (i32, String, String) {
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout.clone()).unwrap(),
        String::from_utf8(out.stderr.clone()).unwrap(),
    )
}

#[test]
fn skills_and_claude_md_are_measured_on_the_default_branch_before_since_and_before_until() {
    let home = Home::new();
    let repo = home.repo();
    let r = s(&repo);
    let (code, out, err) = parts(&home.prose(&["--since", SINCE, "--until", UNTIL, "--repo", &r]));
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        out,
        format!(
            "SINCE: 2026-11-10T00:00:00+09:00\nUNTIL: 2026-11-11T00:00:00+09:00\n\
             PROSE: skills 100 70 -30 {r}\nPROSE: claude-md 50 30 -20 {r}\n"
        )
    );
}

#[test]
fn the_bounds_are_printed_in_jst() {
    let home = Home::new();
    let r = s(&home.repo());
    let (code, out, _) = parts(&home.prose(&[
        "--since",
        "2026-11-09T15:00Z",
        "--until",
        "2026-11-10T16:30:05-01:00",
        "--repo",
        &r,
    ]));
    assert_eq!(code, 0);
    assert!(
        out.starts_with("SINCE: 2026-11-10T00:00:00+09:00\nUNTIL: 2026-11-11T02:30:05+09:00\n")
    );
}

#[test]
fn a_commit_exactly_at_since_is_counted_after_the_start() {
    let home = Home::new();
    let r = s(&home.repo());
    let (code, out, _) = parts(&home.prose(&[
        "--since",
        "2026-11-10T12:00+09:00",
        "--until",
        UNTIL,
        "--repo",
        &r,
    ]));
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!("PROSE: skills 100 70 -30 {r}\n")),
        "{out}"
    );
}

#[test]
fn without_until_the_size_after_is_the_head_of_the_given_path() {
    let home = Home::new();
    let r = s(&home.repo());
    let (code, out, err) = parts(&home.prose(&["--since", SINCE, "--repo", &r]));
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        out,
        format!(
            "SINCE: 2026-11-10T00:00:00+09:00\nUNTIL: now\n\
             PROSE: skills 100 70 -30 {r}\nPROSE: claude-md 50 80 +30 {r}\n"
        )
    );
}

#[test]
fn a_worktree_adds_the_commits_of_its_branch() {
    let home = Home::new();
    let repo = home.repo();
    let wt = home.root.join("prose-wt");
    home.git(
        &repo,
        &["worktree", "add", "-q", "-b", "topic", &s(&wt)],
        None,
    );
    write_bytes(&wt.join(".claude/skills/c/SKILL.md"), 5);
    home.commit_at(&wt, "2026-11-11T13:00:00+09:00");
    let (code, out, _) = parts(&home.prose(&["--since", SINCE, "--repo", &s(&wt)]));
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!("PROSE: skills 100 75 -25 {}\n", s(&wt))),
        "{out}"
    );
}

#[test]
fn a_worktree_from_an_old_default_branch_adds_both_the_later_default_branch_and_its_branch() {
    let home = Home::new();
    let repo = home.repo();
    let wt = home.root.join("prose-old-wt");
    home.git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "old-topic",
            &s(&wt),
            "HEAD~1",
        ],
        None,
    );
    write_bytes(&wt.join(".claude/skills/d/SKILL.md"), 7);
    home.commit_at(&wt, "2026-11-11T14:00:00+09:00");
    let w = s(&wt);
    let (code, out, _) = parts(&home.prose(&["--since", SINCE, "--repo", &w]));
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!("PROSE: skills 100 77 -23 {w}\n")),
        "{out}"
    );
    assert!(
        out.contains(&format!("PROSE: claude-md 50 80 +30 {w}\n")),
        "{out}"
    );
}

#[test]
fn a_checkout_behind_the_default_branch_still_counts_the_default_branch_now() {
    let home = Home::new();
    let repo = home.repo();
    let clone = home.root.join("prose-clone");
    home.git(&home.root, &["clone", "-q", &s(&repo), &s(&clone)], None);
    home.git(&clone, &["reset", "-q", "--hard", "HEAD~1"], None);
    let c = s(&clone);
    let (code, out, _) = parts(&home.prose(&["--since", SINCE, "--repo", &c]));
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!("PROSE: claude-md 50 80 +30 {c}\n")),
        "{out}"
    );
}

#[test]
fn a_head_without_a_common_ancestor_fails_after_the_lines_of_the_other_repositories() {
    let home = Home::new();
    let repo = home.repo();
    let orphan = home.root.join("prose-orphan");
    home.git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "--orphan",
            "-b",
            "orphan",
            &s(&orphan),
        ],
        None,
    );
    write_bytes(&orphan.join(".claude/skills/e/SKILL.md"), 9);
    home.commit_at(&orphan, "2026-11-11T15:00:00+09:00");
    let (o, r) = (s(&orphan), s(&repo));
    let (code, out, err) = parts(&home.prose(&["--since", SINCE, "--repo", &o, "--repo", &r]));
    assert_eq!(code, 1);
    assert_eq!(
        out,
        format!(
            "SINCE: 2026-11-10T00:00:00+09:00\nUNTIL: now\n\
             PROSE: skills 100 70 -30 {r}\nPROSE: claude-md 50 80 +30 {r}\n"
        )
    );
    assert_eq!(
        err,
        format!(
            "ERROR: {o} の HEAD と既定ブランチ（main）に共通の祖先が無いので、HEAD の変えた分が分かりません\n"
        )
    );
}

#[test]
fn a_repository_without_a_default_branch_fails_after_the_other_lines() {
    let home = Home::new();
    let repo = home.repo();
    let empty = home.root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    home.git(&empty, &["init", "-q", "-b", "trunk"], None);
    let (e, r) = (s(&empty), s(&repo));
    let (code, out, err) = parts(&home.prose(&["--since", SINCE, "--repo", &e, "--repo", &r]));
    assert_eq!(code, 1);
    assert!(
        out.contains(&format!("PROSE: skills 100 70 -30 {r}\n")),
        "{out}"
    );
    assert!(!out.contains(&e), "{out}");
    assert_eq!(
        err,
        format!("ERROR: {e} に既定ブランチ（origin/HEAD、main、master）がありません\n")
    );
}

#[test]
fn a_repository_without_commits_before_since_counts_0_before() {
    let home = Home::new();
    let r = s(&home.repo());
    let (code, out, _) = parts(&home.prose(&[
        "--since",
        "2026-11-01T00:00+09:00",
        "--until",
        UNTIL,
        "--repo",
        &r,
    ]));
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!("PROSE: skills 0 70 +70 {r}\n")),
        "{out}"
    );
}

#[test]
fn memory_compares_the_last_backup_before_since_with_the_files_on_disk() {
    let home = Home::new();
    let memory = home.memory();
    let m = s(&memory);
    let (code, out, err) = parts(&home.prose(&["--since", SINCE, "--memory", &m]));
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        out,
        format!("SINCE: 2026-11-10T00:00:00+09:00\nUNTIL: now\nPROSE: memory 100 50 -50 {m}\n")
    );
}

#[test]
fn a_trailing_slash_of_the_memory_directory_is_dropped() {
    let home = Home::new();
    let m = s(&home.memory());
    let (code, out, _) = parts(&home.prose(&["--since", SINCE, "--memory", &format!("{m}/")]));
    assert_eq!(code, 0);
    assert!(
        out.ends_with(&format!("PROSE: memory 100 50 -50 {m}\n")),
        "{out}"
    );
}

#[test]
fn with_until_memory_after_is_the_last_backup_before_until() {
    let home = Home::new();
    let m = s(&home.memory());
    let (code, out, _) = parts(&home.prose(&["--since", SINCE, "--until", UNTIL, "--memory", &m]));
    assert_eq!(code, 0);
    assert!(
        out.ends_with(&format!("PROSE: memory 100 210 +110 {m}\n")),
        "{out}"
    );
}

#[test]
fn memory_lines_follow_the_repository_lines() {
    let home = Home::new();
    let m = s(&home.memory());
    let r = s(&home.repo());
    let (code, out, _) = parts(&home.prose(&["--since", SINCE, "--memory", &m, "--repo", &r]));
    assert_eq!(code, 0);
    let kinds: Vec<&str> = out
        .lines()
        .filter_map(|l| l.strip_prefix("PROSE: "))
        .map(|l| l.split(' ').next().unwrap())
        .collect();
    assert_eq!(kinds, ["skills", "claude-md", "memory"]);
}

// `"` と `\` は Windows のファイル名に使えない
#[cfg(unix)]
#[test]
fn a_memory_directory_with_quotes_and_backslashes_counts_only_its_own_backup() {
    let home = Home::new();
    home.memory();
    let claude = home.claude_home();
    let quoted = claude.join("projects/-q\"\\x/memory");
    write_bytes(&quoted.join("a.md"), 3);
    home.commit_at(&claude, "2026-11-11T12:00:00+09:00");
    home.git(&claude, &["update-ref", "refs/memory-backup", "HEAD"], None);
    let q = s(&quoted);
    let (code, out, _) = parts(&home.prose(&[
        "--since",
        SINCE,
        "--until",
        "2026-11-12T00:00+09:00",
        "--memory",
        &q,
    ]));
    assert_eq!(code, 0);
    assert!(
        out.ends_with(&format!("PROSE: memory 0 3 +3 {q}\n")),
        "{out}"
    );
}

#[test]
fn a_project_without_memory_counts_0_and_continues() {
    let home = Home::new();
    home.memory();
    let none = s(&home.claude_home().join("projects/-none/memory"));
    let r = s(&home.repo());
    let (code, out, _) = parts(&home.prose(&["--since", SINCE, "--memory", &none, "--repo", &r]));
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!("PROSE: memory 0 0 0 {none}\n")),
        "{out}"
    );
    assert!(out.contains("PROSE: skills"), "{out}");
}

#[test]
fn without_the_backup_ref_and_the_directory_memory_counts_0() {
    let home = Home::new();
    let m = s(&home.claude_home().join("projects/-p/memory"));
    let (code, out, err) = parts(&home.prose(&["--since", SINCE, "--memory", &m]));
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(
        out.ends_with(&format!("PROSE: memory 0 0 0 {m}\n")),
        "{out}"
    );
}

#[test]
fn memory_without_a_backup_before_since_fails_after_the_other_lines() {
    let home = Home::new();
    let m = s(&home.memory());
    let r = s(&home.repo());
    let (code, out, err) = parts(&home.prose(&[
        "--since",
        "2026-11-01T00:00+09:00",
        "--memory",
        &m,
        "--repo",
        &r,
    ]));
    assert_eq!(code, 1);
    assert!(out.contains("PROSE: skills"), "{out}");
    assert!(!out.contains("PROSE: memory"), "{out}");
    let claude = s(&home.claude_home());
    assert_eq!(
        err,
        format!(
            "ERROR: 2026-11-01T00:00:00+09:00 より前のメモリの退避（{claude} の refs/memory-backup）が無いので、前の大きさが分かりません: {m}\n"
        )
    );
}

#[test]
fn a_memory_directory_is_matched_by_path_components_not_by_the_spelling() {
    // 重なった区切りと、Windows の `\` と `/` の混ざった書き方でも、退避の中の同じディレクトリを数える
    let home = Home::new();
    home.memory();
    let m = format!(
        "{}{}projects//-p/memory",
        s(&home.claude_home()),
        std::path::MAIN_SEPARATOR
    );
    let (code, out, _) = parts(&home.prose(&["--since", SINCE, "--until", UNTIL, "--memory", &m]));
    assert_eq!(code, 0);
    assert!(
        out.ends_with(&format!("PROSE: memory 100 210 +110 {m}\n")),
        "{out}"
    );
}

#[test]
fn a_memory_directory_outside_claude_home_fails() {
    let home = Home::new();
    let m = s(&home.root.join("elsewhere/memory"));
    let (code, out, err) = parts(&home.prose(&["--since", SINCE, "--memory", &m]));
    assert_eq!(code, 1);
    assert!(!out.contains("PROSE:"), "{out}");
    let claude = s(&home.claude_home());
    assert_eq!(
        err,
        format!(
            "ERROR: メモリのディレクトリが {claude} の下にありません（退避は {claude} の git にあります）: {m}\n"
        )
    );
}

#[test]
fn without_since_the_range_starts_at_midnight_today_in_jst() {
    let home = Home::new();
    let r = s(&home.repo());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let (code, out, _) = parts(&home.prose(&["--repo", &r]));
    assert_eq!(code, 0);
    // JST の今日の日付（1970-01-01 からの日数を暦に直す）
    let days = (now + 9 * 3600).div_euclid(86_400);
    let date = civil(days);
    assert!(
        out.starts_with(&format!("SINCE: {date}T00:00:00+09:00\nUNTIL: now\n")),
        "{out}"
    );
}

/// 1970-01-01 からの日数の `YYYY-MM-DD`（Howard Hinnant の civil_from_days）
fn civil(z: i64) -> String {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[test]
fn a_bound_without_an_offset_fails() {
    let home = Home::new();
    let r = s(&home.repo());
    let (code, out, err) = parts(&home.prose(&["--since", "2026-11-10T00:00", "--repo", &r]));
    assert_eq!((code, out.as_str()), (1, ""));
    assert_eq!(
        err,
        "ERROR: --since は時差つきの ISO 8601 で書いてください（例 2026-11-06T00:00+09:00、2026-11-05T15:00Z）: 2026-11-10T00:00\n"
    );
}

#[test]
fn a_bound_that_is_not_a_date_fails() {
    let home = Home::new();
    let r = s(&home.repo());
    let (code, out, err) = parts(&home.prose(&["--until", "2026-02-29T00:00Z", "--repo", &r]));
    assert_eq!((code, out.as_str()), (1, ""));
    assert_eq!(
        err,
        "ERROR: --until の日時を読めません: 2026-02-29T00:00Z\n"
    );
}

#[test]
fn since_not_before_until_fails() {
    let home = Home::new();
    let r = s(&home.repo());
    let (code, out, err) = parts(&home.prose(&[
        "--since",
        "2026-11-11T00:00Z",
        "--until",
        "2026-11-10T00:00Z",
        "--repo",
        &r,
    ]));
    assert_eq!((code, out.as_str()), (1, ""));
    assert_eq!(
        err,
        "ERROR: --since（2026-11-11T09:00:00+09:00）は --until（2026-11-10T09:00:00+09:00）より前にしてください\n"
    );
}

const USAGE: &str = "Usage: prose-size.sh [--since <日時>] [--until <日時>] [--memory <メモリのディレクトリ>] [--repo <パス>]...\n";

#[test]
fn without_repo_and_memory_the_usage_is_printed() {
    let home = Home::new();
    let (code, out, err) = parts(&home.prose(&["--since", "2026-11-10T00:00Z"]));
    assert_eq!((code, out.as_str(), err.as_str()), (1, "", USAGE));
}

#[test]
fn an_unknown_option_or_a_missing_value_prints_the_usage() {
    let home = Home::new();
    for args in [
        &["--from", "2026-11-10T00:00Z", "--repo", "x"][..],
        &["--repo"][..],
        &["--help"][..],
    ] {
        let (code, out, err) = parts(&home.prose(args));
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (1, "", USAGE),
            "{args:?}"
        );
    }
}
