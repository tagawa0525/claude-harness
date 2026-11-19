//! `claude-hooks script check-each-commit <base> -- <コマンド…>` が、`<base>..HEAD` の各コミットを一時の worktree で検査し、
//! `<短縮hash> <件名> exit=<N>` を古い順に出し、終わり（失敗と中断を含む）に一時の worktree とディレクトリを消すことを
//! 確かめる（ADR-0101。`~/.claude/scripts/check-each-commit.sh` が呼ぶ）。引数の読み方と、起動できなかったコマンドの
//! 終了コードは src/script/check_each_commit.rs の単体のテストが確かめる。
//!
//! 検査のコマンドに sh を使うテスト（作業ツリーの中身で結果が変わる、書き込み不可のファイルを残す、失敗する git を
//! 置く、シグナルを送る）は Unix でだけ確かめる。Windows では、偽のコマンド（examples/fake_tool.rs）で流れを確かめる

mod common;

use common::forget_git_env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Repo {
    /// テストの置き場（リポジトリ、一時のディレクトリの親、記録のファイル）
    root: PathBuf,
}

impl Drop for Repo {
    fn drop(&mut self) {
        make_writable(&self.root);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// 書き込み不可のディレクトリを残すテストの後片付け
fn make_writable(path: &Path) {
    #[cfg(unix)]
    let _ = Command::new("chmod")
        .arg("-R")
        .arg("u+w")
        .arg(path)
        .status();
    #[cfg(not(unix))]
    let _ = path;
}

/// git の呼び出しに、利用者の設定（グローバルの git hook など）を読ませない
fn isolate<'a>(cmd: &'a mut Command, root: &Path) -> &'a mut Command {
    forget_git_env(cmd)
        .env("GIT_CONFIG_GLOBAL", root.join("no-global-config"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
}

impl Repo {
    /// main に 1 コミット、feat/x に「feat: first」（a.txt）、「feat: second breaks」（BAD）、「fix: third repairs」（BAD を
    /// 消す）の 3 コミットを持つリポジトリ
    fn new() -> Repo {
        let repo = Repo::empty();
        repo.git(&["switch", "-q", "-c", "feat/x"]);
        repo.commit("a.txt", "a", "feat: first");
        repo.commit("BAD", "bad", "feat: second breaks");
        repo.git(&["rm", "-q", "BAD"]);
        repo.git(&["commit", "-q", "-m", "fix: third repairs"]);
        repo
    }

    /// main に 1 コミットだけを持つリポジトリ
    fn empty() -> Repo {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "check-each-commit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        make_writable(&root);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::create_dir_all(root.join("tmp")).unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.commit("README.md", "init", "init");
        repo
    }

    fn dir(&self) -> PathBuf {
        self.root.join("app")
    }

    fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }

    fn git(&self, args: &[&str]) -> String {
        let out = isolate(&mut Command::new("git"), &self.root)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(self.dir())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    fn commit(&self, path: &str, content: &str, msg: &str) {
        let file = self.dir().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, format!("{content}\n")).unwrap();
        self.git(&["add", path]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    /// 一時のディレクトリの親をテストの置き場の tmp にして起動する
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
        isolate(&mut cmd, &self.root)
            .args(["script", "check-each-commit"])
            .args(args)
            .current_dir(self.dir())
            .env("TMPDIR", self.tmp())
            .env("TMP", self.tmp())
            .env("TEMP", self.tmp())
            .stdin(Stdio::null());
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// 一時のディレクトリの親に残ったもの
    fn left_in_tmp(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.tmp())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect()
    }

    /// `git worktree list` の行の数（一時の worktree の登録が残らなければ 1）
    fn worktrees(&self) -> usize {
        self.git(&["worktree", "list"]).lines().count()
    }

    fn short(&self, rev: &str) -> String {
        self.git(&["rev-parse", "--short", rev])
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// 呼び出しを数える偽のコマンド。n 回目の起動の終了コードを FAKE_TOOL_EXIT_<n> で決め、作業ディレクトリを記録する
fn fake_tool_command(repo: &Repo, args: &[&str], exits: &[(usize, &str)]) -> Command {
    let mut cmd = repo.command(&[&["main", "--"], args].concat());
    cmd.env("FAKE_TOOL_CALLS", repo.root.join("calls"))
        .env("FAKE_TOOL_CWD", repo.root.join("cwd"));
    for (n, code) in exits {
        cmd.env(format!("FAKE_TOOL_EXIT_{n}"), code);
    }
    cmd
}

#[test]
fn each_commit_is_checked_oldest_first_and_a_failing_one_makes_the_whole_fail() {
    let repo = Repo::new();
    let tool = common::fake_tool();
    let out = fake_tool_command(&repo, &[tool.to_str().unwrap()], &[(2, "1")])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        format!(
            "{} feat: first exit=0\n{} feat: second breaks exit=1\n{} fix: third repairs exit=0\n",
            repo.short("HEAD~2"),
            repo.short("HEAD~1"),
            repo.short("HEAD")
        )
    );
    // 検査は一時の worktree の根で走る（呼んだ作業ツリーではない）
    let cwd = std::fs::read_to_string(repo.root.join("cwd")).unwrap();
    assert_eq!(cwd.lines().count(), 3);
    assert!(cwd.lines().all(|l| Path::new(l).ends_with("wt")), "{cwd}");
    assert!(repo.left_in_tmp().is_empty(), "{:?}", repo.left_in_tmp());
    assert_eq!(repo.worktrees(), 1);
}

#[test]
fn all_commits_passing_ends_with_0() {
    let repo = Repo::new();
    let tool = common::fake_tool();
    let out = fake_tool_command(&repo, &[tool.to_str().unwrap()], &[])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out).lines().count(), 3);
}

#[test]
fn the_output_of_the_command_goes_to_stderr_and_stdout_has_one_line_per_commit() {
    let repo = Repo::new();
    let tool = common::fake_tool();
    let out = fake_tool_command(&repo, &[tool.to_str().unwrap()], &[])
        .env("FAKE_TOOL_OUT", "from-the-command\n")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(
        !stdout(&out).contains("from-the-command"),
        "{}",
        stdout(&out)
    );
    assert_eq!(stderr(&out).matches("from-the-command").count(), 3);
}

#[test]
fn the_current_worktree_and_head_are_not_touched() {
    let repo = Repo::new();
    let head = repo.git(&["rev-parse", "HEAD"]);
    std::fs::write(repo.dir().join("untracked.txt"), "dirty\n").unwrap();
    std::fs::write(repo.dir().join("README.md"), "changed\n").unwrap();
    let tool = common::fake_tool();
    let out = fake_tool_command(&repo, &[tool.to_str().unwrap()], &[])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        std::fs::read_to_string(repo.dir().join("untracked.txt")).unwrap(),
        "dirty\n"
    );
    assert!(repo.git(&["status", "--porcelain"]).contains("M README.md"));
}

