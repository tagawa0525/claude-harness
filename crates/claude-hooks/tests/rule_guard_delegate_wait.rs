//! guard-delegate-wait が、/delegate の担当（hook の入力に agent_id がある呼び出し）の slurm-run.sh を、終わりの見込みが
//! 55 分を超えるか分からなければ止め、slurm-wait.sh を止め、親の呼び出しは通すこと（ADR-0078 の 2026-11-20 の変更）。
//! 見込みの計算の場合分けは src/script/delegate_wait.rs の単体のテストが確かめる

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const RUN: &str =
    "cd ~/github/xlc && ~/.claude/scripts/slurm-run.sh --class trial --ref '#1' --purpose 'p'";

/// 偽の squeue を置くディレクトリ（無ければ squeue の無い PATH）
struct Bin {
    dir: PathBuf,
}

impl Drop for Bin {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Bin {
    fn new() -> Bin {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "guard-delegate-wait-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Bin { dir }
    }

    /// squeue の偽物が json を出す（PATH はこのディレクトリだけなので、シェルの組み込みの printf で書く）
    #[cfg(unix)]
    fn squeue(&self, json: &str) {
        assert!(!json.contains('\''));
        common::write_executable(
            &self.dir.join("squeue"),
            format!("#!/bin/sh\nprintf '%s\\n' '{json}'\n"),
        );
    }

