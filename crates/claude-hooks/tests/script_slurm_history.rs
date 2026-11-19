//! `claude-hooks script slurm-history` が、終わったジョブの記録のファイルと偽の sacct を読んで表を出し、`--job` で
//! 偽の sacct と一時のリポジトリの reflog から走った版を出し、読めないときに 1 で終わることを確かめる（ADR-0101。
//! `~/.claude/scripts/slurm-history.sh` が呼ぶ）。表の場合分けと版の決め方は src/script/slurm_history.rs、記録の
//! 読み方は src/script/slurm.rs の単体のテストが確かめる。ここでは、引数、sacct を呼ぶ形、git の reflog と本体の
//! リポジトリの探し方を、プロセスを起動して確かめる。
//!
//! 偽の sacct は sh のスクリプトで、Slurm のサブコマンドは r995（Linux）でだけ動けばよい（ADR-0101 の決定の要因）ので、
//! Unix でだけ確かめる

#![cfg(unix)]

mod common;

use common::{forget_git_env, sh_path, write_executable};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// sacct の偽物。呼ばれた引数を sacct.log に足す。FAKE_SACCT_FAIL なら失敗する。-j の無い呼び出しには all.json を、
/// -j <番号> の --json の呼び出しには jobs/<番号>.json（無ければ空の jobs）を、--json の無い呼び出しには
/// jobs/<番号>.state（State の文字の出力）を出す
const SACCT: &str = r#"#!/bin/sh
echo "$*" >>"$T/sacct.log"
[ -z "$FAKE_SACCT_FAIL" ] || { echo 'sacct: error: Problem talking to the database' >&2; exit 1; }
json= id= prev=
for a; do
  case $a in --json=*) json=1 ;; esac
  [ "$prev" = -j ] && id=$a
  prev=$a
done
if [ -z "$id" ]; then
  cat "$T/all.json"
elif [ -n "$json" ]; then
  if [ -f "$T/jobs/$id.json" ]; then cat "$T/jobs/$id.json"; else echo '{"jobs":[]}'; fi
elif [ -f "$T/jobs/$id.state" ]; then
  cat "$T/jobs/$id.state"
fi
"#;

/// 偽の sacct と、終わったジョブの記録のファイルと、一時のリポジトリを置くディレクトリ
struct History {
    root: PathBuf,
}