#[test]
fn arguments_without_the_separator_are_a_usage_error() {
    let repo = Repo::new();
    let out = repo.run(&["main", "true"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout(&out), "");
    assert!(
        stderr(&out).contains("Usage: check-each-commit.sh"),
        "{}",
        stderr(&out)
    );
    assert_eq!(repo.run(&["main", "--"]).status.code(), Some(2));
}

#[test]
fn a_base_that_cannot_be_resolved_is_a_usage_error() {
    let repo = Repo::new();
    let out = repo.run(&["nosuchref", "--", "true"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout(&out), "");
    assert_eq!(stderr(&out), "base を解決できません: nosuchref\n");
    assert!(repo.left_in_tmp().is_empty());
}

#[test]
fn no_commit_in_the_range_is_a_usage_error() {
    let repo = Repo::empty();
    let out = repo.run(&["main", "--", "true"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stderr(&out), "main..HEAD にコミットがありません\n");
    assert!(repo.left_in_tmp().is_empty());
}

#[test]
fn a_temporary_directory_that_cannot_be_made_ends_with_3() {
    let repo = Repo::new();
    let missing = repo.root.join("missing");
    let out = repo
        .command(&["main", "--", "true"])
        .env("TMPDIR", &missing)
        .env("TMP", &missing)
        .env("TEMP", &missing)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    // 作れなかった理由（OS の文）も出す。Windows の置き場は std::env::temp_dir が決め、末尾に \ を付ける
    let reason = std::fs::create_dir(missing.join("x")).unwrap_err();
    let root = format!(
        "{}{}",
        missing.to_str().unwrap(),
        if cfg!(windows) { "\\" } else { "" }
    );
    assert_eq!(
        stderr(&out),
        format!("一時ディレクトリを作れません: {root}（{reason}）\n")
    );
    assert!(!missing.exists());
}

/// Windows では、検査のコマンドを Git Bash の sh を通して起動する（.sh を起動でき、相対のパスを worktree から引き、
/// 引数のワイルドカードを展開せず、引用符と空白を含む引数を崩さない。展開を止める MSYS の設定は、検査のコマンドに
/// 渡さない）
#[cfg(windows)]
#[test]
fn a_relative_shell_script_of_the_commit_runs_in_the_temporary_worktree_with_the_arguments_as_given()
 {
    let repo = Repo::empty();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    let log = common::sh_path(&repo.root.join("ran.log"));
    repo.commit(
        "check.sh",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$1\" \"$2\" \"$(pwd)\" \"${{MSYS-unset}}\" >> '{log}'"
        ),
        "feat: add check.sh",
    );
    // 呼んだ側の MSYS が無いときと、あるときの両方で、検査のコマンドはその MSYS を受ける
    for msys in [None, Some("winsymlinks:nativestrict")] {
        let _ = std::fs::remove_file(repo.root.join("ran.log"));
        let mut cmd = repo.command(&["main", "--", "./check.sh", "*.sh", "a \"b\" c\\"]);
        match msys {
            Some(value) => cmd.env("MSYS", value),
            None => cmd.env_remove("MSYS"),
        };
        let out = cmd.output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{msys:?}: {}", stderr(&out));
        let ran = std::fs::read_to_string(repo.root.join("ran.log")).unwrap();
        let lines: Vec<&str> = ran.lines().collect();
        assert_eq!(lines.len(), 4, "{msys:?}: {ran}");
        assert_eq!(lines[0], "*.sh", "{msys:?}: {ran}");
        assert_eq!(lines[1], "a \"b\" c\\", "{msys:?}: {ran}");
        assert!(lines[2].ends_with("/wt"), "{msys:?}: {ran}");
        assert_eq!(lines[3], msys.unwrap_or("unset"), "{ran}");
    }
}

/// Windows でも、- で始まる検査のコマンドは、Unix と同じく名前として引く（sh の exec の旗に読まない）
#[cfg(windows)]
#[test]
fn a_command_starting_with_a_dash_is_looked_up_as_a_name() {
    let repo = Repo::new();
    let out = repo.run(&["main", "--", "-c", "true"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stdout(&out).lines().all(|l| l.ends_with(" exit=127")),
        "{}",
        stdout(&out)
    );
}

#[cfg(unix)]
mod unix {
    use super::*;
    use common::{sh_path, write_executable};

    /// 検査のコマンドが、テストの作る $GO のファイルを待つ。1,000 回（少なくとも 10 秒）で諦めて 99 で終わり、壊れた
    /// ときにテストを止め続けず、待ちの失敗を通ったと見せない（`a || {WAIT_FOR_GO}` で使えるよう、1 つの組にする）
    const WAIT_FOR_GO: &str = r#"{ i=0; while [ ! -e "$GO" ] && [ "$i" -lt 1000 ]; do sleep 0.01; i=$((i + 1)); done; [ -e "$GO" ] || exit 99; }"#;

    #[test]
    fn the_command_sees_the_tree_of_each_commit() {
        let repo = Repo::new();
        let out = repo.run(&["main", "--", "test", "!", "-e", "BAD"]);
        assert_eq!(out.status.code(), Some(1));
        let lines: Vec<String> = stdout(&out).lines().map(String::from).collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].ends_with("feat: first exit=0"), "{lines:?}");
        assert!(
            lines[1].ends_with("feat: second breaks exit=1"),
            "{lines:?}"
        );
        assert!(lines[2].ends_with("fix: third repairs exit=0"), "{lines:?}");
    }

    #[test]
    fn a_command_killed_by_a_signal_gives_128_plus_the_signal_and_the_temporary_files_are_removed()
    {
        let repo = Repo::new();
        let out = repo.run(&["main", "--", "sh", "-c", "kill -9 $$"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stdout(&out).contains("feat: first exit=137"),
            "{}",
            stdout(&out)
        );
        assert!(repo.left_in_tmp().is_empty());
        assert_eq!(repo.worktrees(), 1);
    }

    #[test]
    fn a_command_that_is_not_found_gives_127() {
        let repo = Repo::new();
        let out = repo.run(&["main", "--", "no-such-command-of-check-each-commit"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stdout(&out).contains("feat: first exit=127"),
            "{}",
            stdout(&out)
        );
    }

    #[test]
    fn what_the_command_leaves_does_not_reach_the_next_commit() {
        let repo = Repo::new();
        let out = repo.run(&[
            "main",
            "--",
            "sh",
            "-c",
            "test ! -e artefact && touch artefact",
        ]);
        assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
        assert!(!stdout(&out).contains("exit=1"));
    }

    #[test]
    fn read_only_files_left_by_the_command_are_removed() {
        let repo = Repo::new();
        let out = repo.run(&[
            "main",
            "--",
            "sh",
            "-c",
            "mkdir d && touch d/f && chmod -R a-w d",
        ]);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert!(repo.left_in_tmp().is_empty(), "{:?}", repo.left_in_tmp());
        assert_eq!(repo.worktrees(), 1);
    }

    #[test]
    fn a_read_only_directory_left_by_the_previous_commit_can_be_changed_by_the_next() {
        let repo = Repo::empty();
        repo.commit("d/f", "1", "feat: add d/f");
        repo.commit("d/f", "1\n2", "feat: change d/f");
        let log = repo.root.join("ro.log");
        let script = format!("wc -l < d/f >> {}; chmod -R a-w d", sh_path(&log));
        let out = repo.run(&["main~2", "--", "sh", "-c", &script]);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert!(
            !stderr(&out).contains("unable to unlink"),
            "{}",
            stderr(&out)
        );
        let counts: Vec<String> = std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(|l| l.trim().to_string())
            .collect();
        assert_eq!(counts, ["1", "2"]);
        assert!(repo.left_in_tmp().is_empty());
    }

    #[test]
    fn the_command_does_not_inherit_stdin() {
        let repo = Repo::new();
        // 標準入力のパイプを開いたままにする。cat が継承すれば、入力の終わりを待って終わらない
        let mut child = repo
            .command(&["main", "--", "cat"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let _stdin = child.stdin.take();
        let start = std::time::Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if start.elapsed() > std::time::Duration::from_secs(20) {
                child.kill().unwrap();
                panic!("標準入力を待って終わらない");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert_eq!(status.code(), Some(0));
    }

    #[test]
    fn git_and_the_command_do_not_touch_the_repository_that_git_dir_points_to() {
        let repo = Repo::new();
        let other = Repo::empty();
        std::fs::write(other.dir().join("untracked.txt"), "keep\n").unwrap();
        std::fs::write(other.dir().join("README.md"), "edit\n").unwrap();
        let head = other.git(&["rev-parse", "HEAD"]);
        let status = other.git(&["status", "--porcelain"]);
        let out = repo
            .command(&["main", "--", "test", "-e", "a.txt"])
            .env("GIT_DIR", other.dir().join(".git"))
            .env("GIT_WORK_TREE", other.dir())
            .env("GIT_INDEX_FILE", other.dir().join(".git/index"))
            .output()
            .unwrap();
        let text = stdout(&out);
        assert!(text.contains("feat: first exit=0"), "{text}");
        assert!(text.contains("fix: third repairs exit=0"), "{text}");
        assert_eq!(other.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(other.git(&["status", "--porcelain"]), status);
        assert_eq!(
            std::fs::read_to_string(other.dir().join("untracked.txt")).unwrap(),
            "keep\n"
        );
    }

    #[test]
    fn git_variables_that_do_not_point_to_a_repository_reach_the_command() {
        let repo = Repo::new();
        let log = repo.root.join("env.log");
        let script = format!("echo \"$GIT_CONFIG_COUNT\" >> {}", sh_path(&log));
        let out = repo
            .command(&["main", "--", "sh", "-c", &script])
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "safe.directory")
            .env("GIT_CONFIG_VALUE_0", "*")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "1\n1\n1\n");
    }

    #[test]
    fn the_checkout_of_the_temporary_worktree_runs_no_hook() {
        let repo = Repo::new();
        let log = repo.root.join("hook.log");
        let hook = repo.dir().join(".git/hooks/post-checkout");
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        write_executable(&hook, format!("#!/bin/sh\necho ran >> {}\n", sh_path(&log)));
        let out = repo.run(&["main", "--", "true"]);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert!(!log.exists());
    }

    /// FAIL_GIT_SUB に指定した git のサブコマンドだけ失敗する git を置いた PATH
    fn failing_git(repo: &Repo) -> String {
        let real = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap();
        let real = String::from_utf8_lossy(&real.stdout).trim().to_string();
        let bin = repo.root.join("failbin");
        std::fs::create_dir_all(&bin).unwrap();
        write_executable(
            &bin.join("git"),
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = \"$FAIL_GIT_SUB\" ] && {{ echo 'fatal: injected' >&2; exit 128; }}; done\nexec {real} \"$@\"\n"
            ),
        );
        format!("{}:{}", sh_path(&bin), std::env::var("PATH").unwrap())
    }

    fn run_failing(repo: &Repo, sub: &str) -> Output {
        repo.command(&["main", "--", "true"])
            .env("PATH", failing_git(repo))
            .env("FAIL_GIT_SUB", sub)
            .output()
            .unwrap()
    }

    #[test]
    fn a_failing_rev_list_ends_with_3_and_says_why() {
        let repo = Repo::new();
        let out = run_failing(&repo, "rev-list");
        assert_eq!(out.status.code(), Some(3));
        assert!(
            stderr(&out).contains("git rev-list に失敗"),
            "{}",
            stderr(&out)
        );
    }

    #[test]
    fn a_failing_worktree_add_ends_with_3_and_leaves_nothing() {
        let repo = Repo::new();
        let out = run_failing(&repo, "add");
        assert_eq!(out.status.code(), Some(3));
        assert_eq!(stdout(&out), "");
        assert!(repo.left_in_tmp().is_empty());
    }

    #[test]
    fn a_failing_checkout_ends_with_3_and_leaves_nothing() {
        let repo = Repo::new();
        let out = run_failing(&repo, "checkout");
        assert_eq!(out.status.code(), Some(3));
        assert_eq!(stdout(&out), "");
        assert!(repo.left_in_tmp().is_empty());
        assert_eq!(repo.worktrees(), 1);
    }

    #[test]
    fn a_failing_clean_ends_with_3_without_checking_that_commit() {
        let repo = Repo::new();
        let out = run_failing(&repo, "clean");
        assert_eq!(out.status.code(), Some(3));
        assert_eq!(stdout(&out), "");
        assert!(
            stderr(&out).contains("git clean に失敗したので"),
            "{}",
            stderr(&out)
        );
        assert!(repo.left_in_tmp().is_empty());
    }

    #[test]
    fn an_interrupt_waits_for_the_command_removes_the_temporary_worktree_and_ends_with_130() {
        for signal in ["INT", "TERM", "HUP"] {
            let repo = Repo::new();
            let started = repo.root.join("started");
            let finished = repo.root.join("finished");
            let script = format!(
                "touch {}; sleep 1; touch {}",
                sh_path(&started),
                sh_path(&finished)
            );
            let mut child = repo
                .command(&["main", "--", "sh", "-c", &script])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            while !started.exists() {
                if let Some(status) = child.try_wait().unwrap() {
                    panic!("{signal}: 検査のコマンドを起動する前に終わった（{status}）");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let status = Command::new("kill")
                .arg(format!("-{signal}"))
                .arg(child.id().to_string())
                .status()
                .unwrap();
            assert!(status.success());
            let out = child.wait_with_output().unwrap();
            assert_eq!(out.status.code(), Some(130), "{signal}");
            // 走っている検査のコマンドが終わってから片付ける。中断されたコミットの行は出さない
            assert!(
                finished.exists(),
                "{signal}: 検査のコマンドを待たずに終わった"
            );
            assert_eq!(stdout(&out), "", "{signal}");
            assert!(
                repo.left_in_tmp().is_empty(),
                "{signal}: {:?}",
                repo.left_in_tmp()
            );
            assert_eq!(repo.worktrees(), 1, "{signal}");
        }
    }

    #[test]
    fn a_ctrl_c_that_also_ends_the_command_prints_no_line_and_checks_no_further_commit() {
        // 端末の Ctrl-C と同じく、プロセスグループ全体に INT を送る（検査のコマンドも INT で終わる）
        use std::os::unix::process::CommandExt;
        for round in 0..5 {
            let repo = Repo::new();
            let started = repo.root.join("started");
            let script = format!("echo >> {}; exec sleep 5", sh_path(&started));
            let mut child = repo
                .command(&["main", "--", "sh", "-c", &script])
                .process_group(0)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            while !started.exists() {
                if let Some(status) = child.try_wait().unwrap() {
                    panic!("{round}: 検査のコマンドを起動する前に終わった（{status}）");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // sh が sleep に替わるまで待つ
            std::thread::sleep(std::time::Duration::from_millis(100));
            let status = Command::new("kill")
                .args(["-INT", "--", &format!("-{}", child.id())])
                .status()
                .unwrap();
            assert!(status.success());
            let out = child.wait_with_output().unwrap();
            assert_eq!(out.status.code(), Some(130), "{round}");
            assert_eq!(stdout(&out), "", "{round}");
            let runs = std::fs::read_to_string(&started).unwrap();
            assert_eq!(
                runs.lines().count(),
                1,
                "{round}: 次のコミットの検査を始めた"
            );
            assert!(repo.left_in_tmp().is_empty(), "{round}");
            assert_eq!(repo.worktrees(), 1, "{round}");
        }
    }

    #[test]
    fn a_reader_that_closes_stdout_early_still_gets_the_temporary_worktree_removed() {
        // `check-each-commit.sh ... | head -1` と同じく、最初の行を読んで閉じる。2 つ目のコミットの検査は、閉じた後に
        // 終わらせる（閉じる前に残りの行がパイプに収まると、書き込みが失敗しない）
        use std::io::{BufRead, BufReader};
        let repo = Repo::new();
        let calls = sh_path(&repo.root.join("calls"));
        let go = repo.root.join("go");
        let script =
            format!("echo >> '{calls}'; [ \"$(wc -l < '{calls}')\" -eq 1 ] || {WAIT_FOR_GO}");
        let mut child = repo
            .command(&["main", "--", "sh", "-c", &script])
            .env("GO", &go)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut first = String::new();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        reader.read_line(&mut first).unwrap();
        assert!(first.ends_with("feat: first exit=0\n"), "{first}");
        drop(reader);
        std::fs::write(&go, "").unwrap();
        let status = child.wait().unwrap();
        // シェルが閉じたパイプへの書き込みで終わったと読む 128 + SIGPIPE
        assert_eq!(status.code(), Some(141), "{status}");
        assert!(repo.left_in_tmp().is_empty(), "{:?}", repo.left_in_tmp());
        assert_eq!(repo.worktrees(), 1);
    }

    #[test]
    fn a_reader_that_closes_stderr_early_still_gets_the_temporary_worktree_removed() {
        // `check-each-commit.sh ... 2>&1 | head -1` と同じく、標準エラーの最初の行を読んで閉じる。閉じた後に、
        // 検査のコマンドが見つからないという文を書かせる（2 つ目のコミットで check.sh を消す）
        use std::io::{BufRead, BufReader};
        let repo = Repo::empty();
        repo.git(&["switch", "-q", "-c", "feat/x"]);
        let go = repo.root.join("go");
        repo.commit(
            "check.sh",
            &format!("#!/bin/sh\necho started >&2; {WAIT_FOR_GO}"),
            "feat: add check.sh",
        );
        repo.git(&["update-index", "--chmod=+x", "check.sh"]);
        repo.git(&["commit", "-q", "--amend", "--no-edit"]);
        repo.git(&["rm", "-qf", "check.sh"]);
        repo.git(&["commit", "-q", "-m", "feat: remove check.sh"]);
        let mut child = repo
            .command(&["main", "--", "./check.sh"])
            .env("GO", &go)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut first = String::new();
        let mut reader = BufReader::new(child.stderr.take().unwrap());
        reader.read_line(&mut first).unwrap();
        assert_eq!(first, "started\n");
        drop(reader);
        std::fs::write(&go, "").unwrap();
        let status = child.wait().unwrap();
        // 2 つ目のコミットの検査は失敗する（check.sh が無い）
        assert_eq!(status.code(), Some(1), "{status}");
        assert!(repo.left_in_tmp().is_empty(), "{:?}", repo.left_in_tmp());
        assert_eq!(repo.worktrees(), 1);
    }
}
