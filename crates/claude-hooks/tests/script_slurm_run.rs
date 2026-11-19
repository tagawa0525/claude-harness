//! `claude-hooks script slurm-run` が、sbatch、squeue、sacct、scontrol、git を子のプロセスで呼び、投げる、並べ直す、待つ、
//! 結果を出す流れを sh の版と同じ行で出すことを確かめる（ADR-0101。`~/.claude/scripts/slurm-run.sh` が呼ぶ）。
//! 引数の確かめ方、TIME: の印、Slurm の JSON の読み方、並べ直しと待ちの回数の場合分けは src/script/slurm_run.rs の単体の
//! テストが確かめる。
//!
//! 偽の Slurm のコマンドは sh のスクリプトで、ログのファイルを書き、呼ばれた回数で応答を変える（examples/fake_tool.rs では
//! 書けない）。Slurm のサブコマンドは r995（Linux）でだけ動けばよい（ADR-0101 の決定の要因）ので、Unix でだけ確かめる

#![cfg(unix)]

mod common;

use common::{forget_git_env, sh_path, squeue_json, write_executable};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// sbatch の偽物。引数を sbatch.args に 1 行ずつ書き、-o の %x と %j を埋めたログに書き、FAKE_JOB_EXIT で終わる
/// （sbatch --wait はジョブの終了コードで終わる）。FAKE_SBATCH_PID があれば、番号を出した後に自分の番号を書いて待ち続ける
const SBATCH: &str = r#"#!/bin/sh
printf '%s\n' "$@" >"$T/sbatch.args"
rm -f "$T/squeue.count" "$T/sacct.count"
name= log=
while [ $# -gt 0 ]; do
  case "$1" in
    -J) name=$2; shift ;;
    -o) log=$2; shift ;;
  esac
  shift
done
echo "Submitted batch job 42"
log=$(printf '%s' "$log" | sed "s/%x/$name/; s/%j/42/")
echo "job output" >"$log"
[ -z "$FAKE_LOG_READONLY" ] || chmod 444 "$log"
[ -z "$FAKE_SBATCH_SIGIGN" ] || grep SigIgn /proc/$$/status >"$FAKE_SBATCH_SIGIGN"
if [ -n "$FAKE_SBATCH_PID" ]; then
  echo $$ >"$FAKE_SBATCH_PID"
  exec sleep 30
fi
exit "${FAKE_JOB_EXIT:-0}"
"#;

/// squeue の偽物。走っている main のジョブの問い合わせ（--timed の投げる前の確認）には FAKE_SQUEUE_RUNNING（JSON。
/// 無ければ空の jobs）を出し、FAKE_SQUEUE_RUNNING_ERR を標準エラーに出す。ほかは並べ直しの読み取りで、投げてから n 回目に
/// FAKE_SQUEUE_OUT_<n> を出し（無ければ空の jobs。並べ直しが済んだ状態）、FAKE_SQUEUE_EXIT で終わる
const SQUEUE: &str = r#"#!/bin/sh
case " $* " in
  *" --states=RUNNING,SUSPENDED,CONFIGURING "*)
    echo "$*" >"$T/squeue.args.running"
    [ -z "$FAKE_SQUEUE_RUNNING_ERR" ] || echo "$FAKE_SQUEUE_RUNNING_ERR" >&2
    if [ -n "$FAKE_SQUEUE_RUNNING" ]; then printf '%s\n' "$FAKE_SQUEUE_RUNNING"; else echo '{"jobs":[]}'; fi
    exit "${FAKE_SQUEUE_RUNNING_EXIT:-0}"
    ;;
esac
n=$(($(cat "$T/squeue.count" 2>/dev/null || echo 0) + 1))
echo "$n" >"$T/squeue.count"
eval "out=\${FAKE_SQUEUE_OUT_$n-}"
[ -n "$out" ] || out='{"jobs":[]}'
printf '%s\n' "$out"
exit "${FAKE_SQUEUE_EXIT:-0}"
"#;

/// sacct の偽物。終わったジョブの記録を JSON で返す。FAKE_SACCT_EMPTY: 最初のその回数は、記録がまだ届いていない
/// （空の jobs）。FAKE_SACCT_RUNNING: 続くその回数は RUNNING の記録を返す。パーティションは sbatch に渡したもの
const SACCT: &str = r#"#!/bin/sh
echo "$*" >"$T/sacct.args"
n=$(($(cat "$T/sacct.count" 2>/dev/null || echo 0) + 1))
echo "$n" >"$T/sacct.count"
[ -z "$FAKE_SACCT_EXIT" ] || exit "$FAKE_SACCT_EXIT"
if [ "$n" -le "${FAKE_SACCT_EMPTY:-0}" ]; then
  echo '{"jobs":[]}'
  exit 0
