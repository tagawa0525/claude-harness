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

/// scontrol show job の偽物。FAKE_FORGOTTEN なら覚えていない（空の jobs）。ほかは、呼ばれて FAKE_RUNNING 回までは
/// RUNNING、その後は FAKE_STATE（既定 COMPLETED）と FAKE_EXIT（既定 0）の終わったジョブを返す
const SCONTROL: &str = r#"#!/bin/sh
n=$(($(cat "$T/scontrol.count" 2>/dev/null || echo 0) + 1))
echo "$n" >"$T/scontrol.count"
if [ -n "$FAKE_FORGOTTEN" ]; then
  echo '{"jobs":[]}'
  exit 0
fi
state=${FAKE_STATE:-COMPLETED}
[ "$n" -gt "${FAKE_RUNNING:-0}" ] || state=RUNNING
printf '{"jobs":[{"job_id":42,"name":"xlc-2G","comment":"%s","stdout_expanded":"%s/github/slurm-logs/xlc-2G-42.out","job_state":["%s"],"exit_code":{"status":["X"],"return_code":{"set":true,"infinite":false,"number":%s},"signal":{"id":{"set":false,"infinite":false,"number":0},"name":""}}}]}\n' \
  "${FAKE_COMMENT-trial | #1 | 試験の目的 | feat/x /w commit=abc}" "$HOME" "$state" "${FAKE_EXIT:-0}"
"#;

/// sacct の偽物。FAKE_UNKNOWN なら記録が無い。ほかは FAKE_STATE と FAKE_EXIT の終わった記録を返す
const SACCT: &str = r#"#!/bin/sh
[ -z "$FAKE_UNKNOWN" ] || { echo '{"jobs":[]}'; exit 0; }
printf '{"jobs":[{"job_id":42,"name":"xlc-2G","partition":"main","stdout_expanded":"%s/github/slurm-logs/xlc-2G-42.out","state":{"current":["%s"]},"exit_code":{"status":["X"],"return_code":{"set":true,"infinite":false,"number":%s},"signal":{"id":{"set":false,"infinite":false,"number":0},"name":""}},"time":{"elapsed":754,"limit":{"set":true,"infinite":false,"number":30},"suspended":0}}]}\n' \
  "$HOME" "${FAKE_STATE:-COMPLETED}" "${FAKE_EXIT:-0}"
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

    fn polls(&self) -> u32 {
        std::fs::read_to_string(self.root.join("scontrol.count"))
            .map_or(0, |n| n.trim().parse().unwrap())
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
#[ignore = "RED: slurm-wait が未実装"]
fn a_running_job_is_waited_for_and_ends_with_the_lines_of_slurm_run() {
    let s = Slurm::new();
    std::fs::write(s.log(), "job output\n").unwrap();
    let out = s.run(&["42"], &[("FAKE_RUNNING", "3")]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{text}{}", stderr(&out));
    // 走っている間は見続け、終わった 4 回目で止める（終わりの後に State を確かめる読み直しは数えない）
    assert!(s.polls() >= 4, "{}", s.polls());
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
#[ignore = "RED: slurm-wait が未実装"]
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
#[ignore = "RED: slurm-wait が未実装"]
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
#[ignore = "RED: slurm-wait が未実装"]
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
#[ignore = "RED: slurm-wait が未実装"]
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
#[ignore = "RED: slurm-wait が未実装"]
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
#[ignore = "RED: slurm-wait が未実装"]
fn a_missing_or_non_numeric_job_prints_the_usage() {
    let s = Slurm::new();
    for args in [&[][..], &["x42"][..], &["42", "43"][..], &[""][..]] {
        let out = s.run(args, &[]);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&out).contains("Usage: slurm-wait.sh"),
            "{args:?}: {}",
            stderr(&out)
        );
        assert_eq!(s.polls(), 0);
    }
}
