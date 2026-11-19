//! `claude-hooks script slurm-queue [--order [--first <番号>]]` が、squeue を子のプロセスで読み、一覧か並べ直す列を出し、
//! squeue の失敗をその終了コードで返すことを確かめる（ADR-0101。`~/.claude/scripts/slurm-queue.sh` が呼ぶ）。並べ方の
//! 場合分けは src/script/slurm_queue.rs の単体のテストが確かめる。squeue は偽のコマンド（examples/fake_tool.rs）を
//! PATH の先頭に置く

mod common;

use common::{fake_tool, place_fake_tool};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const C: &str = "feat/x /home/u/github/xlc-feat-x claude=abc";

/// 偽の squeue を置いた一時のディレクトリ
struct FakeSqueue {
    root: PathBuf,
}

impl Drop for FakeSqueue {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl FakeSqueue {
    fn new() -> FakeSqueue {
        assert!(
            fake_tool().is_file(),
            "{} が無い（cargo build --examples を先に流す）",
            fake_tool().display()
        );
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "fake-squeue-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("bin")).unwrap();
        FakeSqueue { root }
    }

    /// 偽の squeue を置く。置かなければ、PATH に squeue が無い
    fn with_squeue(self) -> FakeSqueue {
        place_fake_tool(
            &self
                .root
                .join("bin")
                .join(format!("squeue{}", std::env::consts::EXE_SUFFIX)),
        );
        self
    }

    /// 偽の squeue が squeue_out を標準出力に書いて squeue_exit で終わるとき、`claude-hooks script slurm-queue <args>` を
    /// 起動する。PATH は偽の squeue の置き場だけにする（手元の本物の squeue を読まない）
    fn run(&self, args: &[&str], squeue_out: &str, squeue_exit: i32) -> Output {
        Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
            .args(["script", "slurm-queue"])
            .args(args)
            .env("PATH", self.root.join("bin"))
            .env("FAKE_SQUEUE_LOG", self.root.join("squeue.log"))
            .env("FAKE_SQUEUE_OUT", squeue_out)
            .env("FAKE_SQUEUE_EXIT", squeue_exit.to_string())
            .output()
            .unwrap()
    }

    /// squeue に渡した引数（1 回の起動で 1 行）
    fn log(&self) -> String {
        std::fs::read_to_string(self.root.join("squeue.log")).unwrap_or_default()
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// main で待つジョブ（番号、名前、優先度、区分）の待ち行列を、`squeue --json=v0.0.45` の形で
fn queue(jobs: &[(u64, &str, u64, &str)]) -> String {
    let number = |n: u64| json!({"set": true, "infinite": false, "number": n});
    let unset = json!({"set": false, "infinite": false, "number": 0});
    let jobs: Vec<_> = jobs
        .iter()
        .map(|(id, name, priority, class)| {
            json!({
                "job_id": id,
                "name": name,
                "job_state": ["PENDING"],
                "partition": "main",
                "priority": number(*priority),
                "time_limit": number(10),
                "start_time": number(0),
                "comment": format!("{class} | #1 | p | {C}"),
                "array_job_id": number(0),
                "array_task_id": unset,
                "array_task_string": "",
                "het_job_id": number(0),
                "het_job_offset": unset,
            })
        })
        .collect();
    json!({"jobs": jobs}).to_string()
}

/// main で待つ trial の 20 と、その後ろの record の 21（並べ直しが要る）
fn out_of_order() -> String {
    queue(&[(20, "t", 10010, "trial"), (21, "r", 10005, "record")])
}

fn empty() -> String {
    queue(&[])
}

#[test]
fn reads_squeue_as_json_of_the_fixed_version() {
    let fake = FakeSqueue::new().with_squeue();
    let out = fake.run(&[], &empty(), 0);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(fake.log(), "squeue --json=v0.0.45\n");
}

#[test]
fn an_unreadable_squeue_output_is_an_error_with_exit_code_1() {
    let fake = FakeSqueue::new().with_squeue();
    for bad in [
        "",
        "20|main|t|PENDING|10010|10:00|N/A|c",
        r#"{"jobs": [{"job_id": 20}]}"#,
    ] {
        let out = fake.run(&[], bad, 0);
        assert_eq!(out.status.code(), Some(1), "{bad}");
        assert_eq!(stdout(&out), "", "{bad}");
        assert!(
            stderr(&out).contains("ERROR: squeue の出力を読めない: "),
            "{bad}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn lists_the_jobs_with_the_order_line() {
    let fake = FakeSqueue::new().with_squeue();
    let out = fake.run(&[], &out_of_order(), 0);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.starts_with(&format!(
            "20 PENDING main 10010 trial 10:00 N/A t #1\n  目的: p\n  連絡先: {C}\n21 PENDING main 10005 record"
        )),
        "{text}"
    );
    assert!(
        text.contains(
            "ORDER: main: 21（record）が 20（trial）の後ろで待っている。scontrol top 21,20\nNEXT: "
        ),
        "{text}"
    );
}

#[test]
fn order_first_prints_the_list_to_pass_to_scontrol_top() {
    let fake = FakeSqueue::new().with_squeue();
    let out = fake.run(&["--order", "--first", "20"], &out_of_order(), 0);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "21,20\n");
}

#[test]
fn an_empty_queue_says_so_and_order_prints_nothing() {
    let fake = FakeSqueue::new().with_squeue();
    let out = fake.run(&[], &empty(), 0);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "待ち行列にジョブが無い\n");
    let out = fake.run(&["--order"], &empty(), 0);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn a_failing_squeue_ends_with_its_exit_code() {
    let fake = FakeSqueue::new().with_squeue();
    let out = fake.run(&[], &out_of_order(), 3);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(stdout(&out), "");
    assert!(
        stderr(&out).contains("ERROR: squeue が失敗した（終了コード 3）"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_missing_squeue_ends_with_127_like_sh() {
    let fake = FakeSqueue::new();
    let out = fake.run(&[], "", 0);
    assert_eq!(out.status.code(), Some(127), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("ERROR: squeue が失敗した（終了コード 127）"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn other_arguments_print_the_usage_without_reading_squeue() {
    let fake = FakeSqueue::new().with_squeue();
    for bad in [
        &["--all"][..],
        &["--first", "42"],
        &["--order", "--first"],
        &["--order", "extra"],
    ] {
        let out = fake.run(bad, "", 0);
        assert_eq!(out.status.code(), Some(1), "{bad:?}");
        assert!(stderr(&out).contains("Usage:"), "{bad:?}: {}", stderr(&out));
    }
    assert_eq!(fake.log(), "");
}