    /// command を guard-delegate-wait だけに掛け、止めた理由を返す（止めなければ None）。agent が Some なら、その
    /// agent_id を入力に入れる。PATH はこのディレクトリだけ（squeue を置かなければ squeue が無い）
    fn deny_reason(&self, command: &str, agent: Option<&str>) -> Option<String> {
        let mut input = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": command, "run_in_background": true},
            "cwd": env!("CARGO_TARGET_TMPDIR"),
        });
        if let Some(agent) = agent {
            input["agent_id"] = serde_json::json!(agent);
            input["agent_type"] = serde_json::json!("general-purpose");
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
            .args(["pre-tool-use", "--rule", "guard-delegate-wait"])
            .env("PATH", &self.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        let stdout = String::from_utf8(out.stdout).unwrap();
        if stdout.trim().is_empty() {
            return None;
        }
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        json["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .map(str::to_string)
    }
}

#[cfg(unix)]
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// 待ち行列の 1 本（番号、状態、開始の見込みの UNIX の秒（0 は見込みが無い）、見積もりの分（None は無制限））
#[cfg(unix)]
fn queue(jobs: &[(u64, &str, u64, Option<u64>)]) -> String {
    let number = |n: u64| serde_json::json!({"set": true, "infinite": false, "number": n});
    let unset = serde_json::json!({"set": false, "infinite": false, "number": 0});
    let infinite = serde_json::json!({"set": false, "infinite": true, "number": 0});
    let jobs: Vec<_> = jobs
        .iter()
        .map(|(id, state, start, limit)| {
            serde_json::json!({
                "job_id": id,
                "name": "j",
                "job_state": [state],
                "partition": "main",
                "priority": number(10000),
                "time_limit": limit.map_or(infinite.clone(), number),
                "start_time": number(*start),
                "comment": "trial | #1 | p | c",
                "array_job_id": number(0),
                "array_task_id": unset,
                "array_task_string": "",
                "het_job_id": number(0),
                "het_job_offset": unset,
            })
        })
        .collect();
    serde_json::json!({"jobs": jobs}).to_string()
}

#[test]
fn the_parent_without_an_agent_id_is_not_checked() {
    // squeue が無くても、親の呼び出しは見ない
    let bin = Bin::new();
    assert_eq!(bin.deny_reason(&format!("{RUN} -t 600 x true"), None), None);
    assert_eq!(
        bin.deny_reason(&format!("{RUN} -t 600 x true"), Some("")),
        None
    );
    assert_eq!(
        bin.deny_reason("~/.claude/scripts/slurm-wait.sh 42", None),
        None
    );
}

#[cfg(unix)]
#[test]
fn an_agent_job_that_ends_within_55_minutes_passes() {
    let bin = Bin::new();
    bin.squeue(&queue(&[(961, "RUNNING", now() - 600, Some(30))]));
    // 走っているジョブの残りが 20 分、見積もり 30 分
    assert_eq!(
        bin.deny_reason(&format!("{RUN} -t 30 x true"), Some("a1")),
        None
    );
    bin.squeue(&queue(&[]));
    assert_eq!(
        bin.deny_reason(&format!("{RUN} -t 50 x true"), Some("a1")),
        None
    );
}

#[cfg(unix)]
#[test]
fn an_agent_job_that_ends_after_55_minutes_is_denied_with_the_outlook_and_what_to_do() {
    let bin = Bin::new();
    bin.squeue(&queue(&[
        (961, "RUNNING", now() - 600, Some(30)),
        (962, "PENDING", now() + 1200, Some(20)),
    ]));
    let reason = bin
        .deny_reason(&format!("{RUN} -t 30 x true"), Some("a1"))
        .expect("止まらない");
    // 待ち行列の最も遅い終わりは 40 分後（ジョブ 962）、見積もり 30 分で 70 分
    assert!(reason.contains("今から 40 分"), "{reason}");
    assert!(reason.contains("ジョブ 962"), "{reason}");
    assert!(reason.contains("から 70 分"), "{reason}");
    assert!(reason.contains("見積もり（-t）の合計 30 分"), "{reason}");
    assert!(reason.contains("投げずに"), "{reason}");
    assert!(
        reason.contains("「続きの作業」の「親が投げるコマンド」"),
        "{reason}"
    );
    assert!(reason.contains("報告して終えて"), "{reason}");
    // 一緒に投げるジョブの見積もりは足す
    bin.squeue(&queue(&[]));
    let reason = bin
        .deny_reason(
            &format!("{RUN} -t 30 a true && {RUN} -t 30 b true"),
            Some("a1"),
        )
        .expect("止まらない");
    assert!(reason.contains("見積もり（-t）の合計 60 分"), "{reason}");
}

#[cfg(unix)]
#[test]
fn an_agent_job_with_an_unknown_outlook_is_denied_with_the_reason() {
    let bin = Bin::new();
    for (json, why) in [
        (
            queue(&[(962, "PENDING", 0, Some(20))]),
            "開始の見込みが無い",
        ),
        (
            queue(&[(961, "RUNNING", now() - 60, None)]),
            "見積もりが無い",
        ),
        ("not json".to_string(), "JSON"),
    ] {
        bin.squeue(&json);
        let reason = bin
            .deny_reason(&format!("{RUN} -t 5 x true"), Some("a1"))
            .expect("止まらない");
        assert!(reason.contains("見込みが分からない"), "{reason}");
        assert!(reason.contains(why), "{reason}");
    }
    bin.squeue(&queue(&[]));
    let reason = bin
        .deny_reason(&format!("{RUN} -t \"$T\" x true"), Some("a1"))
        .expect("止まらない");
    assert!(reason.contains("字面で決まらない"), "{reason}");
}

#[test]
fn an_agent_job_without_squeue_is_denied() {
    let bin = Bin::new();
    let reason = bin
        .deny_reason(&format!("{RUN} -t 5 x true"), Some("a1"))
        .expect("止まらない");
    assert!(reason.contains("squeue を起動できない"), "{reason}");
}

#[test]
fn an_agent_calling_slurm_wait_is_denied_and_told_to_use_wait_step() {
    let bin = Bin::new();
    for command in [
        "~/.claude/scripts/slurm-wait.sh 42",
        "cd /w && bash ~/.claude/scripts/slurm-wait.sh 42",
    ] {
        let reason = bin.deny_reason(command, Some("a1")).expect("止まらない");
        assert!(reason.contains("wait-step.sh"), "{reason}");
        assert!(reason.contains("親"), "{reason}");
    }
    // 同名のテストは Slurm を待たない
    assert_eq!(
        bin.deny_reason("bash claude/tests/scripts/slurm-wait.sh", Some("a1")),
        None
    );
}

#[test]
fn other_commands_of_an_agent_pass() {
    let bin = Bin::new();
    for command in [
        "cargo test",
        "echo slurm-run.sh",
        "~/.claude/scripts/slurm-queue.sh",
    ] {
        assert_eq!(bin.deny_reason(command, Some("a1")), None, "{command}");
    }
}