fi
part=main
grep -qx -- '--partition=timed' "$T/sbatch.args" && part=timed
state=${FAKE_STATE:-COMPLETED}
[ "$n" -gt $((${FAKE_SACCT_EMPTY:-0} + ${FAKE_SACCT_RUNNING:-0})) ] || state=RUNNING
printf '{"jobs":[{"job_id":42,"partition":"%s","state":{"current":["%s"]},"time":{"elapsed":754,"limit":{"set":true,"infinite":false,"number":30},"suspended":%s}}]}\n' \
  "$part" "$state" "${FAKE_SACCT_SUSPENDED:-0}"
"#;

/// scontrol の偽物。show job は終わったジョブの State を JSON で返す（FAKE_SHOW_EXIT で失敗させられる）。top は
/// scontrol.calls に引数を足し、FAKE_TOP_EXIT で終わる
const SCONTROL: &str = r#"#!/bin/sh
if [ "$1" = show ]; then
  echo "$*" >"$T/scontrol.show.args"
  printf '{"jobs":[{"job_id":42,"job_state":["%s"]}]}\n' "${FAKE_SCONTROL_STATE:-COMPLETED}"
  exit "${FAKE_SHOW_EXIT:-0}"
fi
echo "$*" >>"$T/scontrol.calls"
exit "${FAKE_TOP_EXIT:-0}"
"#;

const HOSTNAME: &str = r#"#!/bin/sh
echo "${FAKE_HOST:-r995}"
"#;

/// 待っているジョブ（番号、パーティション、優先度、区分、連絡先）の待ち行列
fn queue(jobs: &[(u64, &str, u64, &str, &str)]) -> String {
    let comments: Vec<String> = jobs
        .iter()
        .map(|(_, _, _, class, contact)| format!("{class} | #1 | p | {contact}"))
        .collect();
    let jobs: Vec<_> = jobs
        .iter()
        .zip(&comments)
        .map(|((id, partition, priority, _, _), comment)| {
            (*id, "j", *partition, *priority, comment.as_str())
        })
        .collect();
    squeue_json(&jobs)
}

/// 偽の Slurm と HOME を置いた一時のディレクトリ
struct Slurm {
    root: PathBuf,
}

