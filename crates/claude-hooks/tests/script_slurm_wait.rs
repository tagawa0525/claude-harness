//! `claude-hooks script slurm-wait` が、既にあるジョブの終わりを偽の scontrol と sacct で待ち、slurm-run と同じ終わりの行
//! （ログの末尾と TIME:、RESULT:、EXIT:）を出し、ジョブの成否で終わることを確かめる（ADR-0078 の 2026-11-20 の変更。
//! `~/.claude/scripts/slurm-wait.sh` が呼ぶ）。コメントの読み方は src/script/slurm_wait.rs、TIME: の印と終わりの State は
//! src/script/slurm_run.rs と slurm.rs の単体のテストが確かめる。
//!
//! 偽の Slurm のコマンドは sh のスクリプトで、呼ばれた回数で応答を変える。Slurm のサブコマンドは r995（Linux）でだけ
//! 動けばよい（ADR-0101 の決定の要因）ので、Unix でだけ確かめる

#![cfg(unix)]

mod common;

use common::{sh_path, write_executable};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// scontrol show job <番号> の偽物。FAKE_FORGOTTEN なら覚えていない（空の jobs）。ほかは、その番号で呼ばれて FAKE_RUNNING
/// 回（番号ごとの FAKE_RUNNING_<番号> があればそれ）までは RUNNING、その後は FAKE_STATE（既定 COMPLETED）と終了コード
/// FAKE_EXIT_<番号>（無ければ FAKE_EXIT、既定 0）の終わったジョブを返す。FAKE_FAILING 回までは失敗する（制御のデーモンの
/// 一時の不調）。呼ばれた番号は scontrol.count に足す
const SCONTROL: &str = r#"#!/bin/sh
job=$3
n=$(($(cat "$T/scontrol.$job" 2>/dev/null || echo 0) + 1))
echo "$n" >"$T/scontrol.$job"
echo "$job" >>"$T/scontrol.count"
[ "$n" -gt "${FAKE_FAILING:-0}" ] || { echo 'slurm_load_jobs error: Socket timed out' >&2; exit 1; }
if [ -n "$FAKE_FORGOTTEN" ]; then
  echo '{"jobs":[]}'
  exit 0
fi
eval "running=\${FAKE_RUNNING_$job:-\${FAKE_RUNNING:-0}}"
eval "exit=\${FAKE_EXIT_$job:-\${FAKE_EXIT:-0}}"
state=${FAKE_STATE:-COMPLETED}
[ "$n" -gt "$running" ] || state=RUNNING
printf '{"jobs":[{"job_id":%s,"name":"xlc-2G","comment":"%s","stdout_expanded":"%s/github/slurm-logs/xlc-2G-%s.out","job_state":["%s"],"exit_code":{"status":["X"],"return_code":{"set":true,"infinite":false,"number":%s},"signal":{"id":{"set":false,"infinite":false,"number":0},"name":""}}}]}\n' \
  "$job" "${FAKE_COMMENT-trial | #1 | 試験の目的 | feat/x /w commit=abc}" "$HOME" "$job" "$state" "$exit"
"#;

/// sacct の偽物（最後の引数が番号）。FAKE_UNKNOWN なら記録が無い。ほかは FAKE_STATE と FAKE_EXIT_<番号>（無ければ
/// FAKE_EXIT）の終わった記録を返す
const SACCT: &str = r#"#!/bin/sh
for job; do :; done
[ -z "$FAKE_UNKNOWN" ] || { echo '{"jobs":[]}'; exit 0; }
eval "exit=\${FAKE_EXIT_$job:-\${FAKE_EXIT:-0}}"
printf '{"jobs":[{"job_id":%s,"name":"xlc-2G","partition":"main","stdout_expanded":"%s/github/slurm-logs/xlc-2G-%s.out","state":{"current":["%s"]},"exit_code":{"status":["X"],"return_code":{"set":true,"infinite":false,"number":%s},"signal":{"id":{"set":false,"infinite":false,"number":0},"name":""}},"time":{"elapsed":754,"limit":{"set":true,"infinite":false,"number":30},"suspended":0}}]}\n' \
  "$job" "$HOME" "$job" "${FAKE_STATE:-COMPLETED}" "$exit"
"#;

const HOSTNAME: &str = "#!/bin/sh\necho r995\n";

/// 偽の Slurm と HOME（ログの置き場）を置いた一時のディレクトリ
struct Slurm {
    root: PathBuf,
}