impl Drop for History {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl History {
    fn new() -> History {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "fake-slurm-history-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("jobs")).unwrap();
        write_executable(&root.join("bin/sacct"), SACCT);
        std::fs::write(root.join("all.json"), r#"{"jobs":[]}"#).unwrap();
        History { root }
    }

    fn path(&self, name: &str) -> String {
        sh_path(&self.root.join(name))
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_env(args, &[])
    }

    fn run_env(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
        forget_git_env(&mut cmd)
            .args(["script", "slurm-history"])
            .args(args)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.path("bin"),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("T", &self.root)
            .env_remove("FAKE_SACCT_FAIL");
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd.output().unwrap()
    }

    /// sacct に渡した引数（1 回の起動で 1 行）
    fn sacct_log(&self) -> String {
        std::fs::read_to_string(self.root.join("sacct.log")).unwrap_or_default()
    }

    /// --job が読むジョブの記録（偽の sacct の --json と State の出力）
    fn record(&self, id: u64, state: &str, times: [i64; 3], workdir: &str, submit_line: &str) {
        let job = json!({"jobs": [{
            "job_id": id,
            "name": format!("job-{id}"),
            "time": {"elapsed": 600, "submission": times[0], "start": times[1], "end": times[2]},
            "working_directory": workdir,
            "submit_line": submit_line,
        }]});
        std::fs::write(self.root.join(format!("jobs/{id}.json")), job.to_string()).unwrap();
        std::fs::write(
            self.root.join(format!("jobs/{id}.state")),
            format!("{state}\n"),
        )
        .unwrap();
    }

    /// 本体 proj（main に 09:00 のコミット）と、09:30 に作った worktree proj-feat-x（feat/x、09:40 のコミット）。
    /// 完全な hash と、9 桁の短い hash を返す
    fn setup_repo(&self) -> (String, String) {
        let _ = std::fs::remove_dir_all(self.repo());
        let _ = std::fs::remove_dir_all(self.worktree());
        std::fs::create_dir_all(self.repo()).unwrap();
        self.git_at(None, &self.repo(), &["init", "-q", "-b", "main"]);
        std::fs::write(self.repo().join("input.toml"), "a\n").unwrap();
        self.git_at(None, &self.repo(), &["add", "input.toml"]);
        self.git_at(
            Some(at(9, 0)),
            &self.repo(),
            &["commit", "-q", "-m", "init"],
        );
        let wt = sh_path(&self.worktree());
        self.git_at(
            Some(at(9, 30)),
            &self.repo(),
            &["worktree", "add", "-q", "-b", "feat/x", &wt],
        );
        std::fs::write(self.worktree().join("input.toml"), "b\n").unwrap();
        self.git_at(
            Some(at(9, 40)),
            &self.worktree(),
            &["commit", "-q", "-am", "b"],
        );
        (
            self.git_at(None, &self.worktree(), &["rev-parse", "HEAD"]),
            self.git_at(None, &self.worktree(), &["rev-parse", "--short=9", "HEAD"]),
        )
    }

    fn repo(&self) -> PathBuf {
        self.root.join("proj")
    }

    fn worktree(&self) -> PathBuf {
        self.root.join("proj-feat-x")
    }

    /// dir で git を実行する（time があれば、その UNIX の秒を reflog の項の時刻にする）。標準出力を返す
    fn git_at(&self, time: Option<i64>, dir: &Path, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        forget_git_env(&mut cmd)
            .current_dir(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com");
        if let Some(t) = time {
            let date = format!("{t} +0900");
            cmd.env("GIT_COMMITTER_DATE", &date)
                .env("GIT_AUTHOR_DATE", &date);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
}

/// 30 日の reflog の期限に掛からない日（2 日前、JST）の 0 時の UNIX の秒
fn day() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let jst = 9 * 3600;
    (now - 2 * 86_400 + jst) / 86_400 * 86_400 - jst
}

/// 2 日前の時刻（JST の時と分）の UNIX の秒
fn at(hour: i64, minute: i64) -> i64 {
    day() + hour * 3600 + minute * 60
}

/// UNIX の秒の、JST の `%FT%T`
fn text(t: i64) -> String {
    let out = Command::new("date")
        .args(["-d", &format!("@{t}"), "+%FT%T"])
        .env("TZ", "Asia/Tokyo")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// slurm-run が投げた形の SubmitLine（資源の引数、連絡先、コマンド）
fn submit_line(resources: &str, contact: &str, command: &str) -> String {
    format!(
        "/nix/store/x-slurm/bin/sbatch {resources} --partition=main --wait --export=ALL -J j -o /logs/%x-%j.out --time 10 --comment decision | #1 | 結果で方式を決める | {contact} --wrap {command}"
    )
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// jobcomp/filetxt の 1 行（COMPLETED、見積もり 60 分、timed）
fn jobcomp(id: u64, name: &str, start: &str, end: &str) -> String {
    format!(
        "JobId={id} Name={name} JobState=COMPLETED Partition=timed TimeLimit=60 StartTime={start} EndTime={end} NodeCnt=1 ProcCnt=32 \n"
    )
}

#[test]
fn a_file_alone_is_read_without_calling_sacct() {
    let h = History::new();
    std::fs::write(
        h.root.join("jc"),
        jobcomp(
            73,
            "openmc-2A",
            "2026-10-11T13:45:32",
            "2026-10-11T14:24:05",
        ),
    )
    .unwrap();
    let out = h.run(&[&h.path("jc")]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out)
            .contains("     73  openmc-2A                   1:00:00    0:38:33    0.64        -"),
        "{}",
        stdout(&out)
    );
    assert_eq!(h.sacct_log(), "");
}

#[test]
fn sacct_is_read_as_json_with_steps_since_the_accounting_started_and_merged_with_the_file() {
    let h = History::new();
    let jobs = json!({"jobs": [{
        "job_id": 201,
        "name": "lattice",
        "partition": "timed",
        "state": {"current": ["COMPLETED"]},
        // 2026-11-01T10:00:00 から 10:30:00（JST）
        "time": {"start": 1_761_958_800, "end": 1_761_960_600, "limit": {"set": true, "infinite": false, "number": 60}},
        "submit_line": "/nix/store/x/bin/sbatch --exclusive --partition=timed --wrap run",
        "steps": [{"step": {"id": "201.batch"}, "tres": {"requested": {"max": [{"type": "mem", "count": 2_147_483_648u64}]}}}],
    }]});
    std::fs::write(h.root.join("all.json"), jobs.to_string()).unwrap();
    std::fs::write(
        h.root.join("jc"),
        format!(
            "{}{}",
            jobcomp(100, "lattice", "2026-10-05T10:00:00", "2026-10-05T10:10:00"),
            jobcomp(201, "lattice", "2026-11-01T10:00:00", "2026-11-01T10:30:00")
        ),
    )
    .unwrap();
    let out = h.run(&["--sacct", "--by-name", &h.path("jc")]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(h.sacct_log(), "--json=v0.0.45 -S 2026-10-12\n");
    assert_eq!(
        stdout(&out),
        "NAME                      RUNS   ESTIMATE    ELAPSED   RATIO MAX_RATIO  MAX_RSS\n\
         lattice                      2    1:00:00    0:20:00    0.33      0.50     2.0G\n\
         SKIPPED: 0\nUNTIMED: 0\nSHARED: 0\n"
    );
}

#[test]
fn a_failing_sacct_or_an_unreadable_output_ends_with_1() {
    let h = History::new();
    std::fs::write(h.root.join("jc"), "").unwrap();
    let out = h.run_env(&["--sacct", &h.path("jc")], &[("FAKE_SACCT_FAIL", "1")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("ERROR: sacct を読めませんでした"),
        "{}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), "");
    std::fs::write(h.root.join("all.json"), "Invalid").unwrap();
    let out = h.run(&["--sacct", &h.path("jc")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("ERROR: sacct を読めませんでした（"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn an_unreadable_line_of_the_file_ends_with_1_and_its_line_number() {
    let h = History::new();
    std::fs::write(
        h.root.join("jc"),
        format!(
            "{}JobId=2 Name=broken JobState=COMPLETED TimeLimit=10 NodeCnt=1\n",
            jobcomp(1, "ok", "2026-10-11T10:00:00", "2026-10-11T10:05:00")
        ),
    )
    .unwrap();
    let out = h.run(&[&h.path("jc")]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stderr(&out),
        "ERROR: 2 行目を読めません（StartTime がありません）\n"
    );
    assert_eq!(stdout(&out), "");
}

#[test]
fn a_missing_file_two_files_and_an_unknown_option_end_with_1() {
    let h = History::new();
    std::fs::write(h.root.join("jc"), "").unwrap();
    let jc = h.path("jc");
    for (args, error) in [
        (
            vec![h.path("none")],
            format!("ERROR: ファイルを読めません: {}", h.path("none")),
        ),
        (
            vec![jc.clone(), jc.clone()],
            format!("ERROR: ファイルは 1 つだけ渡せます: {jc} と {jc}"),
        ),
        (
            vec!["--nope".to_string(), jc.clone()],
            "ERROR: 不明なオプション: --nope".to_string(),
        ),
        (
            vec!["--by-name".to_string(), "--job".to_string()],
            "ERROR: 不明なオプション: --job".to_string(),
        ),
    ] {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = h.run(&args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(stderr(&out), format!("{error}\n"), "{args:?}");
    }
    assert_eq!(h.sacct_log(), "");
}

#[test]
fn job_reads_the_job_from_sacct_and_writes_its_lines_and_the_version_of_the_commit() {
    let h = History::new();
    let (c1, short) = h.setup_repo();
    let wt = sh_path(&h.worktree());
    h.record(
        101,
        "COMPLETED",
        [at(10, 0), at(10, 0), at(11, 0)],
        &wt,
        &submit_line(
            "--exclusive",
            &format!("feat/x {wt} commit={short} claude=s"),
            "lattice input.toml > out.txt",
        ),
    );
    let out = h.run(&["--job", "101"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        h.sacct_log(),
        "--json=v0.0.45 -X -j 101\n-j 101 -X -P -n -o State\n"
    );
    assert_eq!(
        stdout(&out),
        format!(
            "JOB: 101 job-101\n\
             STATE: COMPLETED\n\
             ELAPSED: 00:10:00\n\
             TIMES: submit={s} start={s} end={e}\n\
             EXCLUSIVE: yes\n\
             WORKDIR: {wt}\n\
             BRANCH: feat/x\n\
             COMMIT: {short}\n\
             REPO: {repo}\n\
             VERSION: {c1}\n\
             NOTE: 投げた後のコミットしない編集は確かめていない（reflog はコミットしない編集を記録せず、-dirty は投げた時点しか示さない）\n\
             COMMAND: lattice input.toml > out.txt\n",
            s = text(at(10, 0)),
            e = text(at(11, 0)),
            repo = sh_path(&h.repo()),
        )
    );
}

#[test]
fn job_writes_shared_cpus_a_cancelled_state_and_a_multiline_command_as_sacct_has_them() {
    let h = History::new();
    let (_, short) = h.setup_repo();
    let wt = sh_path(&h.worktree());
    h.record(
        102,
        "CANCELLED by 1000",
        [at(10, 0), at(10, 0), at(11, 0)],
        &wt,
        &submit_line(
            "--cpus-per-task=8",
            &format!("feat/x {wt} commit={short}"),
            "python3 -c \"\n107|x|COMPLETED|print(1)\"",
        ),
    );
    let out = stdout(&h.run(&["--job", "102"]));
    assert!(out.contains("STATE: CANCELLED by 1000\n"), "{out}");
    assert!(out.contains("EXCLUSIVE: no (--cpus-per-task=8)\n"), "{out}");
    assert!(
        out.contains("COMMAND: python3 -c \"\nCOMMAND+ 107|x|COMPLETED|print(1)\"\n"),
        "{out}"
    );
    assert_eq!(out.matches("JOB:").count(), 1);
}

#[test]
fn job_separates_jobs_with_a_blank_line_and_reports_missing_ones_after_the_others() {
    let h = History::new();
    h.record(
        101,
        "COMPLETED",
        [at(10, 0), at(10, 0), at(11, 0)],
        "/w",
        "/x/sbatch --exclusive --wrap a",
    );
    h.record(
        114,
        "COMPLETED",
        [at(10, 0), at(10, 0), at(11, 0)],
        "/w",
        "/x/sbatch --exclusive --wrap b",
    );
    let out = h.run(&["--job", "101", "999", "114"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stderr(&out),
        "ERROR: sacct にジョブ 999 が無い（番号の誤りか、sacct の記録より前のジョブ）\n"
    );
    // 前のジョブを出した後は、読めなかったジョブの前にも空行を置く
    let text = stdout(&out);
    assert_eq!(text.matches("JOB:").count(), 2);
    assert!(text.contains("COMMAND: a\n\n\nJOB: 114"), "{text}");
}

#[test]
fn job_with_a_failing_sacct_a_bad_number_or_no_number_ends_with_1() {
    let h = History::new();
    let out = h.run_env(&["--job", "101"], &[("FAKE_SACCT_FAIL", "1")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("ERROR: sacct がジョブ 101 を読めなかった\n"),
        "{}",
        stderr(&out)
    );
    for (args, error) in [
        (vec!["--job", "12a"], "ERROR: ジョブ番号の形でない: 12a"),
        (vec!["--job"], "ERROR: --job にジョブ番号を 1 つ以上渡す"),
        (
            vec!["--job", "--repo"],
            "ERROR: --repo に本体のリポジトリのパスを渡す",
        ),
        (vec!["--job", "--x", "1"], "ERROR: 不明なオプション: --x"),
    ] {
        let out = h.run(&args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(stderr(&out), format!("{error}\n"), "{args:?}");
    }
}

/// 101（commit= のある、10:00 に投げて 10:00 から 11:00 に走ったジョブ）を記録する
fn commit_job(h: &History, short: &str) {
    let wt = sh_path(&h.worktree());
    h.record(
        101,
        "COMPLETED",
        [at(10, 0), at(10, 0), at(11, 0)],
        &wt,
        &submit_line(
            "--exclusive",
            &format!("feat/x {wt} commit={short} claude=s"),
            "lattice input.toml",
        ),
    );
}

/// 110（commit= の無い、10:00 に投げて 10:05 から 11:00 に走ったジョブ）を記録する
fn branch_job(h: &History, id: u64, times: [i64; 3]) {
    let wt = sh_path(&h.worktree());
    h.record(
        id,
        "COMPLETED",
        times,
        &wt,
        &submit_line(
            "--exclusive",
            &format!("feat/x {wt} claude=s"),
            "lattice input.toml",
        ),
    );
}

#[test]
fn job_with_a_commit_reads_moves_of_the_branch_and_of_the_worktree_head_while_it_runs() {
    let h = History::new();
    let (_, short) = h.setup_repo();
    commit_job(&h, &short);
    std::fs::write(h.worktree().join("input.toml"), "c\n").unwrap();
    h.git_at(
        Some(at(10, 30)),
        &h.worktree(),
        &["commit", "-q", "-am", "c"],
    );
    let out = stdout(&h.run(&["--job", "101"]));
    assert!(out.contains("VERSION: -\n"), "{out}");
    assert!(
        out.contains(&format!(
            "REASON: 版を取った時点（{}）から End までに reflog の項がある: {} commit: c\n",
            text(at(10, 0)),
            text(at(10, 30))
        )),
        "{out}"
    );

    let (_, short) = h.setup_repo();
    commit_job(&h, &short);
    h.git_at(
        Some(at(10, 20)),
        &h.worktree(),
        &["checkout", "-q", "--detach", "HEAD~1"],
    );
    let out = stdout(&h.run(&["--job", "101"]));
    assert!(
        out.contains(&format!(
            "から End までに reflog の項がある: {} checkout:",
            text(at(10, 20))
        )),
        "{out}"
    );
}

#[test]
fn job_finds_the_repository_by_the_worktree_name_or_by_repo_after_the_worktree_is_gone() {
    let h = History::new();
    let (c1, short) = h.setup_repo();
    commit_job(&h, &short);
    let wt = sh_path(&h.worktree());
    h.git_at(None, &h.repo(), &["worktree", "remove", "--force", &wt]);
    h.git_at(None, &h.repo(), &["branch", "-q", "-D", "feat/x"]);
    let out = stdout(&h.run(&["--job", "101"]));
    assert!(
        out.contains(&format!("REPO: {}\n", sh_path(&h.repo()))),
        "{out}"
    );
    assert!(out.contains(&format!("VERSION: {c1}\n")), "{out}");
    assert!(
        out.contains(
            "NOTE: 投げた後の変化は確かめていない（ブランチと worktree の reflog が無い）\n"
        ),
        "{out}"
    );

    let moved = h.root.join("moved");
    std::fs::rename(h.repo(), &moved).unwrap();
    let out = stdout(&h.run(&["--job", "--repo", &sh_path(&moved), "101"]));
    assert!(out.contains(&format!("VERSION: {c1}\n")), "{out}");
    let out = stdout(&h.run(&["--job", "101"]));
    assert!(
        out.contains("REASON: 本体のリポジトリが分からない（WorkDir が消えた。--repo で渡す）\n"),
        "{out}"
    );
}

#[test]
fn job_with_a_commit_that_the_repository_lacks_or_a_recreated_branch_says_so() {
    let h = History::new();
    let (c1, _) = h.setup_repo();
    commit_job(&h, "0123456789");
    let out = stdout(&h.run(&["--job", "101"]));
    assert!(out.contains("REASON: commit= のコミット 0123456789 が本体のリポジトリに無い（rebase で外れ、gc で消えた）\n"), "{out}");

    let (_, short) = h.setup_repo();
    commit_job(&h, &short);
    let wt = sh_path(&h.worktree());
    h.git_at(None, &h.repo(), &["worktree", "remove", "--force", &wt]);
    h.git_at(None, &h.repo(), &["branch", "-q", "-D", "feat/x"]);
    h.git_at(Some(at(12, 0)), &h.repo(), &["branch", "feat/x", &c1]);
    let out = stdout(&h.run(&["--job", "101"]));
    assert!(out.contains(&format!("VERSION: {c1}\n")), "{out}");
    assert!(
        out.contains(
            "NOTE: 投げた後の変化は確かめていない（reflog が版を取った時点まで届かない）\n"
        ),
        "{out}"
    );
}

#[test]
fn job_with_a_commit_notes_an_unchecked_worktree_head_only_when_the_worktree_is_gone() {
    let h = History::new();
    let (_, short) = h.setup_repo();
    commit_job(&h, &short);
    let out = stdout(&h.run(&["--job", "101"]));
    assert!(!out.contains("worktree の HEAD の動き"), "{out}");
    let wt = sh_path(&h.worktree());
    h.git_at(None, &h.repo(), &["worktree", "remove", "--force", &wt]);
    let out = stdout(&h.run(&["--job", "101"]));
    assert!(out.contains("NOTE: worktree の HEAD の動き（ブランチの切り替え）は確かめていない（worktree の reflog が無いか、版を取った時点まで届かない）\n"), "{out}");
}

#[test]
fn job_without_a_commit_takes_the_branch_entry_before_the_start() {
    let h = History::new();
    let (c1, _) = h.setup_repo();
    std::fs::write(h.worktree().join("input.toml"), "c\n").unwrap();
    h.git_at(
        Some(at(11, 30)),
        &h.worktree(),
        &["commit", "-q", "-am", "c"],
    );
    branch_job(&h, 110, [at(10, 0), at(10, 5), at(11, 0)]);
    let out = stdout(&h.run(&["--job", "110"]));
    assert!(out.contains("COMMIT: -\n"), "{out}");
    assert!(
        out.contains(&format!(
            "VERSION: {c1}（reflog の feat/x@{{{}}}）\n",
            text(at(9, 40))
        )),
        "{out}"
    );
    assert!(out.contains("NOTE: コミットしていない変更の有無は確かめていない（nix を通らないジョブには dirty の印が出ない）\n"), "{out}");

    // 11:00 から 12:00 に走ったジョブは、11:30 のコミットで版を決められない
    branch_job(&h, 111, [at(11, 0), at(11, 0), at(12, 0)]);
    let out = stdout(&h.run(&["--job", "111"]));
    assert!(
        out.contains(&format!(
            "から End までに reflog の項がある: {} commit: c\n",
            text(at(11, 30))
        )),
        "{out}"
    );

    branch_job(&h, 112, [at(8, 0), at(8, 0), at(8, 30)]);
    let out = stdout(&h.run(&["--job", "112"]));
    assert!(
        out.contains(&format!(
            "REASON: reflog が Start まで届かない（最も古い項は {}）\n",
            text(at(9, 30))
        )),
        "{out}"
    );

    let wt = sh_path(&h.worktree());
    h.git_at(None, &h.repo(), &["worktree", "remove", "--force", &wt]);
    let out = stdout(&h.run(&["--job", "110"]));
    assert!(out.contains(&format!("VERSION: {c1}（")), "{out}");
    assert!(out.contains("NOTE: worktree の HEAD の動き"), "{out}");
    h.git_at(None, &h.repo(), &["branch", "-q", "-D", "feat/x"]);
    let out = stdout(&h.run(&["--job", "110"]));
    assert!(
        out.contains("REASON: ブランチ feat/x か、その reflog が無い（消えた）\n"),
        "{out}"
    );
}

#[test]
fn job_without_a_commit_reads_a_worktree_head_that_moved_before_the_start() {
    let h = History::new();
    h.setup_repo();
    h.git_at(
        Some(at(10, 30)),
        &h.worktree(),
        &["checkout", "-q", "--detach", "main"],
    );
    branch_job(&h, 116, [at(10, 0), at(11, 0), at(11, 30)]);
    let out = stdout(&h.run(&["--job", "116"]));
    assert!(
        out.contains(&format!(
            "REASON: Start の時点の worktree の HEAD がブランチの版と違う（{} checkout: moving from feat/x to main）\n",
            text(at(10, 30))
        )),
        "{out}"
    );
}