impl Drop for Slurm {
    fn drop(&mut self) {
        let _ = Command::new("chmod")
            .arg("-R")
            .arg("u+w")
            .arg(&self.root)
            .status();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Slurm {
    fn new() -> Slurm {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        // CARGO_TARGET_TMPDIR はこのリポジトリの中にあり、git の外の場合を作れないので、OS の一時の場所に置く
        let root = std::env::temp_dir().join(format!(
            "fake-slurm-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["bin", "home/github/proj", "tmp"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for (name, body) in [
            ("sbatch", SBATCH),
            ("squeue", SQUEUE),
            ("sacct", SACCT),
            ("scontrol", SCONTROL),
            ("hostname", HOSTNAME),
        ] {
            write_executable(&root.join("bin").join(name), body);
        }
        std::fs::write(
            root.join("home/.gitconfig"),
            "[user]\n\tname = test\n\temail = test@example.com\n[init]\n\tdefaultBranch = main\n[advice]\n\tdetachedHead = false\n",
        )
        .unwrap();
        Slurm { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn proj(&self) -> PathBuf {
        self.home().join("github/proj")
    }

    /// 起動の形（cwd、環境）。PATH は偽のコマンドの置き場を先にし、git と sh のツールは手元のものを使う
    fn command(&self, cwd: &Path, args: &[&str], env: &[(&str, &str)]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
        cmd.args(["script", "slurm-run"]).args(args);
        self.prepare(&mut cmd, cwd, env);
        cmd
    }

    /// cmd の cwd と環境を、偽の Slurm と HOME を使う形にする
    fn prepare(&self, cmd: &mut Command, cwd: &Path, env: &[(&str, &str)]) {
        forget_git_env(cmd);
        cmd.current_dir(cwd)
            .env("PWD", cwd)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    sh_path(&self.root.join("bin")),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("T", &self.root)
            .env("HOME", self.home())
            .env("GIT_CONFIG_GLOBAL", self.home().join(".gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("TMPDIR", self.root.join("tmp"))
            .env("SLURM_RUN_SACCT_INTERVAL", "0")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("SLURM_RUN_NFS_ENTRY")
            .env_remove("SBATCH_PARTITION");
        for (key, value) in env {
            cmd.env(key, value);
        }
    }

    fn run_in(&self, cwd: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
        self.command(cwd, args, env).output().unwrap()
    }

    /// proj で、区分 trial、参照 #1、目的「試験の目的」を付けて投げる
    fn run(&self, rest: &[&str], env: &[(&str, &str)]) -> Output {
        self.run_in(&self.proj(), &with_purpose(rest), env)
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.root.join(name)).unwrap_or_default()
    }

    fn submitted(&self) -> bool {
        self.root.join("sbatch.args").exists()
    }
}

fn with_purpose<'a>(rest: &[&'a str]) -> Vec<&'a str> {
    let mut all = vec!["--class", "trial", "--ref", "#1", "--purpose", "試験の目的"];
    all.extend_from_slice(rest);
    all
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn git(dir: &Path, args: &[&str], slurm: &Slurm) {
    let mut cmd = Command::new("git");
    forget_git_env(&mut cmd);
    let status = cmd
        .args(args)
        .current_dir(dir)
        .env("HOME", slurm.home())
        .env("GIT_CONFIG_GLOBAL", slurm.home().join(".gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// コミットの 1 つあるリポジトリ（ブランチ feat/commit）と、その短い hash
fn committed(slurm: &Slurm) -> (PathBuf, String) {
    let dir = slurm.home().join("github/committed");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "feat/commit", "."], slurm);
    std::fs::write(dir.join("input.toml"), "a\n").unwrap();
    git(&dir, &["add", "input.toml"], slurm);
    git(&dir, &["commit", "-q", "-m", "feat: seed"], slurm);
    let mut cmd = Command::new("git");
    forget_git_env(&mut cmd);
    let out = cmd
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(&dir)
        .output()
        .unwrap();
    (dir, String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[test]
fn a_completed_job_prints_the_job_log_next_tail_time_result_and_exit_lines_in_order() {
    let slurm = Slurm::new();
    git(
        &slurm.proj(),
        &["init", "-q", "-b", "feat/vera-2g", "."],
        &slurm,
    );
    let out = slurm.run(&["-t", "30", "xlc-2G", "cargo run --release"], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let proj = sh_path(&slurm.proj());
    let log = format!("{}/github/slurm-logs/xlc-2G-42.out", sh_path(&slurm.home()));
    let time = "TIME: job=42 elapsed=00:12:34 estimate=00:30:00 preemptible: 割り込まれうるので時間は記録に使わない（記録するなら排他の --timed） state=COMPLETED class=trial ref=#1 purpose=試験の目的";
    assert_eq!(
        stdout(&out),
        format!(
            "JOB: 42\nLOG: {log}\nNEXT: ジョブは {proj} で走る。意図した worktree でなければ scancel 42 で取り消し、cd <worktree> && で投げ直す\njob output\n{time}\nNEXT: TIME 行に印があるので、この時間（elapsed も、プログラムが自分で測った時間も）は記録に使わない。時間を記録する計算なら、-c を付けずに区分 timed で投げ直して測る（時間を記録しない計算は投げ直さない）\nRESULT: job=42 name=xlc-2G exit=0 state=COMPLETED\nEXIT: 0\n"
        )
    );
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        format!("job output\n{time}\n")
    );
    let args = slurm.read("sbatch.args");
    assert!(
        args.contains(&format!(
            "--comment\ntrial | #1 | 試験の目的 | feat/vera-2g {proj}\n"
        )),
        "{args}"
    );
    assert!(args.contains("-o\n") && args.contains("/github/slurm-logs/%x-%j.out\n"));
    assert_eq!(slurm.read("sacct.args"), "--json=v0.0.45 -X -j 42\n");
}

#[test]
fn a_failed_job_ends_with_its_exit_code_and_the_state() {
    let slurm = Slurm::new();
    let out = slurm.run(
        &["-t", "30", "xlc-2G", "false"],
        &[("FAKE_JOB_EXIT", "3"), ("FAKE_STATE", "FAILED")],
    );
    assert_eq!(out.status.code(), Some(3));
    let text = stdout(&out);
    assert!(
        text.contains("TIME: job=42 elapsed=00:12:34 estimate=00:30:00"),
        "{text}"
    );
    assert!(
        text.ends_with("RESULT: job=42 name=xlc-2G exit=3 state=FAILED\nEXIT: 3 (State=FAILED)\n"),
        "{text}"
    );
}

#[test]
fn a_cancelled_job_after_sbatch_returned_zero_ends_with_one() {
    let slurm = Slurm::new();
    let out = slurm.run(
        &["-t", "30", "xlc-2G", "sleep 999"],
        &[("FAKE_STATE", "CANCELLED")],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).ends_with("state=CANCELLED\nEXIT: 1 (State=CANCELLED)\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn the_record_is_awaited_until_sacct_returns_a_final_state() {
    let slurm = Slurm::new();
    let out = slurm.run(
        &["-t", "30", "xlc-2G", "true"],
        &[("FAKE_SACCT_EMPTY", "2"), ("FAKE_SACCT_RUNNING", "1")],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("TIME: job=42 elapsed=00:12:34"));
    assert_eq!(slurm.read("sacct.count"), "4\n");
}

#[test]
fn without_a_record_the_state_comes_from_scontrol_and_no_time_line_is_written() {
    let slurm = Slurm::new();
    let completed = slurm.run(
        &["-t", "30", "xlc-2G", "true"],
        &[("SLURM_RUN_SACCT_TRIES", "3"), ("FAKE_SACCT_EMPTY", "99")],
    );
    assert_eq!(completed.status.code(), Some(0));
    assert!(
        stderr(&completed).contains(
            "WARN: ジョブ 42 の記録が sacct から読めなかった（3 回。実際の時間は確かめられない）"
        ),
        "{}",
        stderr(&completed)
    );
    assert!(!stdout(&completed).contains("TIME:"));
    assert_eq!(slurm.read("sacct.count"), "3\n");
    assert_eq!(
        slurm.read("scontrol.show.args"),
        "show job 42 --json=v0.0.45\n"
    );

    let cancelled = slurm.run(
        &["-t", "30", "xlc-2G", "sleep 999"],
        &[
            ("SLURM_RUN_SACCT_TRIES", "2"),
            ("FAKE_SACCT_EXIT", "1"),
            ("FAKE_SCONTROL_STATE", "CANCELLED"),
        ],
    );
    assert_eq!(cancelled.status.code(), Some(1));
    assert!(stdout(&cancelled).ends_with("EXIT: 1 (State=CANCELLED)\n"));

    for env in [
        &[("FAKE_SHOW_EXIT", "1")][..],
        &[("FAKE_SCONTROL_STATE", "RUNNING")],
    ] {
        let mut all = vec![("SLURM_RUN_SACCT_TRIES", "2"), ("FAKE_SACCT_EXIT", "1")];
        all.extend_from_slice(env);
        let unknown = slurm.run(&["-t", "30", "xlc-2G", "true"], &all);
        assert_eq!(unknown.status.code(), Some(1), "{env:?}");
        assert!(stderr(&unknown).contains("sacct からも scontrol からも確かめられなかった"));
        assert!(stdout(&unknown).ends_with(
            "RESULT: job=42 name=xlc-2G exit=1 state=unknown\nEXIT: 1 (State=unknown)\n"
        ));
    }
    let failed = slurm.run(
        &["-t", "30", "xlc-2G", "false"],
        &[
            ("SLURM_RUN_SACCT_TRIES", "2"),
            ("FAKE_SACCT_EXIT", "1"),
            ("FAKE_SHOW_EXIT", "1"),
            ("FAKE_JOB_EXIT", "3"),
        ],
    );
    assert_eq!(failed.status.code(), Some(3));
}

#[test]
fn a_log_that_cannot_be_appended_to_gets_a_warning_and_the_time_line_on_stdout() {
    let slurm = Slurm::new();
    let out = slurm.run(
        &["-t", "30", "xlc-ro", "false"],
        &[("FAKE_LOG_READONLY", "1"), ("FAKE_JOB_EXIT", "3")],
    );
    assert_eq!(out.status.code(), Some(3));
    assert!(
        stderr(&out).contains("WARN: ジョブ 42 の実際の時間をログに書き足せなかった"),
        "{}",
        stderr(&out)
    );
    let text = stdout(&out);
    assert!(
        text.contains("TIME: job=42 elapsed=00:12:34 estimate=00:30:00"),
        "{text}"
    );
    assert!(text.ends_with("EXIT: 3\n"), "{text}");
}

#[test]
fn a_main_job_reorders_the_queue_by_class_and_reads_it_again_after_each_top() {
    let slurm = Slurm::new();
    let q = "feat/x /p";
    let first = queue(&[
        (20, "main", 10010, "trial", q),
        (42, "main", 10000, "record", q),
    ]);
    // 42,20 を渡した後、同時に投げた 43 の列が先に通っていて、43 が trial の後ろに残った状態
    let second = queue(&[
        (42, "main", 10010, "record", q),
        (20, "main", 10009, "trial", q),
        (43, "main", 10000, "record", q),
    ]);
    let out = slurm.run_in(
        &slurm.proj(),
        &[
            "--class",
            "record",
            "--ref",
            "#1",
            "--purpose",
            "本番",
            "-t",
            "5",
            "xlc-rec",
            "true",
        ],
        &[
            ("FAKE_SQUEUE_OUT_1", &first),
            ("FAKE_SQUEUE_OUT_2", &second),
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(slurm.read("scontrol.calls"), "top 42,20\ntop 42,43,20\n");
    assert!(!stderr(&out).contains("WARN:"), "{}", stderr(&out));
}

#[test]
fn top_puts_the_main_job_first_in_its_class_and_an_unreadable_queue_only_warns() {
    let slurm = Slurm::new();
    let q = "feat/x /p";
    let queue = queue(&[
        (21, "main", 10030, "record", q),
        (20, "main", 10020, "trial", q),
        (42, "main", 10000, "trial", q),
    ]);
    slurm.run(
        &["--top", "-t", "5", "xlc-0005-1B", "true"],
        &[("FAKE_SQUEUE_OUT_1", &queue)],
    );
    assert_eq!(slurm.read("scontrol.calls"), "top 21,42,20\n");

    let _ = std::fs::remove_file(slurm.root.join("scontrol.calls"));
    let out = slurm.run(
        &["--top", "-t", "5", "xlc-0005-1B", "true"],
        &[("FAKE_SQUEUE_EXIT", "2"), ("FAKE_JOB_EXIT", "3")],
    );
    assert_eq!(out.status.code(), Some(3));
    assert!(
        stderr(&out).contains("ERROR: squeue が失敗した（終了コード 2）\nWARN: 待ち行列を読めず"),
        "{}",
        stderr(&out)
    );
    assert_eq!(slurm.read("scontrol.calls"), "");
}

#[test]
fn a_timed_job_is_not_reordered_by_class_and_top_moves_only_itself() {
    let slurm = Slurm::new();
    let queue = queue(&[
        (21, "timed", 10030, "timed", "c"),
        (42, "timed", 10000, "decision", "c"),
    ]);
    let args = [
        "--class",
        "decision",
        "--ref",
        "#1",
        "--purpose",
        "急ぐ確認",
        "--timed",
        "-t",
        "5",
        "xlc-quick",
        "true",
    ];
    slurm.run_in(&slurm.proj(), &args, &[("FAKE_SQUEUE_OUT_1", &queue)]);
    assert_eq!(slurm.read("scontrol.calls"), "");
    assert!(slurm.read("sbatch.args").contains("--partition=timed\n"));
    let mut top = args.to_vec();
    top.insert(7, "--top");
    let out = slurm.run_in(&slurm.proj(), &top, &[("FAKE_TOP_EXIT", "1")]);
    assert_eq!(slurm.read("scontrol.calls"), "top 42\n");
    assert!(
        stderr(&out).contains("WARN: ジョブ 42 を待ち行列の先頭に移せなかった。投げた順に走る")
    );
}

#[test]
fn the_contact_has_the_branch_place_commit_dirty_mark_and_session() {
    let slurm = Slurm::new();
    let (dir, head) = committed(&slurm);
    let place = sh_path(&dir);
    let comment = |extra: &[(&str, &str)]| {
        slurm.run_in(&dir, &with_purpose(&["-t", "1", "xlc-2G", "true"]), extra);
        let args = slurm.read("sbatch.args");
        args.lines()
            .skip_while(|l| *l != "--comment")
            .nth(1)
            .unwrap()
            .to_string()
    };
    assert_eq!(
        comment(&[("CLAUDE_CODE_SESSION_ID", "abc-123")]),
        format!("trial | #1 | 試験の目的 | feat/commit {place} commit={head} claude=abc-123")
    );
    // git が無視するファイルだけなら印を付けない
    std::fs::write(dir.join(".git/info/exclude"), "trials/\n").unwrap();
    std::fs::create_dir_all(dir.join("trials")).unwrap();
    std::fs::write(dir.join("trials/run.log"), "out\n").unwrap();
    assert!(comment(&[]).ends_with(&format!("commit={head}")));
    // 追加していない新しいファイル（status.showUntrackedFiles が no でも）
    git(&dir, &["config", "status.showUntrackedFiles", "no"], &slurm);
    std::fs::write(dir.join("new_source.rs"), "fn main() {}\n").unwrap();
    assert!(comment(&[]).ends_with(&format!("commit={head}-dirty")));
    std::fs::remove_file(dir.join("new_source.rs")).unwrap();
    git(
        &dir,
        &["config", "--unset", "status.showUntrackedFiles"],
        &slurm,
    );
    // 追跡しているファイルの変更と、ステージしただけの変更
    std::fs::write(dir.join("input.toml"), "b\n").unwrap();
    assert!(comment(&[]).ends_with(&format!("commit={head}-dirty")));
    git(&dir, &["add", "input.toml"], &slurm);
    assert!(comment(&[]).ends_with(&format!("commit={head}-dirty")));
    git(
        &dir,
        &["checkout", "-q", "HEAD", "--", "input.toml"],
        &slurm,
    );
    // HEAD を切り離しても、投げた版のコミットを足す
    git(&dir, &["checkout", "-q", "--detach"], &slurm);
    assert_eq!(
        comment(&[]),
        format!("trial | #1 | 試験の目的 | {place} commit={head}")
    );
}

#[test]
fn a_new_submodule_commit_is_dirty_even_when_diff_ignores_submodules() {
    let slurm = Slurm::new();
    let (dir, _) = committed(&slurm);
    let sub = slurm.root.join("subsrc");
    git(
        &slurm.root,
        &["init", "-q", "-b", "main", &sh_path(&sub)],
        &slurm,
    );
    git(
        &sub,
        &["commit", "-q", "--allow-empty", "-m", "init"],
        &slurm,
    );
    git(
        &dir,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            &sh_path(&sub),
            "sub",
        ],
        &slurm,
    );
    git(&dir, &["commit", "-q", "-m", "feat: add sub"], &slurm);
    git(&dir, &["config", "diff.ignoreSubmodules", "all"], &slurm);
    git(
        &dir.join("sub"),
        &["commit", "-q", "--allow-empty", "-m", "next"],
        &slurm,
    );
    slurm.run_in(&dir, &with_purpose(&["-t", "1", "xlc-2G", "true"]), &[]);
    assert!(
        slurm.read("sbatch.args").contains("-dirty\n"),
        "{}",
        slurm.read("sbatch.args")
    );
}

#[test]
fn outside_git_and_without_commits_no_commit_is_added_and_broken_git_warns() {
    let slurm = Slurm::new();
    let plain = slurm.home().join("github/plain");
    std::fs::create_dir_all(&plain).unwrap();
    let out = slurm.run_in(&plain, &with_purpose(&["-t", "1", "xlc-2G", "true"]), &[]);
    assert!(!stderr(&out).contains("WARN: commit="), "{}", stderr(&out));
    assert!(slurm.read("sbatch.args").contains(&format!(
        "--comment\ntrial | #1 | 試験の目的 | {}\n",
        sh_path(&plain)
    )));
    git(
        &slurm.proj(),
        &["init", "-q", "-b", "feat/empty", "."],
        &slurm,
    );
    let out = slurm.run(&["-t", "1", "xlc-2G", "true"], &[]);
    assert!(!slurm.read("sbatch.args").contains("commit="));
    assert!(!stderr(&out).contains("WARN:"), "{}", stderr(&out));

    let (dir, _) = committed(&slurm);
    std::fs::write(dir.join(".git/index"), "garbage\n").unwrap();
    let out = slurm.run_in(&dir, &with_purpose(&["-t", "1", "xlc-2G", "true"]), &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stderr(&out).contains("WARN: commit= を付けません（git status が失敗）"),
        "{}",
        stderr(&out)
    );
    assert!(!slurm.read("sbatch.args").contains("commit="));

    let bare = slurm.home().join("github/bare.git");
    git(
        &slurm.root,
        &["init", "-q", "--bare", &sh_path(&bare)],
        &slurm,
    );
    let out = slurm.run_in(&bare, &with_purpose(&["-t", "1", "xlc-2G", "true"]), &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stderr(&out)
            .contains("WARN: commit= を付けません（作業ツリーの外。bare リポジトリか .git の中）"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn off_r995_jobs_are_submitted_under_the_nfs_entry_with_logs_there() {
    let slurm = Slurm::new();
    let nfs = slurm.root.join("mnt/r995/github");
    std::fs::create_dir_all(nfs.join("proj")).unwrap();
    let nfs_text = sh_path(&nfs);
    let env = [
        ("FAKE_HOST", "t14g4"),
        ("SLURM_RUN_NFS_ENTRY", nfs_text.as_str()),
    ];
    let out = slurm.run_in(
        &nfs.join("proj"),
        &with_purpose(&["-t", "1", "xlc-2G", "true"]),
        &env,
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains(&format!("LOG: {nfs_text}/slurm-logs/xlc-2G-42.out\n")));
    assert!(slurm.read("sbatch.args").contains(&format!(
        "-o\n{}/github/slurm-logs/%x-%j.out\n",
        sh_path(&slurm.home())
    )));

    let _ = std::fs::remove_file(slurm.root.join("sbatch.args"));
    let out = slurm.run(&["-t", "1", "xlc-2G", "true"], &[("FAKE_HOST", "t14g4")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("ERROR: r995 以外では /mnt/r995/github の下で投げる"),
        "{}",
        stderr(&out)
    );
    assert!(stdout(&out).contains("RESULT: job=- name=xlc-2G exit=1 state=NOT_SUBMITTED"));
    assert!(!slurm.submitted());
}

#[test]
fn a_timed_job_is_refused_while_a_main_job_runs_in_the_same_worktree() {
    let slurm = Slurm::new();
    let (dir, _) = committed(&slurm);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let running = |id: &str, path: &Path| {
        format!(
            r#"{{"jobs":[{{"job_id":{id},"current_working_directory":"{}"}}]}}"#,
            sh_path(path)
        )
    };
    for (cwd, path) in [
        (dir.clone(), dir.clone()),
        (dir.clone(), dir.join("sub")),
        (dir.join("sub"), dir.clone()),
    ] {
        let _ = std::fs::remove_file(slurm.root.join("sbatch.args"));
        let json = running("700", &path);
        let out = slurm.run_in(
            &cwd,
            &with_purpose(&["--timed", "-t", "5", "xlc-bench", "true"]),
            &[("FAKE_SQUEUE_RUNNING", &json)],
        );
        assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
        assert!(stderr(&out).contains("ERROR: 走っている main のジョブ 700 が同じ worktree"));
        assert!(stderr(&out).contains(
            "NEXT: 別の worktree（git worktree add）から投げるか、ジョブ 700 が終わってから投げる"
        ));
        assert!(!slurm.submitted());
    }
    assert_eq!(
        slurm.read("squeue.args.running"),
        "--json=v0.0.45 --partition=main --states=RUNNING,SUSPENDED,CONFIGURING\n"
    );
    // 別の worktree なら投げる。timed でないジョブは確かめない
    let other = running("701", &slurm.proj());
    let out = slurm.run_in(
        &dir,
        &with_purpose(&["--timed", "-t", "5", "xlc-bench", "true"]),
        &[("FAKE_SQUEUE_RUNNING", &other)],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let same = running("700", &dir);
    let out = slurm.run_in(
        &dir,
        &with_purpose(&["-t", "5", "xlc-2G", "true"]),
        &[("FAKE_SQUEUE_RUNNING", &same)],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
}

#[test]
fn the_same_worktree_is_matched_across_the_r995_home_and_the_nfs_entry() {
    let slurm = Slurm::new();
    let nfs = slurm.root.join("mnt/r995/github");
    std::fs::create_dir_all(nfs.join("proj")).unwrap();
    let nfs_text = sh_path(&nfs);
    let json = |path: &Path| {
        format!(
            r#"{{"jobs":[{{"job_id":702,"current_working_directory":"{}"}}]}}"#,
            sh_path(path)
        )
    };
    // r995 以外から NFS の入口の下で投げ、走っているジョブは r995 の上のパスで投げられている
    let from_nfs = json(&slurm.proj());
    let out = slurm.run_in(
        &nfs.join("proj"),
        &with_purpose(&["--timed", "-t", "5", "xlc-bench", "true"]),
        &[
            ("FAKE_HOST", "t14g4"),
            ("SLURM_RUN_NFS_ENTRY", &nfs_text),
            ("FAKE_SQUEUE_RUNNING", &from_nfs),
        ],
    );
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    // r995 から投げ、走っているジョブは NFS の入口の下から投げられている
    let from_home = json(&nfs.join("proj"));
    let out = slurm.run(
        &["--timed", "-t", "5", "xlc-bench", "true"],
        &[
            ("SLURM_RUN_NFS_ENTRY", &nfs_text),
            ("FAKE_SQUEUE_RUNNING", &from_home),
        ],
    );
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(!slurm.submitted());
}

#[test]
fn an_unreadable_list_of_running_jobs_warns_with_the_reason_and_submits() {
    let slurm = Slurm::new();
    let out = slurm.run(
        &["--timed", "-t", "5", "xlc-bench", "true"],
        &[
            ("FAKE_SQUEUE_RUNNING_EXIT", "1"),
            (
                "FAKE_SQUEUE_RUNNING_ERR",
                "slurm_load_jobs error: Unable to contact slurm controller",
            ),
        ],
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stderr(&out).contains("WARN: 走っている main のジョブを読めず、同じ worktree で走っているかを確かめられなかった（squeue が失敗）: slurm_load_jobs error: Unable to contact slurm controller"),
        "{}",
        stderr(&out)
    );
    assert!(slurm.submitted());
}

#[test]
fn refusals_print_one_not_submitted_result_and_a_submitted_job_prints_one_result() {
    let slurm = Slurm::new();
    let out = slurm.run(&["-t", "25m", "xlc-early", "true"], &[]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stdout(&out),
        "RESULT: job=- name=xlc-early exit=1 state=NOT_SUBMITTED\n"
    );
    assert!(stderr(&out).starts_with("ERROR: -t は分の数"));
    let out = slurm.run(&["-t", "5"], &[]);
    assert_eq!(
        stdout(&out),
        "RESULT: job=- name=- exit=1 state=NOT_SUBMITTED\n"
    );
    assert!(!slurm.submitted());
    let out = slurm.run(&["-t", "5", "xlc-2G", "true"], &[]);
    assert_eq!(stdout(&out).matches("RESULT:").count(), 1);
}

#[test]
fn a_failed_submission_shows_sbatch_output_and_a_not_submitted_result() {
    let slurm = Slurm::new();
    write_executable(
        &slurm.root.join("bin/sbatch"),
        "#!/bin/sh\necho 'sbatch: error: Batch job submission failed'\nexit 1\n",
    );
    let out = slurm.run(&["-t", "5", "xlc-2G", "true"], &[]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stderr(&out),
        "ERROR: sbatch がジョブを投げられなかった:\nsbatch: error: Batch job submission failed\n"
    );
    assert_eq!(
        stdout(&out),
        "RESULT: job=- name=xlc-2G exit=1 state=NOT_SUBMITTED\n"
    );
}

#[test]
fn an_interrupted_run_prints_the_result_leaves_no_temporary_file_and_ends_with_130() {
    let slurm = Slurm::new();
    let pid_file = slurm.root.join("sbatch.pid");
    for signal in ["INT", "TERM", "HUP"] {
        let cmd = slurm.command(
            &slurm.proj(),
            &with_purpose(&["-t", "5", "xlc-2G", "true"]),
            &[("FAKE_SBATCH_PID", &sh_path(&pid_file))],
        );
        // 番号を読んで並べ直した後の、sbatch の終わりを待つところで中断する
        let (mut child, mut seen, lines) = start_waiting(cmd, &pid_file);
        kill(signal, &child.id().to_string());
        seen.extend(lines.map(Result::unwrap));
        let code = child.wait().unwrap().code();
        let sbatch = std::fs::read_to_string(&pid_file).unwrap();
        kill("TERM", sbatch.trim());
        assert_eq!(code, Some(130), "{signal}");
        assert_eq!(
            seen.first().map(String::as_str),
            Some("JOB: 42"),
            "{signal}: {seen:?}"
        );
        assert_eq!(
            seen.last().map(String::as_str),
            Some("RESULT: job=42 name=xlc-2G exit=130 state=unknown"),
            "{signal}: {seen:?}"
        );
        assert_eq!(seen.iter().filter(|l| l.starts_with("RESULT:")).count(), 1);
        let left: Vec<_> = std::fs::read_dir(slurm.root.join("tmp")).unwrap().collect();
        assert!(left.is_empty(), "{signal}: {left:?}");
    }
}

/// 起動して、番号を読んで並べ直した後の、sbatch の終わりを待つところまで進める。返すのは子と、そこまでの行
fn start_waiting(
    mut cmd: Command,
    pid_file: &Path,
) -> (
    std::process::Child,
    Vec<String>,
    std::io::Lines<BufReader<std::process::ChildStdout>>,
) {
    let _ = std::fs::remove_file(pid_file);
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut seen = Vec::new();
    for line in lines.by_ref() {
        let line = line.unwrap();
        let next = line.starts_with("NEXT:");
        seen.push(line);
        if next {
            break;
        }
    }
    while !pid_file.exists() {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    (child, seen, lines)
}

fn kill(signal: &str, pid: &str) {
    let status = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn a_signal_ignored_at_start_stays_ignored_like_a_background_command_of_sh() {
    let slurm = Slurm::new();
    let pid_file = slurm.root.join("sbatch.pid");
    // ジョブ制御の無いシェルが & で起動したコマンドと同じく、INT を無視して起動する
    let mut sh = Command::new("sh");
    sh.arg("-c")
        .arg("trap '' INT; exec \"$0\" \"$@\"")
        .arg(env!("CARGO_BIN_EXE_claude-hooks"))
        .args(["script", "slurm-run"])
        .args(with_purpose(&["-t", "5", "xlc-2G", "true"]));
    slurm.prepare(
        &mut sh,
        &slurm.proj(),
        &[("FAKE_SBATCH_PID", &sh_path(&pid_file))],
    );
    let (mut child, _, lines) = start_waiting(sh, &pid_file);
    let pid = child.id().to_string();
    kill("INT", &pid);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(
        child.try_wait().unwrap().is_none(),
        "無視した INT で終わった"
    );
    kill("TERM", &pid);
    let rest: Vec<String> = lines.map(Result::unwrap).collect();
    let code = child.wait().unwrap().code();
    let sbatch = std::fs::read_to_string(&pid_file).unwrap();
    kill("TERM", sbatch.trim());
    assert_eq!(code, Some(130));
    assert_eq!(
        rest.last().map(String::as_str),
        Some("RESULT: job=42 name=xlc-2G exit=130 state=unknown")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn sbatch_ignores_the_terminal_interrupt_like_a_background_command_of_sh() {
    let slurm = Slurm::new();
    let mask_file = slurm.root.join("sigign");
    let out = slurm.run(
        &["-t", "5", "xlc-2G", "true"],
        &[("FAKE_SBATCH_SIGIGN", &sh_path(&mask_file))],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let line = std::fs::read_to_string(&mask_file).unwrap();
    let mask = u64::from_str_radix(line.trim().trim_start_matches("SigIgn:").trim(), 16).unwrap();
    // INT は 2、QUIT は 3（ビットは番号 - 1）
    assert_eq!(mask & 0b110, 0b110, "{line}");
}