impl Drop for Slurm {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Slurm {
    fn new() -> Slurm {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "fake-slurm-wait-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["bin", "home/github/slurm-logs"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for (name, body) in [
            ("scontrol", SCONTROL),
            ("sacct", SACCT),
            ("hostname", HOSTNAME),
        ] {
            write_executable(&root.join("bin").join(name), body);
        }
        Slurm { root }
    }

    fn log(&self) -> PathBuf {
        self.root.join("home/github/slurm-logs/xlc-2G-42.out")
    }

    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
        cmd.args(["script", "slurm-wait"])
            .args(args)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    sh_path(&self.root.join("bin")),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("T", &self.root)
            .env("HOME", self.root.join("home"))
            .env("SLURM_WAIT_INTERVAL", "0")
            .env("SLURM_RUN_SACCT_INTERVAL", "0")
            .env_remove("SLURM_RUN_NFS_ENTRY");
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd.output().unwrap()
    }

    /// scontrol の呼ばれた番号を順に
    fn polls(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("scontrol.count"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn log_of(&self, job: &str) -> PathBuf {
        self.root
            .join(format!("home/github/slurm-logs/xlc-2G-{job}.out"))
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_running_job_is_waited_for_and_ends_with_the_lines_of_slurm_run() {
    let s = Slurm::new();
    std::fs::write(s.log(), "job output\n").unwrap();
    let out = s.run(&["42"], &[("FAKE_RUNNING", "3")]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{text}{}", stderr(&out));
    // 走っている間は見続け、終わった 4 回目で止める（終わりの後に State を確かめる読み直しは数えない）
    assert!(s.polls().len() >= 4, "{:?}", s.polls());
    let log = sh_path(&s.log());
    assert!(
        text.starts_with(&format!("JOB: 42\nLOG: {log}\n")),
        "{text}"
    );
    assert!(text.contains("job output\n"), "{text}");
    assert!(
        text.contains(
            "TIME: job=42 elapsed=00:12:34 estimate=00:30:00 preemptible: 割り込まれうるので時間は記録に使わない（記録するなら排他の --timed） state=COMPLETED class=trial ref=#1 purpose=試験の目的"
        ),
        "{text}"
    );
    assert!(
        text.contains("RESULT: job=42 name=xlc-2G exit=0 state=COMPLETED\n"),
        "{text}"
    );
    assert!(text.ends_with("EXIT: 0\n"), "{text}");
    // TIME: の行はログにも書き足す
    assert!(
        std::fs::read_to_string(s.log())
            .unwrap()
            .contains("TIME: job=42 ")
    );
}

#[test]
fn the_exit_code_is_the_job_exit_code_and_a_job_not_completed_fails() {
    let s = Slurm::new();
    std::fs::write(s.log(), "job output\n").unwrap();
    let out = s.run(&["42"], &[("FAKE_STATE", "FAILED"), ("FAKE_EXIT", "3")]);
    assert_eq!(out.status.code(), Some(3), "{}", stdout(&out));
    assert!(stdout(&out).contains("RESULT: job=42 name=xlc-2G exit=3 state=FAILED"));
    assert!(
        stdout(&out).ends_with("EXIT: 3 (State=FAILED)\n"),
        "{}",
        stdout(&out)
    );

    // 取り消されたジョブは終了コード 0 でも失敗（slurm-run と同じ）
    let s = Slurm::new();
    let out = s.run(&["42"], &[("FAKE_STATE", "CANCELLED")]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(stdout(&out).ends_with("EXIT: 1 (State=CANCELLED)\n"));
}

#[test]
fn a_time_line_already_written_by_slurm_run_is_not_written_again() {
    let s = Slurm::new();
    let written = "job output\nTIME: job=42 elapsed=00:12:34 estimate=00:30:00 state=COMPLETED class=trial ref=#1 purpose=試験の目的\n";
    std::fs::write(s.log(), written).unwrap();
    let out = s.run(&["42"], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    assert_eq!(std::fs::read_to_string(s.log()).unwrap(), written);
    // ログの末尾に出る TIME: の行は 1 つ
    assert_eq!(
        stdout(&out).matches("TIME: job=42 ").count(),
        1,
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_shared_job_gets_the_shared_mark_from_its_comment() {
    let s = Slurm::new();
    let out = s.run(
        &["42"],
        &[(
            "FAKE_COMMENT",
            "trial | #1 | p | feat/x /w commit=abc shared-cpus=8",
        )],
    );
    assert!(
        stdout(&out).contains("shared (-c 8): 時間は当てにならない"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_job_the_controller_forgot_is_read_from_the_accounting_record() {
    let s = Slurm::new();
    std::fs::write(s.log(), "job output\n").unwrap();
    let out = s.run(
        &["42"],
        &[
            ("FAKE_FORGOTTEN", "1"),
            ("FAKE_STATE", "FAILED"),
            ("FAKE_EXIT", "2"),
        ],
    );
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(2), "{text}{}", stderr(&out));
    assert!(text.starts_with("JOB: 42\nLOG: "), "{text}");
    assert!(text.contains("class=- ref=- purpose=-"), "{text}");
    assert!(
        stderr(&out).contains("コメントを読めない"),
        "{}",
        stderr(&out)
    );
    assert!(
        text.contains("RESULT: job=42 name=xlc-2G exit=2 state=FAILED"),
        "{text}"
    );
}

#[test]
fn an_unknown_job_exits_1_with_the_reason_and_a_result_line() {
    let s = Slurm::new();
    let out = s.run(&["42"], &[("FAKE_FORGOTTEN", "1"), ("FAKE_UNKNOWN", "1")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("ERROR: ジョブ 42 が"),
        "{}",
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("RESULT: job=42 name=- exit=1 state=unknown"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_missing_or_non_numeric_job_prints_the_usage() {
    let s = Slurm::new();
    for args in [&[][..], &["x42"][..], &["42", "x"][..], &[""][..]] {
        let out = s.run(args, &[]);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&out).contains("Usage: slurm-wait.sh"),
            "{args:?}: {}",
            stderr(&out)
        );
        assert!(s.polls().is_empty());
    }
}

#[test]
fn several_jobs_end_once_with_their_lines_in_number_order_and_fail_if_one_fails() {
    let s = Slurm::new();
    for job in ["964", "965", "1000"] {
        std::fs::write(s.log_of(job), format!("output of {job}\n")).unwrap();
    }
    // 965 は 3 回、1000 は 1 回、964 はすぐ終わる。965 だけ失敗する
    let out = s.run(
        &["965", "1000", "964", "965"],
        &[
            ("FAKE_RUNNING_965", "3"),
            ("FAKE_RUNNING_1000", "1"),
            ("FAKE_EXIT_965", "4"),
        ],
    );
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(4), "{text}{}", stderr(&out));
    // 待っている間も、終わったジョブを読み直さない
    let polls = s.polls();
    assert_eq!(polls.iter().filter(|j| *j == "964").count(), 1, "{polls:?}");
    assert!(
        polls.iter().filter(|j| *j == "965").count() >= 4,
        "{polls:?}"
    );
    // 終わりの行は番号の順に、ジョブごとに 1 組
    let results: Vec<&str> = text.lines().filter(|l| l.starts_with("RESULT:")).collect();
    assert_eq!(
        results,
        [
            "RESULT: job=964 name=xlc-2G exit=0 state=COMPLETED",
            "RESULT: job=965 name=xlc-2G exit=4 state=COMPLETED",
            "RESULT: job=1000 name=xlc-2G exit=0 state=COMPLETED",
        ],
        "{text}"
    );
    assert_eq!(text.matches("EXIT: ").count(), 3, "{text}");
    for job in ["964", "965", "1000"] {
        assert_eq!(
            text.matches(&format!("TIME: job={job} ")).count(),
            1,
            "{text}"
        );
    }
    let order: Vec<usize> = ["output of 964", "output of 965", "output of 1000"]
        .iter()
        .map(|t| text.find(t).unwrap())
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");
}

#[test]
fn a_failing_scontrol_is_read_again_instead_of_taken_as_a_forgotten_job() {
    let s = Slurm::new();
    std::fs::write(s.log(), "job output\n").unwrap();
    let out = s.run(
        &["42"],
        &[
            ("FAKE_FAILING", "2"),
            ("FAKE_EXIT", "3"),
            ("FAKE_STATE", "FAILED"),
        ],
    );
    // 会計の記録に頼らず、制御のデーモンの終了コードで終わる
    assert_eq!(
        out.status.code(),
        Some(3),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(stderr(&out).contains("読み直す"), "{}", stderr(&out));
    assert!(s.polls().len() >= 3, "{:?}", s.polls());
    assert!(stdout(&out).contains("class=trial"), "{}", stdout(&out));
}
